//! Tests of the scan of the pools and of the pools each snapshot lists.

use pretty_assertions::assert_eq;
use zbm_platform::Context;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::auth::{self, Authority};
use crate::model::Health;
use crate::{Application as _, Runtime, Seen, Through};

/// What `zpool import` lists: `backup`, which this host can import, and
/// `tank`, which another system used last.
const LISTING: &str = "   pool: backup
     id: 5
  state: ONLINE
 action: The pool can be imported using its name or numeric identifier.
 config:

\tbackup      ONLINE
\t  vdc       ONLINE

   pool: tank
     id: 7
  state: ONLINE
 status: The pool was last accessed by another system.
 action: The pool can be imported using its name or numeric identifier and
\tthe '-f' flag.
 config:

\ttank        ONLINE
\t  vdb       ONLINE
";

/// What `zdb -e -C 7` reads of `tank`: the host `otherhost`, whose host ID
/// is `deadbeef`.
const CONFIG: &str = "
MOS Configuration:
        version: 5000
        name: 'tank'
        pool_guid: 7
        hostid: 3735928559
        hostname: 'otherhost'
";

/// A machine with no pool imported, and the two of [`LISTING`] to import.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner.respond("zpool import", 0, LISTING);
    fake.runner.respond("zdb -e -C 7", 0, CONFIG);
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    fake
}

/// How many times `fake` ran `line`.
fn ran(fake: &FakeSystem, line: &str) -> usize {
    fake.runner
        .lines()
        .iter()
        .filter(|ran| ran.as_str() == line)
        .count()
}

/// The pools of `events`, which must be one [`Event::PoolsScanned`].
fn scanned(events: Vec<Event>) -> Vec<Pool> {
    match <[Event; 1]>::try_from(events) {
        Ok([Event::PoolsScanned(pools)]) => pools,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_snapshot_scans_nothing_and_lists_what_the_last_scan_found() {
    let fake = machine();
    let runtime = Runtime::default();
    let seen = Seen::new();
    let app = App::new(Context::new(fake.system()), &runtime).with_seen(&seen);

    assert!(app.snapshot().unwrap().pools.is_empty());
    assert_eq!(ran(&fake, "zpool import"), 0);

    let pools = scanned(app.apply(&Action::ScanPools, Through::Quietly(fake.system())));

    let found: Vec<(&str, Option<u64>, bool)> = pools
        .iter()
        .map(|pool| (pool.name.as_str(), pool.id, pool.imported))
        .collect();
    assert_eq!(
        found,
        [("backup", Some(5), false), ("tank", Some(7), false)]
    );
    assert_eq!(pools[0].host, None);
    let host = pools[1].host.as_ref().unwrap();
    assert_eq!(
        host.hostid.as_ref().map(zbm_types::HostId::as_str),
        Some("deadbeef")
    );
    assert_eq!(
        host.hostname.as_ref().map(|name| name.get().as_str()),
        Some("otherhost")
    );
    assert_eq!(pools[1].health, Health::Online);
    // `zdb` reads the host of the foreign pool alone.
    let zdb: Vec<String> = fake
        .runner
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("zdb"))
        .collect();
    assert_eq!(zdb, ["zdb -e -C 7"]);

    // The snapshots list them from now on, scanning nothing.
    assert_eq!(app.snapshot().unwrap().pools, pools);
    assert_eq!(ran(&fake, "zpool import"), 1);
    assert_eq!(ran(&fake, "zdb -e -C 7"), 1);
}

#[test]
fn a_failed_scan_keeps_what_the_last_one_found() {
    let fake = machine();
    let runtime = Runtime::default();
    let seen = Seen::new();
    let app = App::new(Context::new(fake.system()), &runtime).with_seen(&seen);
    let first = scanned(app.apply(&Action::ScanPools, Through::Quietly(fake.system())));

    fake.runner.first(|command| {
        (command.display() == "zpool import").then(|| Err(std::io::Error::other("cancelled")))
    });
    let again = scanned(app.apply(&Action::ScanPools, Through::Quietly(fake.system())));

    assert_eq!(again, first);
    // Without anything to keep it in, a failed scan finds nothing.
    let alone = App::new(Context::new(fake.system()), &runtime);
    assert!(scanned(alone.apply(&Action::ScanPools, Through::Quietly(fake.system()))).is_empty());
}

#[test]
fn a_scan_that_cannot_read_the_imported_pools_answers_nothing() {
    let fake = FakeSystem::new();
    fake.runner.respond_program("zpool", 1, "");
    let runtime = Runtime::default();
    let app = App::new(Context::new(fake.system()), &runtime);

    let events = app.apply(&Action::ScanPools, Through::Quietly(fake.system()));

    // No failure, whose dialog would stop the countdown: the menu keeps
    // the pools it shows.
    assert!(events.is_empty(), "{events:?}");
}

#[test]
fn the_locked_menu_lets_the_pools_be_scanned() {
    let fake = machine();
    fake.files.file("/zfsbootmenu/bootfs", "tank/ROOT/be\n");
    let runtime = Runtime::default();
    let authority = Authority::new(auth::Lock::On("hash".to_owned()));
    let app = App::new(Context::new(fake.system()), &runtime).with_authority(&authority);

    assert!(app.snapshot().unwrap().menu_locked);
    assert_eq!(
        scanned(app.apply(&Action::ScanPools, Through::Quietly(fake.system()))).len(),
        2
    );
}

#[test]
fn what_may_import_a_pool_or_open_its_devices_scans_again() {
    let fake = machine();
    let runtime = Runtime::default();
    let seen = Seen::new();
    let app = App::new(Context::new(fake.system()), &runtime).with_seen(&seen);
    let import = Action::Import(
        "backup".parse().unwrap(),
        crate::model::ImportOptions::default(),
    );

    // Quietly, the import fails, as it takes the console; the scan runs.
    app.apply(&import, Through::Quietly(fake.system()));
    assert_eq!(ran(&fake, "zpool import"), 1);
    assert_eq!(app.snapshot().unwrap().pools.len(), 2);
    // A reboot moves no pool.
    app.apply(&Action::Reboot, Through::Quietly(fake.system()));
    assert_eq!(ran(&fake, "zpool import"), 1);
    // Without anything to keep a scan in, none runs.
    let alone = App::new(Context::new(fake.system()), &runtime);
    alone.apply(&import, Through::Quietly(fake.system()));
    assert_eq!(ran(&fake, "zpool import"), 1);

    let open = Action::Open {
        device: "/dev/sda2".to_owned(),
        name: None,
    };
    for action in [import, open, Action::UnlockAll] {
        assert!(may_move_pools(&action), "{action:?}");
    }
    assert!(may_move_pools(&Action::Chroot(
        "tank/ROOT/be".parse::<zbm_types::FilesystemName>().unwrap()
    )));
    assert!(!may_move_pools(&Action::ScanPools));
}

#[test]
fn an_export_scans_again_and_the_pool_is_listed_to_import() {
    let fake = machine();
    let runtime = Runtime::default();
    let seen = Seen::new();
    let app = App::new(Context::new(fake.system()), &runtime).with_seen(&seen);
    let export = Action::Export("tank".parse().unwrap());
    assert!(may_move_pools(&export));

    let events = app.apply(&export, Through::Quietly(fake.system()));

    assert!(
        matches!(
            events.last(),
            Some(Event::Finished(crate::model::Outcome::Done(_)))
        ),
        "{events:?}"
    );
    assert_eq!(ran(&fake, "zpool export tank"), 1);
    assert_eq!(ran(&fake, "zpool import"), 1);
    let pools = app.snapshot().unwrap().pools;
    assert!(
        pools
            .iter()
            .any(|pool| pool.name.as_str() == "tank" && !pool.imported),
        "{pools:?}"
    );
}

#[test]
fn the_locked_menu_exports_only_once_the_pin_is_typed() {
    let fake = machine();
    fake.files.file("/zfsbootmenu/bootfs", "tank/ROOT/be\n");
    let runtime = Runtime::default();
    let authority = Authority::new(auth::Lock::On("hash".to_owned()));
    let app = App::new(Context::new(fake.system()), &runtime).with_authority(&authority);
    // The pool of the default environment, which the lock keeps.
    let export = Action::Export("tank".parse().unwrap());

    let refused = app.apply(&export, Through::Quietly(fake.system()));
    assert_eq!(refused, auth::refused());
    assert_eq!(ran(&fake, "zpool export tank"), 0);

    let (client, keyholder) = zbm_sys::socket::Connection::pair().unwrap();
    fake.console.secret(b"correct horse");
    let runner = &fake.runner;
    let events = std::thread::scope(|scope| {
        zbm_platform::event::spawn_scoped_traced(scope, move || {
            let _ = crate::keyholder::serve(
                &keyholder,
                runner,
                |_, _| Vec::new(),
                |pin| pin.expose() == b"correct horse",
            );
        });
        let keyholders = crate::keyholder::tests::Given::new(client);
        let recovery = super::super::tests::NoShell;
        let events = app
            .with_keyholders(&keyholders)
            .apply(&export, Through::Console(fake.system(), &recovery));
        drop(keyholders);
        events
    });

    assert_ne!(events, auth::refused());
    assert!(fake.console.output_text().contains(auth::LOCKED_PROMPT));
    assert_eq!(ran(&fake, "zpool export tank"), 1);
}

#[test]
fn a_devices_messages_are_read_under_the_lock_without_a_pin() {
    let fake = machine();
    fake.files.file("/zfsbootmenu/bootfs", "tank/ROOT/be\n");
    fake.files.file("/sys/class/block/vdb/dev", "252:16\n");
    fake.files.block_device("/dev/vdb");
    fake.files
        .file("/dev/kmsg", "3,1,1000000,-;vdb: I/O error\n");
    let runtime = Runtime::default();
    let authority = Authority::new(auth::Lock::On("hash".to_owned()));
    let app = App::new(Context::new(fake.system()), &runtime).with_authority(&authority);
    let device = zbm_types::Untrusted::new("vdb".to_owned());

    for through in [
        Through::Quietly(fake.system()),
        Through::Console(fake.system(), &super::super::tests::NoShell),
    ] {
        let events = app.apply(&Action::LoadDeviceLog(device.clone()), through);
        let [Event::DeviceLogLoaded(log)] = events.as_slice() else {
            panic!("{events:?}");
        };
        assert_eq!(log.lines.len(), 1, "{log:?}");
    }
    assert!(!fake.console.output_text().contains(auth::LOCKED_PROMPT));
    assert!(!may_move_pools(&Action::LoadDeviceLog(device)));
}

#[test]
fn the_snapshot_has_no_default_once_its_pool_is_exported() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use zbm_platform::{Output, Status};

    let fake = FakeSystem::new();
    // The state file names the default; the export leaves it.
    fake.files.file("/zfsbootmenu/bootfs", "tank/ROOT/be\n");
    let exported = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&exported);
    // `tank` is imported until `zpool export tank` runs.
    fake.runner.on(move |command| {
        let line = command.display();
        if line == "zpool export tank" {
            seen.store(true, Ordering::SeqCst);
        }
        let listed = line == "zpool list -H -p -o name,health,readonly,bootfs,guid"
            && !seen.load(Ordering::SeqCst);
        let row = b"tank\tONLINE\toff\ttank/ROOT/be\t7\n";
        listed.then(|| Ok(Output::new(Status::exited(0), row)))
    });
    fake.runner.respond("zfs version -j", 1, "");
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    let runtime = Runtime::default();
    let app = App::new(Context::new(fake.system()), &runtime);
    let bootfs = |app: &App<'_>| app.snapshot().unwrap().bootfs.map(|name| name.to_string());

    assert_eq!(bootfs(&app).as_deref(), Some("tank/ROOT/be"));
    let export = Action::Export("tank".parse().unwrap());
    app.apply(&export, Through::Quietly(fake.system()));

    assert!(exported.load(Ordering::SeqCst));
    assert_eq!(bootfs(&app), None);
}

mod history;
mod snapshots;
