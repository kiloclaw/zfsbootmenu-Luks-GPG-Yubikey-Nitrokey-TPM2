//! Tests of the menu's imports, on a fake machine: the exact `zpool`
//! commands.

use zbm_boot::BootSettings;
use zbm_platform::Files as _;
use zbm_platform::exec::{Output, Status};
use zbm_state::State;
use zbm_testkit::fakes::FakeSystem;

use super::*;

const READONLY: &str = "org.zfsbootmenu:readonly";

fn tank() -> PoolName {
    "tank".parse().unwrap()
}

fn options(access: Access, policy: ImportPolicy, rewind: bool) -> ImportOptions {
    ImportOptions {
        access,
        policy,
        rewind,
        id: None,
    }
}

/// Runs `test` with the needs of `fake`, on its console when `asks`, with
/// `zbm.import_policy` as `policy`.
fn with_machine<T>(
    fake: &FakeSystem,
    asks: bool,
    policy: ImportPolicy,
    test: impl FnOnce(&OnMachine<'_, '_>) -> T,
) -> T {
    fake.files.dir("/zfsbootmenu").dir("/etc");
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let settings = BootSettings::default().import_policy(policy);
    let import = ImportSettings::default();
    test(&OnMachine {
        system: &system,
        state: &state,
        settings: &settings,
        import: &import,
        asks,
    })
}

/// Imports `tank` on `fake`'s console with `options`.
fn import_tank(fake: &FakeSystem, options: ImportOptions) -> Result<(), Vec<String>> {
    with_machine(fake, true, ImportPolicy::Hostid, |machine| {
        import(machine, &tank(), options)
    })
}

/// A machine where `tank` is imported, read/write when `writable`, with
/// `flag` as its readonly property and no suspend partition.
fn imported(writable: bool, flag: &str) -> FakeSystem {
    let fake = FakeSystem::new();
    fake.runner.respond("zpool list -H -o name", 0, "tank\n");
    let readonly = if writable { "off\n" } else { "on\n" };
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, readonly);
    fake.runner.respond(
        &format!("zpool get -H -o value {READONLY} tank"),
        0,
        &format!("{flag}\n"),
    );
    fake.runner.respond_program("blkid", 2, "");
    fake
}

/// Gives `tank` on `fake` a checkpoint.
fn with_checkpoint(fake: &FakeSystem) {
    fake.runner
        .respond("zpool get -H -o value checkpoint tank", 0, "1.50M\n");
}

/// The `zpool export` and `zpool import` lines `fake` ran.
fn moves(fake: &FakeSystem) -> Vec<String> {
    fake.runner
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("zpool export") || line.starts_with("zpool import"))
        .collect()
}

#[test]
fn an_imported_pool_is_exported_and_imported_with_the_access_asked() {
    let read_only = imported(false, "-");
    let read_write = options(Access::ReadWrite, ImportPolicy::Strict, false);
    assert_eq!(import_tank(&read_only, read_write), Ok(()));
    assert_eq!(
        moves(&read_only),
        ["zpool export tank", "zpool import -N -o readonly=off tank"]
    );

    let writable = imported(true, "-");
    let read_only = options(Access::ReadOnly, ImportPolicy::Strict, false);
    assert_eq!(import_tank(&writable, read_only), Ok(()));
    assert_eq!(
        moves(&writable),
        ["zpool export tank", "zpool import -N -o readonly=on tank"]
    );
    // Read-only asks nothing of the property.
    assert!(
        !writable
            .runner
            .lines()
            .iter()
            .any(|line| line.contains(READONLY))
    );

    // As asked already: nothing moves.
    let writable = imported(true, "-");
    assert_eq!(import_tank(&writable, read_write), Ok(()));
    assert!(moves(&writable).is_empty());
}

#[test]
fn a_rewind_exports_a_writable_pool_under_the_import_policy() {
    let fake = imported(true, "off");
    with_checkpoint(&fake);
    let rewind = options(Access::ReadWrite, ImportPolicy::Strict, true);

    let done = with_machine(&fake, true, ImportPolicy::Force, |machine| {
        import(machine, &tank(), rewind)
    });

    assert_eq!(done, Ok(()));
    assert_eq!(
        moves(&fake),
        [
            "zpool export tank",
            "zpool import -N -f -o readonly=off --rewind-to-checkpoint tank"
        ]
    );
}

#[test]
fn the_readonly_property_or_a_degraded_pool_refuses_read_write_before_the_export() {
    let read_write = options(Access::ReadWrite, ImportPolicy::Strict, false);
    let flagged = imported(false, "on");
    let why = import_tank(&flagged, read_write).unwrap_err();
    assert_eq!(
        why,
        ["tank may not be imported read/write: org.zfsbootmenu:readonly is on"]
    );
    assert!(moves(&flagged).is_empty());

    let degraded = imported(false, "-");
    with_checkpoint(&degraded);
    degraded.files.dir("/zfsbootmenu");
    State::new(&degraded.files, "/zfsbootmenu")
        .add_degraded("tank")
        .unwrap();
    let rewind = options(Access::ReadWrite, ImportPolicy::Strict, true);
    let why = import_tank(&degraded, rewind).unwrap_err();
    assert_eq!(why, ["tank may not be imported read/write: it is degraded"]);
    assert!(moves(&degraded).is_empty());
}

#[test]
fn a_failed_read_write_import_imports_the_pool_read_only_again() {
    let fake = imported(false, "-");
    fake.runner
        .respond("zpool import -N -o readonly=off tank", 1, "");

    let why = import_tank(
        &fake,
        options(Access::ReadWrite, ImportPolicy::Strict, false),
    )
    .unwrap_err();

    assert_eq!(why, ["zpool import tank failed (status 1)"]);
    assert_eq!(
        moves(&fake),
        [
            "zpool export tank",
            "zpool import -N -o readonly=off tank",
            "zpool import -N -o readonly=on tank"
        ]
    );
}

#[test]
fn a_rewind_of_a_pool_with_no_checkpoint_moves_nothing() {
    let fake = imported(true, "-");
    fake.runner
        .respond("zpool get -H -o value checkpoint tank", 0, "-\n");
    let rewind = options(Access::ReadWrite, ImportPolicy::Strict, true);

    let why = import_tank(&fake, rewind).unwrap_err();

    assert_eq!(why, ["tank has no checkpoint"]);
    assert!(moves(&fake).is_empty());
    let planned = with_machine(&fake, false, ImportPolicy::Strict, |machine| {
        steps(machine, &tank(), rewind)
    });
    assert_eq!(planned, Err(vec!["tank has no checkpoint".to_owned()]));
}

/// A machine where `tank` is not imported until a `zpool import` of it
/// other than `refused`, which fails, succeeds; then read-only, with no
/// readonly property and no suspend partition.
fn importable(refused: Option<&'static str>) -> FakeSystem {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let fake = FakeSystem::new();
    let imported = Arc::new(AtomicBool::new(false));
    fake.runner.first(move |command| {
        let line = command.display();
        if Some(line.as_str()) == refused {
            return Some(Ok(Output::new(Status::exited(1), "")));
        }
        if line.starts_with("zpool import") && line.ends_with(" tank") {
            imported.store(true, Ordering::SeqCst);
        }
        let listed = if imported.load(Ordering::SeqCst) {
            "tank\n"
        } else {
            ""
        };
        (line == "zpool list -H -o name").then(|| Ok(Output::new(Status::exited(0), listed)))
    });
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "on\n");
    fake.runner
        .respond(&format!("zpool get -H -o value {READONLY} tank"), 0, "-\n");
    fake.runner.respond_program("blkid", 2, "");
    fake
}

#[test]
fn a_pool_not_imported_is_imported_read_only_first() {
    let strict = importable(None);
    assert_eq!(
        import_tank(
            &strict,
            options(Access::ReadOnly, ImportPolicy::Strict, false)
        ),
        Ok(())
    );
    assert_eq!(moves(&strict), ["zpool import -N -o readonly=on tank"]);

    // Read/write, forced and rewound: imported read-only with force, then
    // with the checks of an imported pool.
    let forced = importable(None);
    with_checkpoint(&forced);
    let force = options(Access::ReadWrite, ImportPolicy::Force, true);
    assert_eq!(import_tank(&forced, force), Ok(()));
    assert_eq!(
        moves(&forced),
        [
            "zpool import -N -f -o readonly=on tank",
            "zpool export tank",
            "zpool import -N -f -o readonly=off --rewind-to-checkpoint tank"
        ]
    );
    assert!(
        forced
            .runner
            .lines()
            .iter()
            .any(|line| line.contains(READONLY))
    );

    // The readonly property refuses read/write once it is imported.
    let flagged = {
        let fake = importable(None);
        fake.runner.first(|command| {
            (command.display() == format!("zpool get -H -o value {READONLY} tank"))
                .then(|| Ok(Output::new(Status::exited(0), "on\n")))
        });
        fake
    };
    let read_write = options(Access::ReadWrite, ImportPolicy::Strict, false);
    let why = import_tank(&flagged, read_write).unwrap_err();
    assert_eq!(
        why,
        ["tank may not be imported read/write: org.zfsbootmenu:readonly is on"]
    );
    assert_eq!(moves(&flagged), ["zpool import -N -o readonly=on tank"]);
}

#[test]
fn strict_tries_no_other_host_id() {
    let refused = FakeSystem::new();
    refused
        .runner
        .respond("zpool import -N -o readonly=on tank", 1, "");
    let why = import_tank(
        &refused,
        options(Access::ReadWrite, ImportPolicy::Strict, false),
    )
    .unwrap_err();
    assert_eq!(why, ["zpool import tank failed (status 1)"]);
    assert_eq!(moves(&refused), ["zpool import -N -o readonly=on tank"]);
    assert!(!refused.files.exists(std::path::Path::new("/etc/hostid")));
}

/// A machine where `tank`, not imported, was last imported by the host
/// `deadbeef`, and imports only once `/etc/hostid` is written.
fn foreign() -> FakeSystem {
    let fake = FakeSystem::new();
    let files = fake.files.clone();
    fake.runner.first(move |command| {
        let line = command.display();
        let (status, output) = if line == "zpool import -o readonly=on -N tank" {
            (
                1,
                "cannot import: Last accessed by host (hostid=deadbeef)\n",
            )
        } else if line.starts_with("zpool import -N")
            && !files.exists(std::path::Path::new("/etc/hostid"))
        {
            (1, "")
        } else {
            return None;
        };
        Some(Ok(Output::new(Status::exited(status), output)))
    });
    fake
}

#[test]
fn as_its_host_a_pool_is_imported_with_the_host_id_that_owns_it() {
    let fake = foreign();
    let hostid = options(Access::ReadOnly, ImportPolicy::Hostid, false);

    assert_eq!(import_tank(&fake, hostid), Ok(()));

    assert_eq!(
        moves(&fake),
        [
            "zpool import -N -o readonly=on tank",
            "zpool import -o readonly=on -N tank",
            "zpool import -N -o readonly=on tank"
        ]
    );
    assert_eq!(
        fake.files.contents("/etc/hostid").unwrap(),
        [0xef, 0xbe, 0xad, 0xde]
    );
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert_eq!(state.spl_hostid().as_deref(), Some("deadbeef"));
}

#[test]
fn as_its_host_a_rewind_follows_the_import_with_the_host_id() {
    let fake = foreign();
    let files = fake.files.clone();
    fake.runner.first(move |command| {
        let imported = files.exists(std::path::Path::new("/etc/hostid"));
        (command.display() == "zpool list -H -o name" && imported)
            .then(|| Ok(Output::new(Status::exited(0), "tank\n")))
    });
    with_checkpoint(&fake);
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "on\n");
    fake.runner
        .respond(&format!("zpool get -H -o value {READONLY} tank"), 0, "-\n");
    fake.runner.respond_program("blkid", 2, "");
    let rewind = options(Access::ReadWrite, ImportPolicy::Hostid, true);

    assert_eq!(import_tank(&fake, rewind), Ok(()));

    assert_eq!(
        moves(&fake),
        [
            "zpool import -N -o readonly=on tank",
            "zpool import -o readonly=on -N tank",
            "zpool import -N -o readonly=on tank",
            "zpool export tank",
            "zpool import -N -o readonly=off --rewind-to-checkpoint tank",
        ]
    );
}

#[test]
fn a_pool_the_host_id_probe_imports_is_imported() {
    let fake = importable(Some("zpool import -N -o readonly=on tank"));
    let hostid = options(Access::ReadOnly, ImportPolicy::Hostid, false);

    assert_eq!(import_tank(&fake, hostid), Ok(()));

    assert_eq!(
        moves(&fake),
        [
            "zpool import -N -o readonly=on tank",
            "zpool import -o readonly=on -N tank"
        ]
    );
    assert!(!fake.files.exists(std::path::Path::new("/etc/hostid")));
}

#[test]
fn a_host_id_that_does_not_import_it_either_says_so() {
    let fake = FakeSystem::new();
    fake.runner
        .respond("zpool import -N -o readonly=on tank", 1, "");

    let why = import_tank(
        &fake,
        options(Access::ReadOnly, ImportPolicy::Hostid, false),
    )
    .unwrap_err();

    assert_eq!(
        why,
        [
            "zpool import tank failed (status 1)",
            "the host ID that last imported tank did not import it either"
        ]
    );
}

#[test]
fn without_the_console_nothing_is_imported() {
    let fake = imported(false, "-");
    let read_write = options(Access::ReadWrite, ImportPolicy::Strict, false);

    let why = with_machine(&fake, false, ImportPolicy::Hostid, |machine| {
        import(machine, &tank(), read_write)
    });

    assert_eq!(
        why,
        Err(vec!["importing tank takes the console".to_owned()])
    );
    assert!(fake.runner.lines().is_empty());
}

#[test]
fn the_steps_are_the_export_and_the_import_with_their_commands() {
    let rewind = options(Access::ReadWrite, ImportPolicy::Strict, true);
    let writable = imported(true, "-");
    with_checkpoint(&writable);
    let planned = with_machine(&writable, false, ImportPolicy::Force, |machine| {
        steps(machine, &tank(), rewind)
    });
    assert_eq!(
        planned.unwrap(),
        [
            Step {
                kind: StepKind::Export(tank()),
                command: Some("zpool export tank".to_owned()),
            },
            Step {
                kind: StepKind::Import {
                    pool: tank(),
                    options: rewind,
                },
                command: Some(
                    "zpool import -N -f -o readonly=off --rewind-to-checkpoint tank".to_owned()
                ),
            },
        ]
    );
    assert!(moves(&writable).is_empty());

    let read_write = options(Access::ReadWrite, ImportPolicy::Strict, false);
    let none = with_machine(&writable, false, ImportPolicy::Force, |machine| {
        steps(machine, &tank(), read_write)
    });
    assert_eq!(none, Ok(Vec::new()));

    let elsewhere = FakeSystem::new();
    let force = options(Access::ReadOnly, ImportPolicy::Force, false);
    let first = with_machine(&elsewhere, false, ImportPolicy::Strict, |machine| {
        steps(machine, &tank(), force)
    });
    assert_eq!(
        first.unwrap(),
        [Step {
            kind: StepKind::Import {
                pool: tank(),
                options: force,
            },
            command: Some("zpool import -N -f -o readonly=on tank".to_owned()),
        }]
    );
}

#[test]
fn the_steps_of_a_pool_not_imported_say_the_host_id_and_the_import_again() {
    let elsewhere = FakeSystem::new();
    let read_write = options(Access::ReadWrite, ImportPolicy::Hostid, false);

    let planned = with_machine(&elsewhere, false, ImportPolicy::Force, |machine| {
        steps(machine, &tank(), read_write)
    });

    let first = options(Access::ReadOnly, ImportPolicy::Hostid, false);
    assert_eq!(
        planned.unwrap(),
        [
            Step {
                kind: StepKind::Import {
                    pool: tank(),
                    options: first,
                },
                command: Some("zpool import -N -o readonly=on tank".to_owned()),
            },
            Step {
                kind: StepKind::AdoptHostid(tank()),
                command: None,
            },
            Step {
                kind: StepKind::Export(tank()),
                command: Some("zpool export tank".to_owned()),
            },
            Step {
                kind: StepKind::Import {
                    pool: tank(),
                    options: read_write,
                },
                command: Some("zpool import -N -o readonly=off tank".to_owned()),
            },
        ]
    );
    assert!(moves(&elsewhere).is_empty());
}

#[test]
fn a_host_id_that_cannot_be_recorded_still_imports() {
    let fake = foreign();
    fake.files.dir("/etc");
    let system = fake.system();
    let state = State::new(&fake.files, "/missing/zfsbootmenu");
    let settings = BootSettings::default();
    let import_settings = ImportSettings::default();
    let machine = OnMachine {
        system: &system,
        state: &state,
        settings: &settings,
        import: &import_settings,
        asks: true,
    };

    let hostid = options(Access::ReadOnly, ImportPolicy::Hostid, false);
    assert_eq!(import(&machine, &tank(), hostid), Ok(()));
    assert_eq!(state.spl_hostid(), None);
}

mod ids;
