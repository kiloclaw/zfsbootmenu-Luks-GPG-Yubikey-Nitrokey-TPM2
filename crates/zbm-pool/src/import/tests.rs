use std::path::Path;

use pretty_assertions::assert_eq;
use zbm_platform::{Installed, KernelLog};
use zbm_testkit::fakes::FakeSystem;
use zbm_testkit::legacy::stub_lines;

use super::*;
use crate::hostid::ETC_HOSTID;

pub(crate) fn log(fake: &FakeSystem) -> Installed {
    fake.files.dir("/zfsbootmenu");
    KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
}

#[test]
fn import_arguments_follow_the_options() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    let state = State::new(&fake.files, "/zfsbootmenu");
    let context = fake.context();
    let tank = "tank".parse::<PoolName>().unwrap();
    assert_eq!(import_pool(&context, &state, Import::all()), Ok(()));
    let import = Import::pool(&tank)
        .policy(ImportPolicy::Force)
        .access(Access::ReadWrite)
        .rewind(Rewind::ToCheckpoint);
    assert_eq!(import_pool(&context, &state, import), Ok(()));
    let rewind_all = Import::all().rewind(Rewind::ToCheckpoint);
    assert_eq!(
        import_pool(&context, &state, rewind_all),
        Err(PoolError::RewindAll)
    );
    assert_eq!(
        fake.runner.lines(),
        [
            "zpool import -N -o readonly=on -a",
            "zpool import -N -f -o readonly=off --rewind-to-checkpoint tank",
        ]
    );
    assert!(fake.runner.calls()[0].merges_stderr());
    assert_eq!(
        stub_lines(&fake.kmsg()),
        "DEBUG: attempting to import all pools\n\
         DEBUG: read_write unset\n\
         DEBUG: zpool import arguments: -N -o readonly=on -a\n\
         DEBUG: successful pool import\n\
         DEBUG: pool set to tank\n\
         DEBUG: import_policy set: force\n\
         DEBUG: read_write set: yes\n\
         DEBUG: rewind_to_checkpoint set: yes\n\
         DEBUG: zpool import arguments: -N -f -o readonly=off --rewind-to-checkpoint tank\n\
         DEBUG: successful pool import\n\
         ERROR: rewind only works on a specific pool\n"
    );
    assert_eq!(Import::target(None), Import::all());
    assert_eq!(Import::target(Some(&tank)), Import::pool(&tank));
}

#[test]
fn failed_imports_log_the_apparent_host_id() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.files.file(ETC_HOSTID, [0x0c, 0xb1, 0xba, 0x00]);
    fake.runner.respond(
        "zpool import -N -o readonly=on tank",
        1,
        "cannot import\n\n",
    );
    let state = State::new(&fake.files, "/zfsbootmenu");
    let tank = "tank".parse::<PoolName>().unwrap();
    assert_eq!(
        import_pool(&fake.context(), &state, Import::pool(&tank)),
        Err(PoolError::Import {
            target: "tank".to_owned(),
            status: 1
        })
    );
    let lines = stub_lines(&fake.kmsg());
    assert!(lines.ends_with(
        "DEBUG: hostid from /etc/hostid: 00bab10c\n\
         DEBUG: import process failed with code 1, apparent hostid 0x00bab10c\n\
         DEBUG: cannot import\n"
    ));

    // A zpool that cannot start, and no host ID anywhere.
    let missing = FakeSystem::new();
    let _log = log(&missing);
    missing
        .runner
        .on(|_| Some(Err(std::io::Error::other("no zpool"))));
    let state = State::new(&missing.files, "/zfsbootmenu");
    assert_eq!(
        import_pool(&missing.context(), &state, Import::all()),
        Err(PoolError::Import {
            target: "-a".to_owned(),
            status: 126
        })
    );
    assert!(stub_lines(&missing.kmsg()).contains("apparent hostid unknown"));
}

#[test]
fn online_pools_leave_out_the_preferred_one() {
    let output = "   pool: tank\n     id: 1\n  state: ONLINE\n config:\n\n\
                  \ttank  ONLINE\n\n   pool: dead\n     id: 2\n  state: UNAVAIL\n\
                  \n   pool: boot\n     id: 3\n  state: ONLINE\n";
    assert_eq!(online_pools(output, None), ["tank", "boot"]);
    let boot = "boot".parse::<PoolName>().unwrap();
    assert_eq!(online_pools(output, Some(&boot)), ["tank"]);
}

/// `zpool import` listing the pools of `listing`, each refusing a plain
/// import with the message of a pool of host `deadbeef`.
fn foreign_pools(fake: &FakeSystem, listing: &str) {
    let listing = listing.to_owned();
    fake.runner.on(move |command| {
        let line = command.display();
        let output = if line == "zpool import" {
            listing.clone()
        } else if line.starts_with("zpool import -o readonly=on -N ") {
            "cannot import: Last accessed by host (hostid=deadbeef)\n".to_owned()
        } else {
            return None;
        };
        Some(Ok(Output::new(
            zbm_platform::exec::Status::exited(1),
            output,
        )))
    });
}

#[test]
fn host_id_matching_writes_the_foreign_host_id_and_imports() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.files.dir("/etc");
    foreign_pools(&fake, "   pool: tank\n  state: ONLINE\n");
    let state = State::new(&fake.files, "/zfsbootmenu");
    let settings = ImportSettings {
        policy: ImportPolicy::Hostid,
        ..ImportSettings::default()
    };
    let found = match_hostid(&fake.context(), &state, &settings, None).unwrap();
    assert_eq!(found.pool.as_str(), "tank");
    assert_eq!(found.hostid.as_str(), "deadbeef");
    assert_eq!(
        fake.files.contents(ETC_HOSTID).unwrap(),
        [0xef, 0xbe, 0xad, 0xde]
    );
    assert_eq!(
        fake.runner.lines(),
        [
            "zpool import",
            "zpool import -o readonly=on -N tank",
            "zpool import -N -o readonly=on tank",
        ]
    );
    assert!(stub_lines(&fake.kmsg()).contains("WARN: imported tank with assumed hostid deadbeef"));

    // A release build does not warn.
    let release = FakeSystem::new();
    let _log = log(&release);
    release.files.dir("/etc");
    foreign_pools(&release, "");
    let state = State::new(&release.files, "/zfsbootmenu");
    let settings = ImportSettings {
        build: Build::Release,
        ..settings
    };
    let tank = "tank".parse::<PoolName>().unwrap();
    assert!(match_hostid(&release.context(), &state, &settings, Some(&tank)).is_some());
    assert!(!release.kmsg().contains("assumed hostid"));
}

#[test]
fn host_id_matching_skips_pools_without_a_host_id_or_when_it_cannot_write() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.runner
        .respond("zpool import", 0, "   pool: tank\n  state: ONLINE\n");
    let state = State::new(&fake.files, "/zfsbootmenu");
    let settings = ImportSettings::default();
    assert_eq!(match_hostid(&fake.context(), &state, &settings, None), None);
    assert!(stub_lines(&fake.kmsg()).contains("unable to scrape hostid for tank, skipping"));

    // No /etc to write the host ID to.
    let unwritable = FakeSystem::new();
    let _log = log(&unwritable);
    foreign_pools(&unwritable, "   pool: tank\n  state: ONLINE\n");
    let state = State::new(&unwritable.files, "/zfsbootmenu");
    assert_eq!(
        match_hostid(&unwritable.context(), &state, &settings, None),
        None
    );
    assert!(
        stub_lines(&unwritable.kmsg())
            .contains("failed to set hostid deadbeef, skipping import of pool tank")
    );

    // The import with the matched ID fails.
    let refused = FakeSystem::new();
    let _log = log(&refused);
    refused.files.dir("/etc");
    foreign_pools(&refused, "   pool: tank\n  state: ONLINE\n");
    refused
        .runner
        .respond("zpool import -N -o readonly=on tank", 1, "");
    let state = State::new(&refused.files, "/zfsbootmenu");
    assert_eq!(
        match_hostid(&refused.context(), &state, &settings, None),
        None
    );
}

#[test]
fn unavailable_pools_say_why() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.runner.respond(
        "zpool import",
        0,
        "   pool: dead\n     id: 42\n  state: UNAVAIL\n\n\
            \x20  pool: fine\n     id: 7\n  state: ONLINE\n",
    );
    fake.runner.respond(
        "zpool import -N 42",
        1,
        "cannot import 'dead': one or more devices is currently unavailable\n",
    );
    log_unimportable(fake.system().runner);
    assert_eq!(fake.runner.lines(), ["zpool import", "zpool import -N 42"]);
    assert!(
        stub_lines(&fake.kmsg())
            .contains("ERROR: cannot import 'dead': one or more devices is currently unavailable")
    );
}

#[test]
fn imported_pools_are_listed_by_name() {
    let fake = FakeSystem::new();
    fake.runner
        .respond("zpool list -H -o name", 0, "tank\nboot\n");
    assert_eq!(imported_pools(fake.system().runner), ["tank", "boot"]);
    let none = FakeSystem::new();
    none.runner.respond("zpool list -H -o name", 0, "");
    assert!(imported_pools(none.context().runner()).is_empty());
}
