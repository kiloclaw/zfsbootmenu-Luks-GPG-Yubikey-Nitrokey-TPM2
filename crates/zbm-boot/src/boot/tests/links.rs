//! The boot loads and measures the files it finds inside the boot
//! environment (SB20): links are followed from the environment's root,
//! never in the image's namespace, where the image's key cache
//! `/zfsbootmenu/.keys/tank/k` holds `CACHED` and its `/etc/zfs/key` holds
//! `SECRET`; only regular files of bounded size with a UTF-8 path and no
//! control character are loaded, and measured when they fit in memory.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use zbm_crypt::measure::{Measured, parse_log};
use zbm_testkit::fakes::{FakeSystem, Loads};

use super::*;

/// The image's cached key.
const KEY: &str = "/zfsbootmenu/.keys/tank/k";
/// What it holds.
const CACHED: &str = "CACHED";
/// The kernel the tests boot.
const KERNEL: &str = "/boot/vmlinuz-6.1.10";
/// Its initramfs.
const INITRAMFS: &str = "/boot/initramfs-6.1.10.img";
/// The event log of measured boot.
const LOG: &str = "/zfsbootmenu/tpm-measure.json";

/// A machine where the boot of [`FS`] fails only at `kexec -e`, with the
/// image's key files, the kernel 6.1.10 and its initramfs, the kernel's
/// call answering `loads`; `prepare` prepares it first, its answers before
/// the others.
fn machine(loads: Loads, prepare: impl FnOnce(&FakeSystem)) -> (FakeSystem, Installed) {
    let fake = FakeSystem::new();
    let log = fake_log(&fake);
    prepare(&fake);
    fake.files.file(KEY, CACHED).file("/etc/zfs/key", "SECRET");
    fake.kernel.answer(loads);
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "-\n");
    fake.runner.respond_program("mount", 0, "");
    fake.runner.respond_program("umount", 0, "");
    fake.runner.respond("kexec -e -i", 1, "");
    fake.runner.respond_program("kexec", 0, "");
    kernel_files(&fake.files, &State::new(&fake.files, "/zfsbootmenu"), FS);
    (fake, log)
}

/// [`machine`] with a TPM, whose SHA-256 bank is active.
fn with_tpm(fake: &FakeSystem) {
    fake.files.device("/dev/tpmrm0");
    fake.runner
        .respond("tpm2_getcap pcrs", 0, "selected-pcrs:\n  - sha256: [ 0 ]\n");
    fake.runner.respond_program("tpm2_pcrextend", 0, "");
}

/// `path`, as the boot environment names it, below its mount point.
fn at(path: &str) -> PathBuf {
    mnt().join(path.trim_start_matches('/'))
}

/// Where the boot environment is mounted.
fn mnt() -> PathBuf {
    let files = zbm_testkit::fakes::FakeFiles::new();
    State::new(&files, "/zfsbootmenu")
        .environment(FS)
        .unwrap()
        .join("mnt")
}

/// Boots `kernel` with `initramfs` of [`FS`] on `fake`.
fn boot(fake: &FakeSystem, (kernel, initramfs): (&str, &str)) {
    let _secrets = crate::testing::booting();
    let state = State::new(&fake.files, "/zfsbootmenu");
    kexec_kernel(
        &fake.context(),
        &ConsoleInteraction::new(fake.system()),
        &state,
        &BootSettings::default(),
        &name(FS),
        &Kernel::new(FS, kernel, initramfs),
        &Shells::default(),
    );
}

/// The `kexec -a -l` lines `fake` ran.
fn kexec_lines(fake: &FakeSystem) -> Vec<String> {
    let lines = fake.runner.lines().into_iter();
    lines
        .filter(|line| line.starts_with("kexec -a -l"))
        .collect()
}

/// Asserts that the boot failed before anything was loaded: nothing went
/// to the kernel, the boot environment was unmounted, and the failed-boot
/// prompt showed.
fn assert_not_loaded(fake: &FakeSystem) {
    assert!(
        fake.kernel.loaded().is_empty(),
        "{:?}",
        fake.kernel.loaded()
    );
    assert_eq!(kexec_lines(fake), Vec::<String>::new());
    let umount = format!("umount -R {}", mnt().display());
    assert!(
        fake.runner.lines().contains(&umount),
        "{:#?}",
        fake.runner.lines()
    );
    let output = fake.console.output_text();
    assert!(
        output.contains("Unable to load kernel or initramfs into memory"),
        "{output}"
    );
    assert_eq!(fake.kernel.starts(), 0);
}

/// What the event log of `fake` says was measured.
fn measured(fake: &FakeSystem) -> Vec<Measured> {
    let log = fake.files.text(LOG).unwrap_or_default();
    let events = parse_log(&log).unwrap_or_default();
    events.into_iter().map(|event| event.measured).collect()
}

#[test]
fn an_initramfs_that_links_to_a_cached_key_is_not_loaded() {
    let (fake, _log) = machine(Loads::Accepted, |_| {});
    fake.files.symlink(at(INITRAMFS), KEY);

    boot(&fake, (KERNEL, INITRAMFS));

    assert_not_loaded(&fake);
    let error = format!(
        "unable to load {}: {KEY} is missing in the environment",
        at(INITRAMFS).display()
    );
    assert!(fake.kmsg().contains(&error), "{}", fake.kmsg());
}

#[test]
fn a_measured_boot_never_measures_or_copies_a_linked_key() {
    let (fake, _log) = machine(Loads::Accepted, with_tpm);
    fake.files.symlink(at(INITRAMFS), KEY);

    boot(&fake, (KERNEL, INITRAMFS));

    assert_not_loaded(&fake);
    // The separator alone: after a boot attempt that failed, what is
    // bound to PCR 9 does not unseal (ADR 0008, point 4).
    let measured = measured(&fake);
    assert!(measured.contains(&Measured::Unsealed), "{measured:?}");
    assert!(
        !measured
            .iter()
            .any(|event| matches!(event, Measured::Kernel(_) | Measured::Initramfs(_))),
        "{measured:?}"
    );
    let copied = fake.files.paths().into_iter().any(|path| {
        path.starts_with("/zfsbootmenu/measured")
            && fake
                .files
                .contents(&path)
                .is_some_and(|bytes| bytes.windows(CACHED.len()).any(|w| w == CACHED.as_bytes()))
    });
    assert!(!copied);
}

#[test]
fn a_resolved_path_that_is_not_utf8_is_never_loaded() {
    let (fake, _log) = machine(Loads::Refused(8), |_| {});
    fake.files
        .symlink(at(INITRAMFS), OsStr::from_bytes(b"/\xff/i"))
        .file(mnt().join(OsStr::from_bytes(b"\xff/i")), "")
        // The lossy text of the path would lead through this link.
        .symlink(at("/\u{fffd}"), "/zfsbootmenu/.keys/tank");

    boot(&fake, (KERNEL, INITRAMFS));

    assert_not_loaded(&fake);
    let kmsg = fake.kmsg();
    assert!(kmsg.contains("is not a UTF-8 path"), "{kmsg}");
    assert!(!kmsg.contains(".keys"), "{kmsg}");
}

#[test]
fn a_kernel_whose_path_has_a_control_character_is_never_loaded() {
    let (fake, _log) = machine(Loads::Accepted, |_| {});
    let forged = "/boot/a\n<7>ZBM:[1]|x|\x1b[2J";
    fake.files
        .symlink(at(KERNEL), forged)
        .file(at(forged), "IMAGE");

    boot(&fake, (KERNEL, INITRAMFS));

    assert_not_loaded(&fake);
    let kmsg = fake.kmsg();
    assert!(
        kmsg.contains("has a control character in its path"),
        "{kmsg}"
    );
    assert!(!kmsg.contains('\x1b'), "{kmsg}");
    assert!(
        !kmsg.lines().any(|line| line.starts_with("<7>ZBM:[1]|x|")),
        "{kmsg}"
    );
}

#[test]
fn a_kernel_that_links_inside_the_environment_loads_the_environment_s_file() {
    let (fake, _log) = machine(Loads::Accepted, |_| {});
    fake.files
        .file("/boot/real-vmlinuz", "IMAGE")
        .file(at("/boot/real-vmlinuz"), "REAL")
        .symlink(at(KERNEL), "/boot/real-vmlinuz");

    boot(&fake, (KERNEL, INITRAMFS));

    let loaded = fake.kernel.loaded();
    assert_eq!(loaded.len(), 1, "{:?}", fake.runner.lines());
    assert_eq!(loaded[0].kernel, at("/boot/real-vmlinuz"));
    assert_eq!(loaded[0].initrd, at(INITRAMFS));
    // The log names the kernel as the boot environment does.
    let named = format!(
        "loaded {} and {} into memory",
        at(KERNEL).display(),
        at(INITRAMFS).display()
    );
    assert!(fake.kmsg().contains(&named), "{}", fake.kmsg());
}

#[test]
fn the_kexec_program_gets_the_resolved_paths_when_the_call_fails() {
    // ENOEXEC: a kernel the call does not take, which the program loads.
    let (fake, _log) = machine(Loads::Refused(8), |_| {});
    fake.files
        .file(at("/boot/real-vmlinuz"), "REAL")
        .file(at("/boot/real.img"), "REALRD")
        .symlink(at(KERNEL), "/boot/real-vmlinuz")
        .symlink(at(INITRAMFS), "real.img");

    boot(&fake, (KERNEL, INITRAMFS));

    let lines = kexec_lines(&fake);
    assert_eq!(lines.len(), 1, "{:#?}", fake.runner.lines());
    let expected = format!(
        "kexec -a -l {} --initrd={} --command-line=",
        at("/boot/real-vmlinuz").display(),
        at("/boot/real.img").display()
    );
    assert!(lines[0].starts_with(&expected), "{lines:?}");
}

#[test]
fn zkexec_s_dot_dot_stays_inside_the_environment() {
    let (fake, _log) = machine(Loads::Accepted, |_| {});
    let outside = mnt().parent().unwrap().join("x");
    fake.files
        .file(&outside, "OUTSIDE")
        .file(at("/x"), "INSIDE")
        .file(at("/y"), "INSIDERD");

    boot(&fake, ("/boot/../../x", "/boot/../../y"));

    let loaded = fake.kernel.loaded();
    assert_eq!(loaded.len(), 1, "{:?}", fake.runner.lines());
    assert_eq!(loaded[0].kernel, at("/x"));
    assert_eq!(loaded[0].initrd, at("/y"));
}

#[test]
fn a_fifo_initramfs_is_not_opened() {
    let (fake, _log) = machine(Loads::Accepted, |_| {});
    fake.files.fifo(at(INITRAMFS));

    boot(&fake, (KERNEL, INITRAMFS));

    assert_not_loaded(&fake);
    assert!(
        fake.kmsg().contains("is a pipe or other special file"),
        "{}",
        fake.kmsg()
    );
}

#[test]
fn an_initramfs_over_the_limit_is_not_read_for_measuring() {
    let (fake, _log) = machine(Loads::Accepted, with_tpm);
    fake.files.sized(at(INITRAMFS), (1 << 30) + 1);

    boot(&fake, (KERNEL, INITRAMFS));

    assert_not_loaded(&fake);
    assert_eq!(measured(&fake), [Measured::Unsealed]);
    assert!(fake.kmsg().contains("more than is read"), "{}", fake.kmsg());
}

#[test]
fn files_too_large_for_memory_are_not_measured() {
    let (fake, _log) = machine(Loads::Accepted, with_tpm);
    fake.files
        .file(
            "/proc/meminfo",
            "MemTotal: 4096 kB\nMemAvailable: 2048 kB\n",
        )
        .sized(at(KERNEL), 1 << 20);

    boot(&fake, (KERNEL, INITRAMFS));

    // Unmeasured, after the separator: the boot environment's files load.
    assert_eq!(measured(&fake), [Measured::Unsealed]);
    let loaded = fake.kernel.loaded();
    assert_eq!(loaded.len(), 1, "{:?}", fake.runner.lines());
    assert_eq!(loaded[0].kernel, at(KERNEL));
    assert_eq!(loaded[0].initrd, at(INITRAMFS));
    assert!(
        fake.kmsg()
            .contains("too large to measure in the 2097152 bytes"),
        "{}",
        fake.kmsg()
    );
}

/// [`machine`] where `org.zfsbootmenu:devicetree` is `board.dtb`.
fn with_device_tree(fake: &FakeSystem) {
    fake.runner.respond(
        &format!("zfs get -H -o value org.zfsbootmenu:devicetree {FS}"),
        0,
        "board.dtb\n",
    );
}

/// The device tree of the kernel 6.1.10, as the boot environment names it.
const DTB: &str = "/boot/dtbs/dtbs-6.1.10/board.dtb";

#[test]
fn a_device_tree_that_links_to_a_key_is_not_passed_to_kexec() {
    let (fake, _log) = machine(Loads::Accepted, with_device_tree);
    fake.files.symlink(at(DTB), "/etc/zfs/key");

    boot(&fake, (KERNEL, INITRAMFS));

    // Without a device tree the call loads the kernel.
    assert_eq!(kexec_lines(&fake), Vec::<String>::new());
    assert_eq!(fake.kernel.loaded().len(), 1);
    let missing = format!("devicetree file missing: '{}'", at(DTB).display());
    assert!(fake.kmsg().contains(&missing), "{}", fake.kmsg());
}

#[test]
fn a_device_tree_that_is_a_fifo_is_warned_of_and_not_passed_to_kexec() {
    let (fake, _log) = machine(Loads::Accepted, with_device_tree);
    fake.files.fifo(at(DTB));

    boot(&fake, (KERNEL, INITRAMFS));

    assert_eq!(kexec_lines(&fake), Vec::<String>::new());
    assert_eq!(fake.kernel.loaded().len(), 1);
    let warned = format!(
        "the device tree {} is not read: {DTB} is a pipe or other special file",
        at(DTB).display()
    );
    assert!(fake.kmsg().contains(&warned), "{}", fake.kmsg());
}

#[test]
fn a_device_tree_s_lines_escape_the_property() {
    let (fake, _log) = machine(Loads::Accepted, |fake| {
        fake.runner.respond(
            &format!("zfs get -H -o value org.zfsbootmenu:devicetree {FS}"),
            0,
            "\x1b[2Jboard.dtb\n",
        );
    });
    fake.files
        .fifo(at("/boot/dtbs/dtbs-6.1.10/\x1b[2Jboard.dtb"));

    boot(&fake, (KERNEL, INITRAMFS));

    assert_eq!(fake.kernel.loaded().len(), 1);
    let kmsg = fake.kmsg();
    assert!(kmsg.contains("devicetree property set to"), "{kmsg}");
    assert!(kmsg.contains("devicetree file missing"), "{kmsg}");
    assert!(!kmsg.contains('\x1b'), "{kmsg}");
}

#[test]
fn a_device_tree_inside_the_environment_is_passed_as_before() {
    let (fake, _log) = machine(Loads::Accepted, with_device_tree);
    fake.files.file(at(DTB), "DTB");

    boot(&fake, (KERNEL, INITRAMFS));

    let lines = kexec_lines(&fake);
    assert_eq!(lines.len(), 1, "{:#?}", fake.runner.lines());
    let dtb = format!("--dtb={}", at(DTB).display());
    assert!(lines[0].contains(&dtb), "{lines:?}");
    let exists = format!("devicetree file exists: '{}'", at(DTB).display());
    assert!(fake.kmsg().contains(&exists), "{}", fake.kmsg());
}
