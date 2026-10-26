//! Tests of the machine's needs, on a fake machine.

use zbm_testkit::fakes::FakeSystem;

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
    test(&mut OnMachine {
        system: &system,
        state: &state,
        settings: &settings,
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
    unmountable
        .files
        .file("/proc/self/mounts", "tank/ROOT/void /nowhere zfs ro 0 0\n");
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
