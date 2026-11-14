//! Tests of the machine's needs, on a fake machine.

use zbm_testkit::fakes::FakeSystem;
use zbm_tools::mount::table::MOUNTINFO;

use super::*;

/// Runs `test` with the needs of `fake`, asking when `asks`.
fn with_needs<T>(
    fake: &FakeSystem,
    asks: bool,
    test: impl FnOnce(&mut OnMachine<'_, '_>) -> T,
) -> T {
    fake.files.dir("/zfsbootmenu");
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let settings = BootSettings::default();
    let import = ImportSettings::default();
    test(&mut OnMachine {
        system: &system,
        state: &state,
        settings: &settings,
        import: &import,
        asks,
    })
}

fn pool(name: &str) -> PoolName {
    name.parse().unwrap()
}

fn dataset(name: &str) -> DatasetName {
    name.parse().unwrap()
}

/// `tank` with the encryption root `tank/ROOT`, whose key is `status`.
fn encrypted(status: &str) -> FakeSystem {
    let fake = FakeSystem::new();
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "active\n");
    fake.runner.respond(
        "zfs get -H -o value encryptionroot tank/ROOT/void",
        0,
        "tank/ROOT\n",
    );
    fake.runner.respond(
        "zfs get -H -o value keystatus tank/ROOT",
        0,
        &format!("{status}\n"),
    );
    fake
}

#[test]
fn without_the_console_nothing_is_imported_or_asked_for() {
    let fake = FakeSystem::new();
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "on\n");
    fake.runner
        .respond("zpool get -H -o value readonly zroot", 0, "off\n");
    with_needs(&fake, false, |needs| {
        assert_eq!(
            needs.writable(&pool("tank")),
            Err(vec!["tank is imported read-only".to_owned()])
        );
        assert_eq!(needs.writable(&pool("zroot")), Ok(()));
    });
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.contains("export"))
    );

    let locked = encrypted("unavailable");
    with_needs(&locked, false, |needs| {
        assert_eq!(
            needs.load_key(&dataset("tank/ROOT/void")),
            Err(vec!["the key of tank/ROOT is not loaded".to_owned()])
        );
    });
    let open = encrypted("available");
    with_needs(&open, false, |needs| {
        assert_eq!(needs.load_key(&dataset("tank/ROOT/void")), Ok(()));
    });
}

#[test]
fn with_the_console_the_pool_is_imported_read_write() {
    let fake = FakeSystem::new();
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "on\n");
    fake.runner.respond(
        "zpool get -H -o value org.zfsbootmenu:readonly tank",
        0,
        "-\n",
    );
    fake.runner.respond_program("blkid", 2, "");
    with_needs(&fake, true, |needs| {
        assert_eq!(needs.writable(&pool("tank")), Ok(()));
    });
    assert!(
        fake.runner
            .lines()
            .contains(&"zpool import -N -o readonly=off tank".to_owned())
    );

    let refused = FakeSystem::new();
    refused
        .runner
        .respond("zpool get -H -o value readonly tank", 0, "on\n");
    refused.runner.respond_program("zpool", 1, "");
    with_needs(&refused, true, |needs| {
        let why = needs.writable(&pool("tank")).unwrap_err();
        assert_eq!(why, ["unable to read the properties of tank"]);
    });
}

#[test]
fn keys_mounts_and_kernels_on_the_console() {
    let open = encrypted("available");
    with_needs(&open, true, |needs| {
        assert_eq!(needs.load_key(&dataset("tank/ROOT/void")), Ok(()));
    });
    let locked = encrypted("unavailable");
    locked.runner.respond(
        "zfs get -H -o value keylocation tank/ROOT",
        0,
        "file:///k\n",
    );
    locked
        .runner
        .respond("zfs get -H -o value keyformat tank/ROOT", 0, "raw\n");
    with_needs(&locked, true, |needs| {
        let why = needs.load_key(&dataset("tank/ROOT/void")).unwrap_err();
        assert_eq!(why, ["unable to load the key of tank/ROOT"]);
    });

    let fake = FakeSystem::new();
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    with_needs(&fake, true, |needs| {
        let snapshot: SnapshotName = "tank/ROOT/void@first".parse().unwrap();
        assert!(!needs.has_kernels(&snapshot));
        let mounted = needs.mount(&dataset("tank/ROOT/void")).unwrap();
        assert!(mounted.ends_with("tank/ROOT/void/mnt"), "{mounted}");
        needs.unmount(&mounted);
    });
    assert!(
        fake.runner
            .lines()
            .iter()
            .any(|line| line.starts_with("umount -R"))
    );

    let unmountable = FakeSystem::new();
    unmountable.files.file(
        MOUNTINFO,
        "40 25 0:50 / /nowhere ro - zfs tank/ROOT/void ro\n",
    );
    unmountable.runner.respond_program("mount", 32, "");
    unmountable.runner.respond_program("umount", 1, "");
    with_needs(&unmountable, true, |needs| {
        assert_eq!(
            needs.mount(&dataset("tank/ROOT/void")),
            Err(vec!["tank/ROOT/void could not be mounted".to_owned()])
        );
        needs.unmount("/nowhere");
    });
}

/// A link that root planted on a directory of the state, which every
/// mount of a filesystem below it would follow (SB19).
fn planted() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files
        .dir("/etc")
        .symlink("/zfsbootmenu/environments/tank", "/etc");
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    fake
}

/// Whether anything ran on `fake` that mounts.
fn has_mounted(fake: &FakeSystem) -> bool {
    fake.runner
        .lines()
        .iter()
        .any(|line| line.starts_with("mount"))
}

#[test]
fn a_change_whose_state_path_runs_through_a_link_mounts_nothing() {
    let fake = planted();
    with_needs(&fake, true, |needs| {
        assert_eq!(
            needs.mount(&dataset("tank/ROOT/void")),
            Err(vec!["tank/ROOT/void could not be mounted".to_owned()])
        );
        assert_eq!(
            needs.mount_writable(&dataset("tank/ROOT/void")),
            Err(vec![
                "tank/ROOT/void could not be mounted read/write".to_owned()
            ])
        );
    });
    assert!(!has_mounted(&fake), "{:#?}", fake.runner.lines());
}

#[test]
fn a_snapshot_whose_state_path_runs_through_a_link_has_no_kernels() {
    let fake = planted();
    fake.files
        .file("/etc/ROOT/void@first/mnt/boot/vmlinuz-1", "")
        .file("/etc/ROOT/void@first/mnt/boot/initramfs-1.img", "");
    with_needs(&fake, true, |needs| {
        let snapshot: SnapshotName = "tank/ROOT/void@first".parse().unwrap();
        assert!(!needs.has_kernels(&snapshot));
    });
    assert!(!has_mounted(&fake), "{:#?}", fake.runner.lines());
    assert!(fake.files.text("/etc/ROOT/void@first/kernels").is_none());
}

/// A new default is recorded in the state directory, and the key hooks of
/// later changes get it as `BOOTFS`; a state that cannot be written is
/// logged, and the change goes on.
#[test]
fn a_new_default_is_recorded_for_the_hooks() {
    let fake = encrypted("unavailable");
    fake.files
        .file_with_mode("/libexec/hooks/load-key.d/10-key", "#!/bin/sh\n", 0o755);
    fake.runner.respond(
        "zfs get -H -o value keylocation tank/ROOT",
        0,
        "file:///k\n",
    );
    fake.runner
        .respond("zfs get -H -o value keyformat tank/ROOT", 0, "raw\n");
    with_needs(&fake, true, |needs| {
        needs.record_default(&dataset("tank/ROOT/void"));
        let _ = needs.load_key(&dataset("tank/ROOT/void"));
    });
    assert_eq!(
        fake.files.text("/zfsbootmenu/bootfs").as_deref(),
        Some("tank/ROOT/void\n")
    );
    let hook = fake
        .runner
        .calls()
        .into_iter()
        .find(|call| call.program() == "/libexec/hooks/load-key.d/10-key")
        .expect("the key hook ran");
    assert!(
        hook.get_env()
            .contains(&("BOOTFS".into(), Some("tank/ROOT/void".into()))),
        "{:?}",
        hook.get_env()
    );

    let unwritable = FakeSystem::new();
    let system = unwritable.system();
    let state = State::new(&unwritable.files, "/missing/zfsbootmenu");
    let settings = BootSettings::default();
    let import = ImportSettings::default();
    let mut needs = OnMachine {
        system: &system,
        state: &state,
        settings: &settings,
        import: &import,
        asks: true,
    };
    needs.record_default(&dataset("tank/ROOT/void"));
    assert_eq!(unwritable.files.text("/missing/zfsbootmenu/bootfs"), None);
}

#[test]
fn planned_needs_are_steps_and_what_they_read_comes_from_the_machine() {
    let fake = encrypted("unavailable");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "on\n");
    let steps = std::sync::Mutex::new(Vec::new());
    with_needs(&fake, false, |machine| {
        let machine = OnMachine {
            system: machine.system,
            state: machine.state,
            settings: machine.settings,
            import: machine.import,
            asks: false,
        };
        let mut planning = Planning {
            machine,
            steps: &steps,
        };
        assert_eq!(planning.writable(&pool("tank")), Ok(()));
        assert_eq!(planning.load_key(&dataset("tank/ROOT/void")), Ok(()));
        assert!(!planning.has_kernels(&"tank/ROOT/void@old".parse().unwrap()));
        assert!(planning.mount(&dataset("tank/ROOT/void")).is_err());
        planning.unmount("/nowhere");
    });
    let kinds: Vec<StepKind> = steps
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|step| step.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            StepKind::ImportReadWrite(pool("tank")),
            StepKind::LoadKey(dataset("tank/ROOT")),
        ]
    );
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("zfs load-key"))
    );
}

#[test]
fn a_restore_mounts_read_write_and_its_plan_copies_nothing() {
    let fake = FakeSystem::new();
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    let mnt = "/zfsbootmenu/environments/tank/ROOT/void/mnt";
    fake.files
        .file(format!("{mnt}/.zfs/snapshot/old/etc/fstab"), "fstab")
        .symlink(format!("{mnt}/.zfs/snapshot/old/lib"), "/usr/lib");
    let snapshot: SnapshotName = "tank/ROOT/void@old".parse().unwrap();
    let fstab = ["/etc/fstab".to_owned()];
    let steps = std::sync::Mutex::new(Vec::new());
    with_needs(&fake, true, |needs| {
        let mounted = needs.mount_writable(&dataset("tank/ROOT/void")).unwrap();
        assert_eq!(mounted, mnt);
        assert_eq!(needs.check_restore(&mounted, &snapshot, &fstab), Ok(()));
        let why = needs
            .check_restore(&mounted, &snapshot, &["/lib/libc.so".to_owned()])
            .unwrap_err();
        assert_eq!(why, ["/lib is a symbolic link in tank/ROOT/void@old"]);
        assert!(needs.now() > 0);
        assert_eq!(
            needs.kernel_file(&dataset("tank/ROOT/void"), "6.1"),
            Err(vec!["tank/ROOT/void has no kernel 6.1".to_owned()])
        );

        let machine = OnMachine {
            system: needs.system,
            state: needs.state,
            settings: needs.settings,
            import: needs.import,
            asks: false,
        };
        let mut planning = Planning {
            machine,
            steps: &steps,
        };
        let mounted = planning.mount_writable(&dataset("tank/ROOT/void")).unwrap();
        assert_eq!(planning.check_restore(&mounted, &snapshot, &fstab), Ok(()));
        assert_eq!(planning.restore(&mounted, &snapshot, &fstab), Ok(1));
        assert!(
            planning
                .kernel_file(&dataset("tank/ROOT/void"), "6.1")
                .is_err()
        );
        assert_eq!(planning.now(), needs.now());
    });
    let lines = fake.runner.lines();
    // Read/write once, for the restore; the kernels and the plan, read-only.
    let mounts: Vec<&String> = lines
        .iter()
        .filter(|line| line.starts_with("mount "))
        .collect();
    assert_eq!(
        mounts[0],
        &format!("mount -o zfsutil,rw -t zfs tank/ROOT/void {mnt}")
    );
    assert!(mounts[1..].iter().all(|line| line.contains("zfsutil,ro")));
    assert!(!lines.iter().any(|line| line.starts_with("cp")));
    let kinds: Vec<StepKind> = steps
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|step| step.kind)
        .collect();
    assert_eq!(
        kinds,
        [StepKind::Restore {
            snapshot,
            paths: vec![Untrusted::new("/etc/fstab".to_owned())],
        }]
    );
}

/// On the machine, the paths are copied with `cp -a` into the mounted
/// environment, and a refusal stops the copy.
#[test]
fn a_restore_on_the_machine_copies_with_cp() {
    let fake = FakeSystem::new();
    let mnt = "/zfsbootmenu/environments/tank/ROOT/void/mnt";
    fake.files
        .file(format!("{mnt}/.zfs/snapshot/old/etc/fstab"), "old fstab")
        .file(format!("{mnt}/etc/fstab"), "new fstab")
        .symlink(format!("{mnt}/.zfs/snapshot/old/lib"), "/usr/lib");
    let files = fake.files.clone();
    fake.runner.first(move |command| {
        let [_, _, from, to] = command.get_args() else {
            return None;
        };
        if command.program() != "cp" {
            return None;
        }
        files.file(to, files.contents(from)?);
        Some(Ok(zbm_platform::exec::Output::new(
            zbm_platform::exec::Status::exited(0),
            "",
        )))
    });
    let snapshot: SnapshotName = "tank/ROOT/void@old".parse().unwrap();
    with_needs(&fake, true, |needs| {
        assert_eq!(
            needs.restore(mnt, &snapshot, &["/etc/fstab".to_owned()]),
            Ok(1)
        );
        let why = needs
            .restore(mnt, &snapshot, &["/lib/libc.so".to_owned()])
            .unwrap_err();
        assert_eq!(
            why,
            [
                "/lib is a symbolic link in tank/ROOT/void@old",
                "0 restored before it"
            ]
        );
    });
    assert_eq!(
        fake.files.text(format!("{mnt}/etc/fstab")).as_deref(),
        Some("old fstab")
    );
    assert_eq!(
        fake.runner.lines(),
        [format!(
            "cp -a -- {mnt}/.zfs/snapshot/old/etc/fstab {mnt}/etc/.fstab.zbm-restore"
        )]
    );
}
