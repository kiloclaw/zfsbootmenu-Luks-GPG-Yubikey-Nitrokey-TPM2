//! The kernel search reads an environment's files inside its root (SB20):
//! links are followed from the environment's root, never in the image's
//! namespace, where the image's `/etc/zfs/key` holds `SECRET` and its key
//! cache `/zfsbootmenu/.keys/tank/k` holds `CACHED`; only regular files of
//! bounded size with a UTF-8 path count, names with a control character
//! are no kernels, and a `/boot` gives at most 64.

use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use zbm_platform::{Context, Installed, KernelLog};
use zbm_state::State;
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// The boot environment.
const FS: &str = "tank/ROOT/be";
/// Where it is mounted.
const MNT: &str = "/zfsbootmenu/environments/tank/ROOT/be/mnt";

/// A machine with the image's key files, an empty boot environment, and
/// the kernel log, where warnings go.
fn machine() -> (FakeSystem, Installed) {
    let fake = FakeSystem::new();
    fake.files
        .file("/etc/zfs/key", "SECRET")
        .file("/zfsbootmenu/.keys/tank/k", "CACHED")
        .dir(MNT);
    let log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1)
        .without_journal()
        .install();
    (fake, log)
}

/// `path`, as the environment names it, below its mount point.
fn at(path: &str) -> PathBuf {
    Path::new(MNT).join(path.trim_start_matches('/'))
}

/// The kernels found, with their initramfs images, as the environment
/// names them.
fn kernels(fake: &FakeSystem) -> Vec<(String, String)> {
    discover(&fake.files, FS, Path::new(MNT))
        .kernels
        .into_iter()
        .map(|kernel| (kernel.kernel, kernel.initramfs))
        .collect()
}

/// `kernel` with `initramfs`, as [`kernels`] gives them.
fn pair(kernel: &str, initramfs: &str) -> (String, String) {
    (kernel.to_owned(), initramfs.to_owned())
}

/// The lines of the kernel log at `level`.
fn lines_at(fake: &FakeSystem, level: u8) -> Vec<String> {
    let prefix = format!("<{level}>");
    fake.kmsg()
        .lines()
        .filter(|line| line.starts_with(&prefix))
        .map(str::to_owned)
        .collect()
}

/// The warnings of the kernel log.
fn warnings(fake: &FakeSystem) -> Vec<String> {
    lines_at(fake, 4)
}

#[test]
fn a_command_line_file_that_links_to_an_image_file_is_not_found() {
    let (fake, _log) = machine();
    fake.files
        .file(at("/boot/vmlinuz-1"), "")
        .file(at("/boot/initramfs-1.img"), "")
        .symlink(at("/boot/vmlinuz-1.kcl"), "/etc/zfs/key");

    let found = discover(&fake.files, FS, Path::new(MNT));

    assert_eq!(found.kcl_files, []);
    assert_eq!(found.kernels.len(), 1);
    // The file is not there in the environment: nothing to warn of.
    assert_eq!(warnings(&fake), Vec::<String>::new());
}

#[test]
fn an_initramfs_that_links_to_a_cached_key_is_not_paired() {
    let (fake, _log) = machine();
    fake.files
        .file(at("/boot/vmlinuz-1"), "")
        .symlink(at("/boot/initramfs-1.img"), "/zfsbootmenu/.keys/tank/k");

    assert_eq!(kernels(&fake), []);
}

#[test]
fn an_initramfs_that_links_inside_the_environment_is_paired() {
    let (fake, _log) = machine();
    fake.files
        .file(at("/boot/vmlinuz-1"), "")
        .file(at("/boot/real.img"), "")
        .symlink(at("/boot/initramfs-1.img"), "/boot/real.img");

    assert_eq!(
        kernels(&fake),
        [pair("/boot/vmlinuz-1", "/boot/initramfs-1.img")]
    );
}

#[test]
fn a_kernel_that_links_absolutely_inside_the_environment_is_listed() {
    let (fake, _log) = machine();
    fake.files
        .file(at("/boot/vmlinuz-6.1"), "")
        .file(at("/boot/initramfs-6.1.img"), "")
        .file(at("/boot/initramfs.img"), "")
        .symlink(at("/boot/vmlinuz"), "/boot/vmlinuz-6.1");

    assert_eq!(
        kernels(&fake),
        [
            pair("/boot/vmlinuz", "/boot/initramfs.img"),
            pair("/boot/vmlinuz-6.1", "/boot/initramfs-6.1.img"),
        ]
    );
}

#[test]
fn a_boot_directory_that_links_out_lists_the_environment_s_own_directory() {
    let (fake, _log) = machine();
    fake.files
        .file("/etc/vmlinuz-9", "")
        .file("/etc/initramfs-9.img", "")
        .symlink(at("/boot"), "/etc");
    assert_eq!(kernels(&fake), []);

    // The environment's own `/etc`, once it has kernels.
    fake.files
        .file(at("/etc/vmlinuz-2"), "")
        .file(at("/etc/initramfs-2.img"), "");
    assert_eq!(
        kernels(&fake),
        [pair("/boot/vmlinuz-2", "/boot/initramfs-2.img")]
    );
}

#[test]
fn a_boot_that_is_no_directory_is_warned_of_in_the_boot_s_words() {
    let cases = [
        ("file", "/boot is a file, not a directory"),
        (
            "fifo",
            "/boot is a pipe or other special file, not a directory",
        ),
        (
            "proc",
            "it leads into the environment's /proc, which is not read",
        ),
        ("loop", "too many symbolic links"),
    ];
    for (case, said) in cases {
        let (fake, _log) = machine();
        let boot = at("/boot");
        match case {
            "file" => fake.files.file(&boot, ""),
            "fifo" => fake.files.fifo(&boot),
            "proc" => fake
                .files
                .symlink(&boot, "/proc")
                .file(at("/proc/vmlinuz-1"), "")
                .file(at("/proc/initramfs-1.img"), ""),
            _ => fake.files.symlink(&boot, "boot"),
        };
        assert_eq!(kernels(&fake), [], "{case}");
        let warnings = warnings(&fake);
        let expected = format!("/boot of {FS} is not read: {said}");
        assert_eq!(warnings.len(), 1, "{case}: {warnings:#?}");
        assert!(warnings[0].contains(&expected), "{case}: {warnings:#?}");
    }
}

#[test]
fn a_relative_link_that_climbs_stops_at_the_environment_s_root() {
    let (fake, _log) = machine();
    let climb = "../".repeat(12);
    fake.files
        .file(at("/boot/vmlinuz-1"), "")
        .symlink(
            at("/boot/initramfs-1.img"),
            format!("{climb}zfsbootmenu/.keys/tank/k"),
        )
        .symlink(at("/boot/vmlinuz-1.kcl"), format!("{climb}etc/zfs/key"))
        .file(at("/etc/zfs/key"), "own\n");

    let found = discover(&fake.files, FS, Path::new(MNT));

    assert_eq!(found.kernels, []);
    assert_eq!(
        found.kcl_files,
        [("/boot/vmlinuz-1".to_owned(), at("/etc/zfs/key"))]
    );
}

#[test]
fn names_with_a_control_character_are_no_kernels() {
    let (fake, _log) = machine();
    fake.files
        .file(at("/boot/vmlinuz-1"), "")
        .file(at("/boot/initramfs-1.img"), "");
    for version in ["9\nq\tx\ty", "2\tz", "\x1b[2J", "3\x7f", "4\u{85}"] {
        fake.files
            .file(at(&format!("/boot/vmlinuz-{version}")), "")
            .file(at(&format!("/boot/initramfs-{version}.img")), "");
    }

    assert_eq!(
        kernels(&fake),
        [pair("/boot/vmlinuz-1", "/boot/initramfs-1.img")]
    );
    let warnings = warnings(&fake);
    assert_eq!(warnings.len(), 1, "{warnings:#?}");
    assert!(warnings[0].contains(": 5, such as"), "{warnings:#?}");
    let kmsg = fake.kmsg();
    assert!(!kmsg.contains(['\x1b', '\t', '\x7f', '\u{85}']), "{kmsg}");
}

#[test]
fn a_fifo_device_or_directory_is_no_kernel_or_initramfs() {
    let (fake, _log) = machine();
    fake.files
        .fifo(at("/boot/vmlinuz-1"))
        .file(at("/boot/initramfs-1.img"), "")
        .device(at("/boot/vmlinuz-2"))
        .file(at("/boot/initramfs-2.img"), "")
        .dir(at("/boot/vmlinuz-3"))
        .file(at("/boot/initramfs-3.img"), "")
        .file(at("/boot/vmlinuz-4"), "")
        .fifo(at("/boot/initramfs-4.img"))
        .file(at("/boot/initrd.img-4"), "");

    assert_eq!(
        kernels(&fake),
        [pair("/boot/vmlinuz-4", "/boot/initrd.img-4")]
    );
    let warnings = warnings(&fake);
    assert_eq!(warnings.len(), 3, "{warnings:#?}");
    assert!(
        warnings.iter().all(|line| !line.contains("vmlinuz-3")),
        "{warnings:#?}"
    );
    let debug = lines_at(&fake, 7);
    assert!(
        debug
            .iter()
            .any(|line| line.contains("/boot/vmlinuz-3 is a directory")),
        "{debug:#?}"
    );
}

#[test]
fn an_initramfs_over_the_limit_is_passed_over_for_the_next_candidate() {
    let (fake, _log) = machine();
    fake.files
        .file(at("/boot/vmlinuz-1"), "")
        .sized(at("/boot/initramfs-1.img"), (1 << 30) + 1)
        .file(at("/boot/initrd.img-1"), "");

    assert_eq!(
        kernels(&fake),
        [pair("/boot/vmlinuz-1", "/boot/initrd.img-1")]
    );
    // Exactly the limit is read.
    fake.files.sized(at("/boot/initramfs-1.img"), 1 << 30);
    assert_eq!(
        kernels(&fake),
        [pair("/boot/vmlinuz-1", "/boot/initramfs-1.img")]
    );
}

#[test]
fn a_path_into_the_environment_s_dev_is_refused() {
    let (fake, _log) = machine();
    fake.files
        .device("/dev/zero")
        .device(at("/dev/zero"))
        .file(at("/boot/vmlinuz-1"), "")
        .symlink(at("/boot/initramfs-1.img"), "/dev/zero");

    assert_eq!(kernels(&fake), []);
    let warnings = warnings(&fake);
    assert!(
        warnings
            .iter()
            .any(|line| line.contains("it leads into the environment's /dev")),
        "{warnings:#?}"
    );
}

#[test]
fn a_path_through_the_environment_s_zfs_directory_is_refused() {
    let (fake, _log) = machine();
    fake.files
        .file(at("/boot/vmlinuz-1"), "")
        .file(at("/.zfs/snapshot/s/boot/initramfs-1.img"), "")
        .symlink(
            at("/boot/initramfs-1.img"),
            "../.zfs/snapshot/s/boot/initramfs-1.img",
        )
        .fail(at("/.zfs"), std::io::ErrorKind::PermissionDenied);

    assert_eq!(kernels(&fake), []);
    let warnings = warnings(&fake);
    assert!(
        warnings
            .iter()
            .any(|line| line.contains("snapshots' directory .zfs")),
        "{warnings:#?}"
    );
}

#[test]
fn a_resolved_path_that_is_not_utf8_is_refused() {
    let (fake, _log) = machine();
    let odd = |prefix: &[u8]| PathBuf::from(OsStr::from_bytes(&[prefix, b"\xff/i"].concat()));
    fake.files
        .file(at("/boot/vmlinuz-1"), "")
        .symlink(at("/boot/initramfs-1.img"), odd(b"/"))
        .file(at("/boot/vmlinuz-2"), "")
        .symlink(at("/boot/initramfs-2.img"), odd(b"../"))
        .file(Path::new(MNT).join(OsStr::from_bytes(b"\xff/i")), "")
        // The lossy text of the path would lead through this link.
        .symlink(at("/\u{fffd}"), "/zfsbootmenu/.keys/tank");

    let found = discover(&fake.files, FS, Path::new(MNT));

    assert_eq!(found.kernels, []);
    assert_eq!(found.kcl_files, []);
    let warnings = warnings(&fake);
    assert_eq!(warnings.len(), 2, "{warnings:#?}");
    assert!(
        warnings
            .iter()
            .all(|line| line.contains("not a UTF-8 path")),
        "{warnings:#?}"
    );
    assert!(!fake.kmsg().contains(".keys"), "{}", fake.kmsg());
}

#[test]
fn a_link_loop_is_refused_not_followed_forever() {
    let (fake, _log) = machine();
    fake.files
        .file(at("/boot/vmlinuz-1"), "")
        .symlink(at("/boot/initramfs-1.img"), "loop")
        .symlink(at("/boot/loop"), "initramfs-1.img");

    assert_eq!(kernels(&fake), []);
    let warnings = warnings(&fake);
    assert_eq!(warnings.len(), 1, "{warnings:#?}");
    assert!(
        warnings[0].contains("/boot/initramfs-1.img: too many symbolic links"),
        "{warnings:#?}"
    );
}

#[test]
fn only_the_last_64_kernels_are_kept() {
    let (fake, _log) = machine();
    for version in 1..=70 {
        fake.files
            .file(at(&format!("/boot/vmlinuz-{version}")), "")
            .file(at(&format!("/boot/initramfs-{version}.img")), "");
    }

    let found = kernels(&fake);

    assert_eq!(found.len(), 64);
    assert_eq!(found[0], pair("/boot/vmlinuz-7", "/boot/initramfs-7.img"));
    assert_eq!(
        found.last(),
        Some(&pair("/boot/vmlinuz-70", "/boot/initramfs-70.img"))
    );
    let warnings = warnings(&fake);
    assert_eq!(warnings.len(), 1, "{warnings:#?}");
    assert!(
        warnings[0].contains("70 kernels in /boot: the 6 oldest are left out"),
        "{warnings:#?}"
    );
}

#[test]
fn initramfs_candidates_over_name_max_are_not_looked_up() {
    let (fake, _log) = machine();
    let kernel = format!("vmlinuz-{}", "x".repeat(242));
    assert_eq!(kernel.len(), 250);
    // A name of 264 bytes: no directory can hold it, though the fake can,
    // so it would be paired if it were looked up.
    let long = format!("initramfs-{kernel}.img");
    fake.files
        .file(at(&format!("/boot/{kernel}")), "")
        .file(at(&format!("/boot/{long}")), "");

    assert_eq!(kernels(&fake), []);
    assert_eq!(fake.kmsg(), "");
    // Nor is it read at all: a read of it would fail, and be warned of.
    fake.files.fail(
        at(&format!("/boot/{long}")),
        io::ErrorKind::PermissionDenied,
    );
    assert_eq!(kernels(&fake), []);
    assert_eq!(fake.kmsg(), "");
}

#[test]
fn refused_candidates_give_one_warning_per_kernel_escaped() {
    let (fake, _log) = machine();
    fake.files
        .file(at("/boot/vmlinuz-1"), "")
        .symlink(at("/boot/initramfs-1.img"), "real\x1b[2J")
        .fifo(at("/boot/real\x1b[2J"))
        .symlink(at("/boot/initrd.img-1"), "/dev/sda")
        .sized(at("/boot/initramfs-1"), 2 << 30)
        .file(at("/boot/vmlinuz-2"), "")
        .fifo(at("/boot/initramfs-2.img"));

    assert_eq!(kernels(&fake), []);
    let warnings = warnings(&fake);
    assert_eq!(warnings.len(), 2, "{warnings:#?}");
    assert!(
        warnings[0].contains("kernel /boot/vmlinuz-1 of tank/ROOT/be: 3 of its files not read"),
        "{warnings:#?}"
    );
    assert!(
        warnings[1].contains("kernel /boot/vmlinuz-2 of tank/ROOT/be: 1 of its files not read"),
        "{warnings:#?}"
    );
    let kmsg = fake.kmsg();
    assert!(
        !kmsg.contains('\x1b') && kmsg.contains("\\x1b[2J"),
        "{kmsg}"
    );
}

#[test]
fn ordinary_trees_are_found_as_before() {
    let (fake, _log) = machine();
    for name in [
        "vmlinuz-6.12.9_1",
        "initramfs-6.12.9_1.img",
        "vmlinuz-6.6.30_1",
        "initramfs-6.6.30_1.img",
        "config-6.1.0",
        "linux-lts",
        "initrd.img-linux-lts",
        "vmlinuz.old",
    ] {
        fake.files.file(at(&format!("/boot/{name}")), "");
    }
    fake.files
        .symlink(at("/boot/vmlinuz"), "vmlinuz-6.12.9_1")
        .symlink(at("/boot/initrd.img"), "initramfs-6.12.9_1.img")
        .file(at("/boot/vmlinuz-6.12.9_1.kcl"), "quiet\n");

    let found = discover(&fake.files, FS, Path::new(MNT));

    assert_eq!(
        found
            .kernels
            .iter()
            .map(|kernel| (kernel.kernel.clone(), kernel.initramfs.clone()))
            .collect::<Vec<_>>(),
        [
            pair("/boot/linux-lts", "/boot/initrd.img-linux-lts"),
            pair("/boot/vmlinuz", "/boot/initrd.img"),
            pair("/boot/vmlinuz-6.6.30_1", "/boot/initramfs-6.6.30_1.img"),
            pair("/boot/vmlinuz-6.12.9_1", "/boot/initramfs-6.12.9_1.img"),
        ]
    );
    assert_eq!(
        found.kcl_files,
        [(
            "/boot/vmlinuz-6.12.9_1".to_owned(),
            at("/boot/vmlinuz-6.12.9_1.kcl")
        )]
    );
    assert_eq!(warnings(&fake), Vec::<String>::new());
}

/// What a name of a random `/boot` is.
#[derive(Clone, Debug)]
enum Entry {
    File,
    Dir,
    Fifo,
    Link(String),
}

/// A name of a random `/boot`: kernels, initramfs images and command line
/// files of a few versions.
fn boot_name() -> impl Strategy<Value = String> {
    let base = proptest::sample::select(vec![
        "vmlinuz-",
        "linux-",
        "initramfs-",
        "initrd.img-",
        "vmlinuz",
    ]);
    let version = proptest::sample::select(vec!["1", "2", "6.1", "x"]);
    let suffix = proptest::sample::select(vec!["", ".img", ".kcl"]);
    (base, version, suffix).prop_map(|(base, version, suffix)| format!("{base}{version}{suffix}"))
}

/// What an entry is: mostly files, and links inside, out of, or around
/// the environment.
fn boot_entry() -> impl Strategy<Value = Entry> {
    prop_oneof![
        4 => Just(Entry::File),
        1 => Just(Entry::Dir),
        1 => Just(Entry::Fifo),
        2 => boot_name().prop_map(Entry::Link),
        1 => boot_name().prop_map(|name| Entry::Link(format!("/boot/{name}"))),
        1 => Just(Entry::Link("/etc/zfs/key".to_owned())),
        1 => Just(Entry::Link("../../../../../../../../etc/zfs/key".to_owned())),
    ]
}

proptest! {
    /// Pins the one walk that `discover` and `find_kernels` share.
    #[test]
    fn discover_and_find_kernels_agree(
        tree in proptest::collection::vec((boot_name(), boot_entry()), 0..12),
    ) {
        let fake = FakeSystem::new();
        fake.files.dir("/zfsbootmenu").dir(at("/boot")).file("/etc/zfs/key", "SECRET");
        for (name, entry) in &tree {
            let path = at(&format!("/boot/{name}"));
            match entry {
                Entry::File => { fake.files.file(&path, ""); }
                Entry::Dir => { fake.files.dir(&path); }
                Entry::Fifo => { fake.files.fifo(&path); }
                Entry::Link(target) => { fake.files.symlink(&path, target); }
            }
        }
        fake.runner.respond(
            &format!("zfs get -H -o value org.zfsbootmenu:kernel {FS}"),
            0,
            "-\n",
        );
        let discovered: String = discover(&fake.files, FS, Path::new(MNT))
            .kernels
            .iter()
            .map(Kernel::line)
            .collect();

        let state = State::new(&fake.files, "/zfsbootmenu");
        let fs = FS.parse().unwrap();
        let _ = crate::find_kernels(&Context::new(fake.system()), &state, &fs);
        let listed = fake.files.text(state.kernels(FS)).unwrap_or_default();

        prop_assert_eq!(listed, discovered);
        let leaked = fake.files.paths().into_iter().any(|path| {
            path.starts_with("/zfsbootmenu/environments")
                && !path.starts_with(MNT)
                && fake.files.text(&path).is_some_and(|text| text.contains("SECRET"))
        });
        prop_assert!(!leaked, "the image's key is in the state");
    }

    #[test]
    fn every_kernel_line_reads_back_as_the_kernel_it_was_written_from(
        versions in proptest::collection::vec(
            proptest::collection::vec(any::<u8>().prop_filter("a name", |byte| {
                *byte != b'/' && *byte != 0
            }), 0..12),
            0..6,
        ),
    ) {
        let fake = FakeSystem::new();
        fake.files.dir(at("/boot"));
        for version in &versions {
            let name = |base: &[u8], suffix: &[u8]| {
                at("/boot").join(OsStr::from_bytes(&[base, version, suffix].concat()))
            };
            fake.files
                .file(name(b"vmlinuz-", b""), "")
                .file(name(b"initramfs-", b".img"), "");
        }
        for kernel in discover(&fake.files, FS, Path::new(MNT)).kernels {
            prop_assert_eq!(Kernel::parse(&kernel.line()), kernel.clone());
            prop_assert!(!kernel.kernel.contains(char::is_control), "{:?}", kernel);
            prop_assert!(!kernel.initramfs.contains(char::is_control), "{:?}", kernel);
        }
    }
}
