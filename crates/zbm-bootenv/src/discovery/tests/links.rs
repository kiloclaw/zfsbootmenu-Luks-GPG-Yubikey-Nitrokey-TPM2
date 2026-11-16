//! The kernel search of `tank/a` copies only its own command line files
//! into the cache, resolved inside its root, and writes only its own
//! kernels into its kernel list (SB20): the image's `/etc/zfs/key`, which
//! holds `SECRET`, is never read.

use std::path::Path;

use zbm_platform::{Context, Installed, KernelLog};
use zbm_state::State;
use zbm_testkit::fakes::FakeSystem;

use super::{E, NoProperties, search};
use crate::commandline::be_cmdline;
use crate::select_kernel;
use crate::testing::name;

/// Where `tank/a` is mounted.
const MNT: &str = "/zfsbootmenu/environments/tank/a/mnt";
/// Its command line file in the cache.
const CACHED: &str = "/zfsbootmenu/environments/tank/a/kcl/boot:vmlinuz-1.kcl";

/// A machine where `tank/a` has the kernel `/boot/vmlinuz-1` with its
/// initramfs and no `org.zfsbootmenu:kernel`, the image has its key, and
/// the kernel log takes the warnings.
fn machine() -> (FakeSystem, Installed) {
    let fake = FakeSystem::new();
    fake.files
        .dir("/zfsbootmenu")
        .file("/etc/zfs/key", "SECRET")
        .file(format!("{MNT}/boot/vmlinuz-1"), "")
        .file(format!("{MNT}/boot/initramfs-1.img"), "");
    fake.runner.respond(
        "zfs get -H -o value org.zfsbootmenu:kernel tank/a",
        0,
        "-\n",
    );
    let log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1)
        .without_journal()
        .install();
    (fake, log)
}

/// The warnings of the kernel log.
fn warnings(fake: &FakeSystem) -> Vec<String> {
    fake.kmsg()
        .lines()
        .filter(|line| line.starts_with("<4>"))
        .map(str::to_owned)
        .collect()
}

/// Whether a file of the state, outside the mount point, holds `text`.
fn is_in_the_state(fake: &FakeSystem, text: &str) -> bool {
    fake.files.paths().into_iter().any(|path| {
        path.starts_with(E)
            && !path.starts_with(MNT)
            && fake
                .files
                .text(&path)
                .is_some_and(|held| held.contains(text))
    })
}

#[test]
fn a_command_line_file_that_links_to_an_image_key_is_not_cached() {
    let (fake, _log) = machine();
    fake.files
        .symlink(format!("{MNT}/boot/vmlinuz-1.kcl"), "/etc/zfs/key");

    search(&fake, "tank/a").unwrap();

    assert_eq!(fake.files.text(CACHED), None);
    assert!(!is_in_the_state(&fake, "SECRET"));
    let state = State::new(&fake.files, "/zfsbootmenu");
    let line = be_cmdline(
        &fake.files,
        &NoProperties,
        &state,
        "tank/a",
        "/boot/vmlinuz-1",
        false,
        &mut Vec::new(),
    );
    assert!(!line.to_string().contains("SECRET"), "{line}");
}

#[test]
fn a_command_line_file_that_links_to_the_environment_s_own_file_caches_that_file() {
    let (fake, _log) = machine();
    fake.files
        .file(format!("{MNT}/etc/zfs/key"), "own\n")
        .symlink(format!("{MNT}/boot/vmlinuz-1.kcl"), "/etc/zfs/key");

    search(&fake, "tank/a").unwrap();

    assert_eq!(fake.files.text(CACHED).as_deref(), Some("own\n"));
    assert!(!is_in_the_state(&fake, "SECRET"));
}

#[test]
fn a_fifo_command_line_file_is_not_read() {
    let (fake, _log) = machine();
    fake.files.fifo(format!("{MNT}/boot/vmlinuz-1.kcl"));

    search(&fake, "tank/a").unwrap();

    assert_eq!(fake.files.text(CACHED), None);
    let warnings = warnings(&fake);
    assert!(
        warnings.iter().any(|line| line.contains(
            "kernel /boot/vmlinuz-1 of tank/a: 1 of its files not read, \
             the first: /boot/vmlinuz-1.kcl"
        )),
        "{warnings:#?}"
    );
}

#[test]
fn a_command_line_file_over_the_limit_is_not_cached() {
    let (fake, _log) = machine();
    let kcl = format!("{MNT}/boot/vmlinuz-1.kcl");
    fake.files.file(&kcl, vec![b'x'; (64 << 10) + 1]);
    search(&fake, "tank/a").unwrap();
    assert_eq!(fake.files.text(CACHED), None);

    // At the limit, it is.
    fake.files.file(&kcl, vec![b'x'; 64 << 10]);
    search(&fake, "tank/a").unwrap();
    assert_eq!(
        fake.files.contents(CACHED).map(|held| held.len()),
        Some(64 << 10)
    );
}

#[test]
fn a_kernel_name_with_a_newline_adds_no_line_to_the_kernel_list() {
    let (fake, _log) = machine();
    fake.files
        .file(format!("{MNT}/boot/vmlinuz-9\nq\tx\ty"), "")
        .file(format!("{MNT}/boot/initramfs-9\nq\tx\ty.img"), "");

    search(&fake, "tank/a").unwrap();

    let line = "tank/a\t/boot/vmlinuz-1\t/boot/initramfs-1.img\n";
    let list = fake.files.text(format!("{E}/tank/a/kernels"));
    assert_eq!(list.as_deref(), Some(line));
    let state = State::new(&fake.files, "/zfsbootmenu");
    let context = Context::new(fake.system());
    let selected = select_kernel(&context, &state, &name("tank/a")).unwrap();
    assert_eq!(selected.line(), line);
}

#[test]
fn the_search_warns_once_for_each_refused_kernel_and_names_it_escaped() {
    let (fake, _log) = machine();
    // A command line file is also named like a kernel, which it may be
    // when it is a regular file: one larger than a command line is read.
    fake.files
        .sized(format!("{MNT}/boot/vmlinuz-1.kcl"), 1 << 20)
        .file(format!("{MNT}/boot/vmlinuz-2"), "")
        .symlink(format!("{MNT}/boot/initramfs-2.img"), "/dev/\x1b]0;x\x07")
        .symlink(format!("{MNT}/boot/vmlinuz-3"), "x\x1b[2J")
        .dir(format!("{MNT}/boot/x\x1b[2J"));

    search(&fake, "tank/a").unwrap();
    search(&fake, "tank/a").unwrap();

    let warnings = warnings(&fake);
    let about = |kernel: &str| {
        warnings
            .iter()
            .filter(|line| line.contains(&format!("kernel /boot/{kernel} of tank/a:")))
            .count()
    };
    // Once for each search; a directory named like a kernel, not at all.
    assert_eq!(
        (about("vmlinuz-1"), about("vmlinuz-2")),
        (2, 2),
        "{warnings:#?}"
    );
    assert_eq!(about("vmlinuz-3"), 0, "{warnings:#?}");
    assert_eq!(warnings.len(), 4, "{warnings:#?}");
    let kmsg = fake.kmsg();
    assert!(!kmsg.contains(['\x1b', '\x07']), "{kmsg}");
    assert!(kmsg.contains("/boot/x\\x1b[2J is a directory"), "{kmsg}");
}
