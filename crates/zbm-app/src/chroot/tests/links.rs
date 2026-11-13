//! The kernel's mounts on no link, and no unmount through one (SB16).

use std::sync::Mutex;

use pretty_assertions::assert_eq;
use proptest::prelude::*;
use zbm_tools::mount::inside::{self, Unresolved};

use super::resolv::{STUB, named};
use super::*;

/// What `fake`'s console said, without colours.
fn said(fake: &FakeSystem) -> String {
    decolorize(&fake.console.output_text())
}

/// The notices of a chroot of `tank/ROOT/be`, read/write, with `between`
/// after the filesystem's, and `/tmp`'s last where `tmp` holds.
fn notices(between: &[&str], tmp: bool) -> String {
    let mut text = "* tank/ROOT/be is mounted read/write\n".to_owned();
    for notice in between {
        text.push_str("* ");
        text.push_str(notice);
        text.push('\n');
    }
    if tmp {
        text.push_str("* /tmp is mounted read/write\n");
    }
    text.push('\n');
    text
}

/// Whether a mount or an unmount of `lines` is on `target`.
fn is_target(lines: &[String], target: &str) -> bool {
    lines.iter().any(|line| {
        (line.starts_with("mount") || line.starts_with("umount"))
            && line.split(' ').any(|word| word == target)
    })
}

/// Makes the interactive shell of `fake`'s chroot leave `path` a link to
/// `target` as it runs.
fn shell_leaves_link(fake: &FakeSystem, path: String, target: &'static str) {
    let files = fake.files.clone();
    fake.runner.first(move |command| {
        if command.display() == "/bin/sh" {
            files.symlink(&path, target);
        }
        None
    });
}

/// The host's `efivarfs` set to `options`.
fn host_efivarfs(options: &str) -> String {
    format!("mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o {options}")
}

#[test]
fn a_tmp_that_links_to_etc_is_not_bound_and_the_notice_says_why() {
    let fake = machine("off");
    fake.files.symlink(format!("{MP}/tmp"), "/etc");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert!(!is_target(&lines, &format!("{MP}/tmp")), "{lines:#?}");
    assert!(
        !lines.iter().any(|line| line.contains("/tmp")),
        "{lines:#?}"
    );
    assert_eq!(lines[1..4], legacy_mounts()[2..5], "{lines:#?}");
    assert_eq!(
        said(&fake),
        notices(&["/tmp is not mounted: it is a link to /etc"], false)
    );
    assert_eq!(fake.files.operations(), []);
    assert!(is_all_unmounted(&lines), "{lines:#?}");
}

#[test]
fn each_kernel_mount_point_that_is_a_link_is_refused_alone() {
    for name in ["tmp", "proc", "sys", "dev"] {
        for target in ["/etc", "etc", "../../../../../../etc"] {
            let fake = machine("off");
            fake.files.symlink(format!("{MP}/{name}"), target);
            open_on(&fake, "tank/ROOT/be").unwrap();
            let lines = ran(&fake);
            for other in ["tmp", "proc", "sys", "dev"] {
                let mounted = is_target(&lines, &format!("{MP}/{other}"));
                assert_eq!(mounted, other != name, "{name} -> {target}: {lines:#?}");
            }
            let refused = format!("/{name} is not mounted: it is a link to {target}");
            assert_eq!(said(&fake), notices(&[&refused], name != "tmp"));
        }
    }
}

#[test]
fn a_kernel_mount_point_that_cannot_be_read_is_refused() {
    let fake = machine("off");
    fake.files
        .fail(format!("{MP}/proc"), io::ErrorKind::PermissionDenied);
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert!(!is_target(&lines, &format!("{MP}/proc")), "{lines:#?}");
    assert!(is_target(&lines, &format!("{MP}/sys")), "{lines:#?}");
    let shown = said(&fake);
    assert!(
        shown.contains("* /proc is not mounted: Permission denied"),
        "{shown}"
    );
}

#[test]
fn a_missing_mount_point_is_tried_as_zfs_chroot_tried_it() {
    // The fixture's root has no `tmp`, `proc`, `sys` or `dev`; a file is
    // tried too. Each mount fails by itself on a real system.
    for tmp_is_file in [false, true] {
        let fake = machine("off");
        if tmp_is_file {
            fake.files.file(format!("{MP}/tmp"), "");
        }
        open_on(&fake, "tank/ROOT/be").unwrap();
        let lines = ran(&fake);
        assert_eq!(lines[..5], legacy_mounts()[..5], "{lines:#?}");
        assert_eq!(said(&fake), notices(&[], true));
    }
}

#[test]
fn efivarfs_below_a_linked_sys_is_not_mounted_and_the_host_s_is_left_alone() {
    let fake = machine("off");
    efi(&fake);
    fake.files.symlink(format!("{MP}/sys"), "/");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert!(
        !lines.iter().any(|line| line.contains("efivar")),
        "{lines:#?}"
    );
    let shown = said(&fake);
    assert!(!shown.contains("efivarfs"), "{shown}");

    // A link below a `sysfs` that mounted refuses the chroot's `efivarfs`
    // alone, before the host's is touched.
    let fake = machine("off");
    efi(&fake);
    fake.files
        .symlink(format!("{MP}/sys/firmware"), "/sys/firmware");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert!(is_target(&lines, &format!("{MP}/sys")), "{lines:#?}");
    assert!(
        !lines.iter().any(|line| line.contains("efivar")),
        "{lines:#?}"
    );
    assert_eq!(
        said(&fake),
        notices(
            &["efivarfs is not mounted: /sys/firmware is a link to /sys/firmware"],
            true
        )
    );
}

#[test]
fn efivarfs_is_not_mounted_in_a_chroot_whose_sysfs_failed() {
    let fake = machine("off");
    efi(&fake);
    fake.runner
        .respond(&format!("mount -t sysfs sys {MP}/sys"), 32, "");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert!(
        !lines.iter().any(|line| line.contains("efivar")),
        "{lines:#?}"
    );
    assert!(!lines.contains(&format!("umount {MP}/sys")), "{lines:#?}");
    assert_eq!(said(&fake), notices(&[], true));
}

#[test]
fn the_host_s_efivarfs_is_read_only_again_before_the_shell_when_the_chroot_s_failed() {
    let fake = machine("off");
    efi(&fake);
    let inside = format!("mount -t efivarfs efivarfs {MP}/sys/firmware/efi/efivars -o rw");
    fake.runner.respond(&inside, 32, "");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    let rw = host_efivarfs("remount,rw");
    let at = lines.iter().position(|line| *line == rw).unwrap();
    assert_eq!(
        lines[at..at + 4],
        [
            rw,
            inside,
            host_efivarfs("remount,ro"),
            "/bin/sh -c exit 0".to_owned()
        ],
        "{lines:#?}"
    );
    // Once, and not again at the end.
    let ro = host_efivarfs("remount,ro");
    assert_eq!(lines.iter().filter(|line| **line == ro).count(), 1);
    assert_eq!(lines.last(), Some(&format!("umount -R {MP}")));
    assert!(!said(&fake).contains("efivarfs"));
}

#[test]
fn devpts_below_a_linked_dev_is_not_mounted() {
    let fake = machine("off");
    fake.files.dir("/etc/pts");
    fake.files.symlink(format!("{MP}/dev"), "/etc");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert!(
        !lines.iter().any(|line| line.contains("devpts")),
        "{lines:#?}"
    );
    assert!(!is_target(&lines, &format!("{MP}/dev")), "{lines:#?}");
    assert_eq!(
        said(&fake),
        notices(&["/dev is not mounted: it is a link to /etc"], true)
    );
}

#[test]
fn devpts_is_not_mounted_where_the_dev_bind_failed() {
    let fake = machine("off");
    fake.files.symlink(format!("{MP}/dev/pts"), "/etc");
    fake.runner
        .respond(&format!("mount -B /dev {MP}/dev"), 32, "");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert!(
        !lines.iter().any(|line| line.contains("devpts")),
        "{lines:#?}"
    );
    assert_eq!(said(&fake), notices(&[], true));

    // On a bound `/dev`, the link is refused with a notice.
    let fake = machine("off");
    fake.files.symlink(format!("{MP}/dev/pts"), "/etc");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert!(
        !lines.iter().any(|line| line.contains("devpts")),
        "{lines:#?}"
    );
    assert_eq!(
        said(&fake),
        notices(&["/dev/pts is not mounted: it is a link to /etc"], true)
    );
}

#[test]
fn the_tmp_notice_is_said_only_when_tmp_is_mounted() {
    let fake = machine("off");
    fake.runner
        .respond(&format!("mount -B /tmp {MP}/tmp"), 32, "");
    open_on(&fake, "tank/ROOT/be").unwrap();
    assert_eq!(said(&fake), notices(&[], false));
    assert!(!ran(&fake).contains(&format!("umount {MP}/tmp")));

    let fake = machine("off");
    open_on(&fake, "tank/ROOT/be").unwrap();
    assert_eq!(said(&fake), notices(&[], true));
}

#[test]
fn a_link_target_with_control_characters_is_escaped_in_the_refusal() {
    let fake = machine("off");
    fake.files.symlink(format!("{MP}/tmp"), "/\x1b[31mevil");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let raw = fake.console.output_text();
    assert!(!raw.contains("\x1b[31mevil"), "{raw:?}");
    assert!(
        said(&fake).contains("* /tmp is not mounted: it is a link to /\\x1b[31mevil\n"),
        "{raw:?}"
    );
}

#[test]
fn a_link_the_shell_left_on_tmp_is_not_unmounted_through() {
    let fake = machine("off");
    shell_leaves_link(&fake, format!("{MP}/tmp"), "/proc");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    let shell = lines.iter().position(|line| line == "/bin/sh").unwrap();
    assert_eq!(
        lines[shell + 1..],
        [
            format!("umount {MP}/dev"),
            format!("umount {MP}/sys"),
            format!("umount {MP}/proc"),
            format!("umount -R {MP}"),
        ]
    );
}

#[test]
fn a_link_the_shell_left_on_the_way_to_resolv_conf_is_not_unmounted_through() {
    let fake = named();
    fake.files.file(format!("{MP}/etc/resolv.conf"), "");
    shell_leaves_link(&fake, format!("{MP}/etc"), "/etc");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert!(lines.contains(&format!("mount -B /etc/resolv.conf {MP}/etc/resolv.conf")));
    assert!(
        !lines
            .iter()
            .any(|line| line.starts_with("umount") && line.contains("resolv")),
        "{lines:#?}"
    );
    assert_eq!(lines.last(), Some(&format!("umount -R {MP}")));
}

#[test]
fn a_link_the_shell_left_below_run_in_memory_is_not_unmounted_through() {
    let fake = named();
    fake.files.symlink(format!("{MP}/etc/resolv.conf"), STUB);
    shell_leaves_link(&fake, format!("{MP}/run/systemd"), "/sys");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert!(lines.contains(&format!("mount -B /etc/resolv.conf {MP}{STUB}")));
    assert!(!lines.contains(&format!("umount {MP}{STUB}")), "{lines:#?}");
    let shell = lines.iter().position(|line| line == "/bin/sh").unwrap();
    assert_eq!(
        lines[lines.len() - 2..],
        [format!("umount {MP}/run"), format!("umount -R {MP}")],
        "{:#?}",
        &lines[shell..]
    );
}

#[test]
fn a_guard_outside_the_root_or_with_dot_dot_is_left_never_unmounted_by_path() {
    let fake = FakeSystem::new();
    fake.files.dir(format!("{MP}/proc"));
    let mount = Mount::new(&fake.runner, &fake.files);
    let root = Path::new(MP);
    for path in ["/etc".to_owned(), format!("{MP}/proc/../../etc")] {
        let mounted = mount.bind(Path::new("/tmp"), Path::new(&path)).unwrap();
        unmount_below(&fake.files, root, mounted);
    }
    // One below the root, with no link on its way, is unmounted by its
    // path.
    let mounted = mount.proc(&root.join("proc")).unwrap();
    unmount_below(&fake.files, root, mounted);
    let unmounted: Vec<String> = fake
        .runner
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("umount"))
        .collect();
    assert_eq!(unmounted, [format!("umount {MP}/proc")]);
}

#[test]
fn every_kernel_mount_point_is_covered_by_the_resolver() {
    let fake = FakeSystem::new();
    fake.files.dir(MP);
    for name in kernel::MOUNT_POINTS {
        fake.files.dir(format!("{MP}/{name}/x"));
        let path = format!("/{name}/x");
        let resolved = inside::resolve(&fake.files, Path::new(MP), Path::new(&path));
        assert!(
            matches!(resolved, Err(Unresolved::Covered(_))),
            "{name}: {resolved:?}"
        );
    }
}

/// The paths below the root that the random layouts set: the kernel's
/// mount points and the name servers', the deepest first, so that a link
/// set later is not undone by a directory made for one below it.
const PLACES: [&str; 6] = ["dev/pts", "etc/resolv.conf", "tmp", "proc", "sys", "dev"];

/// What a path of [`PLACES`] is in a random layout.
#[derive(Clone, Debug)]
enum Entry {
    Dir,
    File,
    Missing,
    Link(String),
}

/// A link's target: absolute or relative, with `..` and `.`, and paths
/// of the image among them.
fn target() -> impl Strategy<Value = String> {
    const NAMES: [&str; 9] = ["etc", "proc", "sys", "dev", "tmp", "run", "pts", "..", "."];
    let names = proptest::collection::vec(proptest::sample::select(NAMES.as_slice()), 0..5);
    (any::<bool>(), names).prop_map(|(absolute, names)| {
        let joined = names.join("/");
        if absolute {
            format!("/{joined}")
        } else {
            joined
        }
    })
}

fn entry() -> impl Strategy<Value = Entry> {
    prop_oneof![
        Just(Entry::Dir),
        Just(Entry::File),
        Just(Entry::Missing),
        target().prop_map(Entry::Link),
    ]
}

/// The target of a `mount` or `umount` line: its last argument, an `-o`
/// with its options after it set aside.
fn mount_target(line: &str) -> Option<&str> {
    let words: Vec<&str> = line.split(' ').collect();
    if !matches!(words.first(), Some(&("mount" | "umount"))) {
        return None;
    }
    let end = match words.len().checked_sub(2) {
        Some(at) if words[at] == "-o" => at,
        _ => words.len(),
    };
    words.get(end.checked_sub(1)?).copied()
}

/// Whether no name of `path`, from `/`, is a symbolic link in `files`.
fn is_link_free(files: &dyn Files, path: &Path) -> bool {
    path.ancestors()
        .filter(|ancestor| *ancestor != Path::new("/"))
        .all(|ancestor| files.read_link(ancestor).is_err())
}

/// A machine for the property: the image's name servers, the
/// environment's `/run` empty, `places` set as drawn, and EFI where `efi`
/// holds.
fn layout(places: &[Entry], efi_on: bool) -> FakeSystem {
    let fake = named();
    if efi_on {
        efi(&fake);
    }
    for (place, entry) in PLACES.iter().zip(places) {
        let path = format!("{MP}/{place}");
        match entry {
            Entry::Dir => {
                fake.files.dir(&path);
            }
            Entry::File => {
                fake.files.file(&path, "");
            }
            Entry::Missing => {}
            Entry::Link(target) => {
                fake.files.symlink(&path, target);
            }
        }
    }
    fake
}

proptest! {
    /// No mount and no unmount of a chroot lands on a path that passes
    /// through a link, as the tree is when its line runs; the root's
    /// recursive unmount ends it. Its limits: the fakes model no mounts,
    /// so it cannot see an overmount, propagation, or a linked prefix of
    /// the root's own path (SB16d and the VM test cover those).
    #[test]
    fn no_mount_or_unmount_target_passes_through_a_link(
        places in proptest::collection::vec(entry(), PLACES.len()),
        efi_on in any::<bool>(),
        failures in any::<u64>(),
        left in proptest::option::of((0..PLACES.len(), target())),
    ) {
        let fake = layout(&places, efi_on);
        let wrong = Arc::new(Mutex::new(Vec::new()));
        let (files, seen) = (fake.files.clone(), Arc::clone(&wrong));
        fake.runner.first(move |command| {
            let line = command.display();
            if line == "/bin/sh" && let Some((place, target)) = &left {
                files.symlink(format!("{MP}/{}", PLACES[*place]), target);
            }
            let host = line.starts_with("mount -t efivarfs efivarfs /sys/firmware/efi/efivars ");
            if let Some(target) = mount_target(&line).filter(|_| !host) {
                let target = Path::new(target);
                if !target.starts_with(MP) || !is_link_free(&files, target) {
                    seen.lock().unwrap().push(line);
                }
            }
            None
        });
        let count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        fake.runner.on(move |command| {
            let line = command.display();
            if !line.starts_with("mount ") || line.contains(" -t zfs ") {
                return None;
            }
            let at = count.fetch_add(1, Ordering::SeqCst) % 64;
            (failures >> at & 1 == 1).then(|| Ok(Output::new(Status::exited(32), "")))
        });
        open_on(&fake, "tank/ROOT/be").unwrap();
        let wrong = wrong.lock().unwrap().clone();
        prop_assert_eq!(wrong, Vec::<String>::new());
        let lines = ran(&fake);
        let last = lines.iter().rev().find(|line| line.starts_with("umount"));
        prop_assert_eq!(last, Some(&format!("umount -R {MP}")));
    }
}
