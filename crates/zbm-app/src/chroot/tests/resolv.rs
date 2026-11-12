//! This system's name servers in the chroot (SB15).

use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FileOperation;

use super::*;

/// The name servers of this system.
const SERVERS: &str = "nameserver 10.0.2.3\n";

/// systemd-resolved's file, where the environment's link leads.
const STUB: &str = "/run/systemd/resolve/stub-resolv.conf";

/// A writable machine whose image has name servers, its environment with
/// `/etc` and an empty `/run`, as a dataset's are.
fn named() -> FakeSystem {
    let fake = machine("off");
    fake.files
        .file("/etc/resolv.conf", SERVERS)
        .dir(format!("{MP}/etc"))
        .dir(format!("{MP}/run"));
    fake
}

/// The bind of the image's name servers on `target`, read-only.
fn bound_on(target: &str) -> [String; 2] {
    [
        format!("mount -B /etc/resolv.conf {target}"),
        format!("mount -o remount,bind,ro {target}"),
    ]
}

/// What `fake`'s console said, without colours.
fn said(fake: &FakeSystem) -> String {
    decolorize(&fake.console.output_text())
}

/// The notices of a chroot of `tank/ROOT/be`, read/write, with `between`
/// after the filesystem's.
fn notices(between: &[&str]) -> String {
    let mut text = "* tank/ROOT/be is mounted read/write\n".to_owned();
    for notice in between {
        text.push_str("* ");
        text.push_str(notice);
        text.push('\n');
    }
    text.push_str("* /tmp is mounted read/write\n\n");
    text
}

/// Whether a line of `lines` names `resolv.conf` or `/run`.
fn is_any_named(lines: &[String]) -> bool {
    lines
        .iter()
        .any(|line| line.contains("resolv") || line.contains("/run"))
}

#[test]
fn the_image_s_resolv_conf_is_bound_read_only_right_after_the_root() {
    let fake = named();
    fake.files
        .file(format!("{MP}/etc/resolv.conf"), "nameserver 127.0.0.53\n");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert_eq!(lines[0], legacy_mounts()[0]);
    assert_eq!(lines[1..3], bound_on(&format!("{MP}/etc/resolv.conf")));
    assert_eq!(lines[3..7], legacy_mounts()[1..5], "{lines:#?}");
    // The environment's file is as it was, and nothing was written.
    assert_eq!(
        fake.files.text(format!("{MP}/etc/resolv.conf")).as_deref(),
        Some("nameserver 127.0.0.53\n")
    );
    assert_eq!(fake.files.operations(), []);
}

#[test]
fn it_is_unmounted_after_the_kernel_s_file_systems_and_before_the_root() {
    let fake = named();
    fake.files
        .file(format!("{MP}/etc/resolv.conf"), "")
        .dir(format!("{MP}/dev/pts"));
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    let shell = lines.iter().position(|line| line == "/bin/sh").unwrap();
    assert_eq!(
        lines[shell + 1..],
        [
            format!("umount {MP}/dev/pts"),
            format!("umount {MP}/dev"),
            format!("umount {MP}/sys"),
            format!("umount {MP}/proc"),
            format!("umount {MP}/tmp"),
            format!("umount {MP}/etc/resolv.conf"),
            format!("umount -R {MP}"),
        ]
    );
    assert!(is_all_unmounted(&lines), "{lines:#?}");
}

#[test]
fn the_notices_say_resolv_conf_is_mounted_read_only_before_tmp() {
    let fake = named();
    fake.files.file(format!("{MP}/etc/resolv.conf"), "");
    open_on(&fake, "tank/ROOT/be").unwrap();
    assert_eq!(
        said(&fake),
        notices(&["/etc/resolv.conf is mounted read-only"])
    );
}

#[test]
fn without_resolv_conf_in_the_image_nothing_more_is_mounted() {
    let fake = machine("off");
    fake.files.file(format!("{MP}/etc/resolv.conf"), "");
    open_on(&fake, "tank/ROOT/be").unwrap();
    assert!(!is_any_named(&ran(&fake)), "{:#?}", ran(&fake));
    assert_eq!(said(&fake), notices(&[]));
}

#[test]
fn an_empty_resolv_conf_in_the_image_is_not_bound() {
    // Nor one that is no file.
    for image in ["", "dir"] {
        let fake = machine("off");
        fake.files.file(format!("{MP}/etc/resolv.conf"), "");
        if image.is_empty() {
            fake.files.file("/etc/resolv.conf", "");
        } else {
            fake.files.dir("/etc/resolv.conf");
        }
        open_on(&fake, "tank/ROOT/be").unwrap();
        assert!(!is_any_named(&ran(&fake)), "{:#?}", ran(&fake));
        assert_eq!(said(&fake), notices(&[]));
    }
}

#[test]
fn an_environment_without_resolv_conf_gets_none_and_nothing_is_created() {
    let fake = named();
    open_on(&fake, "tank/ROOT/be").unwrap();
    assert!(!is_any_named(&ran(&fake)), "{:#?}", ran(&fake));
    assert_eq!(fake.files.operations(), []);
    assert_eq!(
        said(&fake),
        notices(&[
            "/etc/resolv.conf is not mounted: /etc/resolv.conf is missing in the environment"
        ])
    );
}

#[test]
fn an_absolute_link_is_followed_inside_the_root_never_the_image_s() {
    let fake = named();
    // The image has the link's target too, which no mount may take.
    fake.files
        .file(STUB, "nameserver 192.0.2.1\n")
        .file(format!("{MP}{STUB}"), "")
        .symlink(format!("{MP}/etc/resolv.conf"), STUB);
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert_eq!(lines[1..3], bound_on(&format!("{MP}{STUB}")));
    assert!(lines.contains(&format!("umount {MP}{STUB}")));
    assert_eq!(
        said(&fake),
        notices(&[&format!("/etc/resolv.conf -> {STUB} is mounted read-only")])
    );
    assert_eq!(fake.files.operations(), []);
}

#[test]
fn a_relative_link_is_followed_from_etc_inside_the_root() {
    let fake = named();
    fake.files
        .file(STUB, "")
        .file(format!("{MP}{STUB}"), "")
        .symlink(format!("{MP}/etc/resolv.conf"), format!("..{STUB}"));
    open_on(&fake, "tank/ROOT/be").unwrap();
    assert_eq!(ran(&fake)[1..3], bound_on(&format!("{MP}{STUB}")));
    assert!(said(&fake).contains(&format!(
        "* /etc/resolv.conf -> {STUB} is mounted read-only\n"
    )));
}

#[test]
fn a_link_that_climbs_out_of_the_root_stays_inside_it() {
    let fake = named();
    fake.files
        .file("/etc/real", "")
        .file(format!("{MP}/etc/real"), "")
        .symlink(
            format!("{MP}/etc/resolv.conf"),
            "../../../../../../../../../etc/real",
        );
    open_on(&fake, "tank/ROOT/be").unwrap();
    assert_eq!(ran(&fake)[1..3], bound_on(&format!("{MP}/etc/real")));
    assert!(said(&fake).contains("* /etc/resolv.conf -> /etc/real is mounted read-only\n"));
}

#[test]
fn a_link_into_tmp_dev_proc_or_sys_mounts_nothing() {
    for (target, top) in [
        ("/tmp/resolv.conf", "/tmp"),
        ("/dev/shm/resolv.conf", "/dev"),
        ("/proc/self/net/resolv.conf", "/proc"),
        ("/sys/resolv.conf", "/sys"),
    ] {
        let fake = named();
        fake.files
            .file(format!("{MP}{target}"), "")
            .symlink(format!("{MP}/etc/resolv.conf"), target);
        open_on(&fake, "tank/ROOT/be").unwrap();
        let lines = ran(&fake);
        assert!(
            !lines.iter().any(|line| line.contains("resolv")),
            "{lines:#?}"
        );
        let reason = format!("/etc/resolv.conf is not mounted: it leads into this system's {top}");
        assert_eq!(said(&fake), notices(&[&reason]));
    }
}

/// Lays out the environment of a machine.
type Layout = fn(&FakeSystem);

#[test]
fn a_link_to_a_directory_mounts_nothing() {
    // Nor a loop, a file on the way, or a link that cannot be read.
    let cases: [(&str, Layout); 4] = [
        ("/etc/dir is not a file", |fake| {
            fake.files
                .dir(format!("{MP}/etc/dir"))
                .symlink(format!("{MP}/etc/resolv.conf"), "dir");
        }),
        ("too many symbolic links", |fake| {
            fake.files
                .symlink(format!("{MP}/etc/resolv.conf"), "loop")
                .symlink(format!("{MP}/etc/loop"), "resolv.conf");
        }),
        ("/etc/file is not a directory", |fake| {
            fake.files
                .file(format!("{MP}/etc/file"), "")
                .symlink(format!("{MP}/etc/resolv.conf"), "file/resolv.conf");
        }),
        ("Permission denied (os error 13)", |fake| {
            fake.files.fail(
                format!("{MP}/etc/resolv.conf"),
                io::ErrorKind::PermissionDenied,
            );
        }),
    ];
    for (why, layout) in cases {
        let fake = named();
        layout(&fake);
        open_on(&fake, "tank/ROOT/be").unwrap();
        assert!(!is_any_named(&ran(&fake)), "{why}: {:#?}", ran(&fake));
        let reason = format!("/etc/resolv.conf is not mounted: {why}");
        assert_eq!(said(&fake), notices(&[&reason]));
    }
}

#[test]
fn a_snapshot_a_read_only_and_a_read_write_chroot_all_bind_it_read_only() {
    for (readonly, fs, at) in [
        ("off", "tank/ROOT/be@old", SNAPSHOT_MP),
        ("on", "tank/ROOT/be", MP),
        ("off", "tank/ROOT/be", MP),
    ] {
        let fake = machine(readonly);
        fake.files
            .file("/etc/resolv.conf", SERVERS)
            .file(format!("{at}/etc/resolv.conf"), "");
        open_on(&fake, fs).unwrap();
        let lines = ran(&fake);
        assert_eq!(
            lines[1..3],
            bound_on(&format!("{at}/etc/resolv.conf")),
            "{fs}"
        );
        assert!(said(&fake).contains("* /etc/resolv.conf is mounted read-only\n"));
    }
}

#[test]
fn a_failed_remount_leaves_no_bind_and_no_notice() {
    let fake = named();
    let target = format!("{MP}/etc/resolv.conf");
    fake.files.file(&target, "");
    fake.runner
        .respond(&format!("mount -o remount,bind,ro {target}"), 32, "");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert_eq!(lines[3], format!("umount {target}"), "{lines:#?}");
    assert_eq!(
        lines.iter().filter(|line| line.contains("resolv")).count(),
        3
    );
    assert!(is_all_unmounted(&lines), "{lines:#?}");
    assert_eq!(said(&fake), notices(&[]));
}

#[test]
fn a_bind_left_writable_is_said_read_write_and_unmounted_with_the_rest() {
    let fake = named();
    let target = format!("{MP}/etc/resolv.conf");
    fake.files.file(&target, "");
    fake.runner
        .respond(&format!("mount -o remount,bind,ro {target}"), 32, "");
    fake.runner.respond(&format!("umount {target}"), 1, "");
    open_on(&fake, "tank/ROOT/be").unwrap();
    assert_eq!(
        said(&fake),
        notices(&["/etc/resolv.conf is mounted read/write"])
    );
    let lines = ran(&fake);
    assert_eq!(
        lines[lines.len() - 3..],
        [
            format!("umount {MP}/tmp"),
            format!("umount {target}"),
            format!("umount -R {MP}"),
        ]
    );
    // In red, as the filesystem's read/write.
    let raw = fake.console.output_text();
    assert!(raw.contains(&notice("/etc/resolv.conf", true)), "{raw:?}");
}

/// The environment's layouts the tests above take: a file, links, and a
/// dangling link into an empty `/run`.
fn layouts() -> Vec<FakeSystem> {
    let file = named();
    file.files.file(format!("{MP}/etc/resolv.conf"), "");
    let absolute = named();
    absolute
        .files
        .file(STUB, "")
        .file(format!("{MP}{STUB}"), "")
        .symlink(format!("{MP}/etc/resolv.conf"), STUB);
    let relative = named();
    relative
        .files
        .file(format!("{MP}/run/x"), "")
        .symlink(format!("{MP}/etc/resolv.conf"), "../run/x");
    let dangling = named();
    dangling
        .files
        .symlink(format!("{MP}/etc/resolv.conf"), STUB);
    vec![file, absolute, relative, dangling]
}

#[test]
fn the_image_s_own_resolv_conf_is_never_a_mount_target() {
    for fake in layouts() {
        open_on(&fake, "tank/ROOT/be").unwrap();
        let lines = ran(&fake);
        let mounts: Vec<&String> = lines
            .iter()
            .filter(|line| line.starts_with("mount") || line.starts_with("umount"))
            .collect();
        assert!(mounts.len() > 8, "{lines:#?}");
        for line in mounts {
            let target = line.rsplit(' ').next().unwrap_or_default();
            assert!(target.starts_with(MP), "{line}");
            assert_ne!(target, "/etc/resolv.conf", "{line}");
            assert_ne!(target, "/run", "{line}");
        }
    }
}

#[test]
fn a_link_target_with_control_characters_is_escaped_in_the_notice() {
    let evil = "/run/\x1b[31mevil";
    let fake = named();
    fake.files
        .file(format!("{MP}{evil}"), "")
        .symlink(format!("{MP}/etc/resolv.conf"), evil);
    open_on(&fake, "tank/ROOT/be").unwrap();
    let raw = fake.console.output_text();
    assert!(!raw.contains("\x1b[31mevil"), "{raw:?}");
    assert!(
        said(&fake).contains("* /etc/resolv.conf -> /run/\\x1b[31mevil is mounted read-only\n"),
        "{raw:?}"
    );

    // And in the notice that nothing is mounted.
    let fake = named();
    fake.files
        .symlink(format!("{MP}/etc/resolv.conf"), "/var/\x1b]0;title\x07");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let raw = fake.console.output_text();
    assert!(!raw.contains("\x1b]0;title"), "{raw:?}");
    assert!(
        raw.contains("/var/\\x1b]0;title\\x07 is missing"),
        "{raw:?}"
    );
}

#[test]
fn a_dangling_link_into_an_empty_run_gets_run_in_memory_and_nothing_on_the_environment() {
    let fake = named();
    fake.files.symlink(format!("{MP}/etc/resolv.conf"), STUB);
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    let run = format!("{MP}/run");
    assert_eq!(
        lines[1],
        format!("mount -t tmpfs -o mode=0755,nosuid,nodev tmpfs {run}")
    );
    assert_eq!(lines[2..4], bound_on(&format!("{MP}{STUB}")));
    assert_eq!(lines[4], format!("mount -B /tmp {MP}/tmp"));
    let shell = lines.iter().position(|line| line == "/bin/sh").unwrap();
    assert_eq!(
        lines[shell + 1..],
        [
            format!("umount {MP}/dev"),
            format!("umount {MP}/sys"),
            format!("umount {MP}/proc"),
            format!("umount {MP}/tmp"),
            format!("umount {MP}{STUB}"),
            format!("umount {run}"),
            format!("umount -R {MP}"),
        ]
    );
    // What is made is made in the memory on /run: the link's directories
    // and an empty file to mount on.
    assert_eq!(
        fake.files.operations(),
        [
            FileOperation::CreateDir(format!("{run}/systemd").into()),
            FileOperation::CreateDir(format!("{run}/systemd/resolve").into()),
            FileOperation::Write(format!("{MP}{STUB}").into(), Vec::new()),
        ]
    );
    assert_eq!(
        said(&fake),
        notices(&[
            "/run (in memory) is mounted read/write",
            &format!("/etc/resolv.conf -> {STUB} is mounted read-only"),
        ])
    );
}

#[test]
fn a_run_that_is_not_empty_is_never_hidden() {
    // Nor a /run that is missing, or a link.
    let full = named();
    full.files.file(format!("{MP}/run/lock"), "");
    let missing = machine("off");
    missing
        .files
        .file("/etc/resolv.conf", SERVERS)
        .dir(format!("{MP}/etc"));
    let linked = machine("off");
    linked
        .files
        .file("/etc/resolv.conf", SERVERS)
        .dir(format!("{MP}/var/run"))
        .symlink(format!("{MP}/run"), "/var/run");
    for fake in [full, missing, linked] {
        fake.files.symlink(format!("{MP}/etc/resolv.conf"), STUB);
        open_on(&fake, "tank/ROOT/be").unwrap();
        let lines = ran(&fake);
        assert!(!is_any_named(&lines), "{lines:#?}");
        assert_eq!(fake.files.operations(), []);
        assert!(said(&fake).contains("* /etc/resolv.conf is not mounted: "));
    }
}

#[test]
fn a_failed_tmpfs_creates_nothing_and_binds_nothing() {
    let fake = named();
    fake.files.symlink(format!("{MP}/etc/resolv.conf"), STUB);
    fake.runner.respond(
        &format!("mount -t tmpfs -o mode=0755,nosuid,nodev tmpfs {MP}/run"),
        32,
        "",
    );
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    let named: Vec<&String> = lines
        .iter()
        .filter(|line| line.contains("resolv") || line.contains("/run"))
        .collect();
    assert_eq!(
        named,
        [&format!(
            "mount -t tmpfs -o mode=0755,nosuid,nodev tmpfs {MP}/run"
        )]
    );
    assert_eq!(fake.files.operations(), []);
    assert_eq!(said(&fake), notices(&[]));
}

#[test]
fn a_run_in_memory_with_nothing_bound_on_it_is_unmounted_at_once() {
    // The file to mount on cannot be made, or the bind fails.
    let unmade = named();
    unmade
        .files
        .fail(format!("{MP}{STUB}"), io::ErrorKind::StorageFull);
    let unbound = named();
    unbound
        .runner
        .respond(&format!("mount -B /etc/resolv.conf {MP}{STUB}"), 32, "");
    for fake in [unmade, unbound] {
        fake.files.symlink(format!("{MP}/etc/resolv.conf"), STUB);
        open_on(&fake, "tank/ROOT/be").unwrap();
        let lines = ran(&fake);
        let unmounted = format!("umount {MP}/run");
        let at = lines.iter().position(|line| *line == unmounted);
        // Before /tmp is mounted, and once.
        assert!(at.is_some_and(|at| at < 5), "{lines:#?}");
        assert_eq!(lines.iter().filter(|line| **line == unmounted).count(), 1);
        assert!(!lines.contains(&format!("umount {MP}{STUB}")), "{lines:#?}");
        assert_eq!(said(&fake), notices(&[]));
    }
}

#[test]
fn a_dangling_link_outside_run_mounts_nothing() {
    let fake = named();
    fake.files.symlink(
        format!("{MP}/etc/resolv.conf"),
        "/var/lib/resolv/resolv.conf",
    );
    open_on(&fake, "tank/ROOT/be").unwrap();
    assert!(!is_any_named(&ran(&fake)), "{:#?}", ran(&fake));
    assert_eq!(fake.files.operations(), []);
    assert_eq!(
        said(&fake),
        notices(&["/etc/resolv.conf is not mounted: \
                   /var/lib/resolv/resolv.conf is missing in the environment"])
    );
}
