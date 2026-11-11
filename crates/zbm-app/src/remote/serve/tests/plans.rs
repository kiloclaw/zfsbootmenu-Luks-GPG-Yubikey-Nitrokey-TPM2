//! Plans for a remote menu: only of what the menu shows, as their reviews
//! read what the lock leaves free.

use pretty_assertions::assert_eq;

use super::*;
use crate::model::{Access, ImportOptions};

/// A machine whose pool `tank`, of identifier 7, has the boot environment
/// `tank/ROOT/be`.
fn shown() -> FakeSystem {
    let fake = machine();
    fake.runner.respond("zfs version -j", 1, "");
    fake.runner.respond(
        "zpool list -H -p -o name,health,readonly,bootfs,guid",
        0,
        "tank\tONLINE\toff\t-\t7\n",
    );
    fake.runner.respond(
        "zpool status -p -P",
        0,
        "  pool: tank\n state: ONLINE\nconfig:\n\n\tNAME STATE READ WRITE CKSUM\n\
         \ttank ONLINE 0 0 0\n\t  /dev/sda2 ONLINE 0 0 0\n\nerrors: No known data errors\n",
    );
    let listing = |name: &str| {
        format!("zfs list -H -p -t filesystem -o name,mountpoint,canmount,keystatus -r {name}")
    };
    fake.runner.respond(
        &listing("tank/ROOT/be"),
        0,
        "tank/ROOT/be\t/\tnoauto\t-\ntank/ROOT/be/home\t/home\ton\t-\n",
    );
    fake.runner
        .respond(&listing("tank/ROOT"), 0, "tank/ROOT\tnone\ton\t-\n");
    fake
}

fn plan(action: Action) -> Action {
    Action::Plan(Box::new(action))
}

fn forced(pool: &str, id: Option<u64>) -> Action {
    let options = ImportOptions {
        access: Access::ReadOnly,
        policy: zbm_types::ImportPolicy::Force,
        rewind: false,
        id,
    };
    Action::Import(pool.parse().unwrap(), options)
}

#[test]
fn a_remote_menu_plans_only_what_it_is_shown() {
    let fake = shown();
    let rollback = |name: &str| Action::Rollback(name.parse().unwrap());
    let environment = Some("the machine has no such boot environment");
    let pool = Some("the machine shows no such pool");
    let unplanned = Some("only a change has a plan");
    let key_file = Action::OpenWithKeyFile {
        device: "/dev/sda2".parse().unwrap(),
        name: "v".parse().unwrap(),
        key_file: "/etc/key".into(),
        options: zbm_crypt::OpenOptions::default(),
    };

    for (action, refused) in [
        (rollback("tank/ROOT/be@old"), None),
        (rollback("tank/ROOT@old"), environment),
        (rollback("tank/ROOT/be/home@old"), environment),
        (rollback("tank/gone@old"), environment),
        (forced("tank", None), None),
        (forced("tank", Some(7)), None),
        (forced("tank", Some(9)), pool),
        (forced("other", None), pool),
        (Action::Export("tank".parse().unwrap()), None),
        (Action::Export("other".parse().unwrap()), pool),
        // What is not a change has no plan, and is not performed either:
        // the volumes are opened one at a time, as the menu shows them.
        (Action::Reboot, unplanned),
        (Action::Close(None), unplanned),
        (Action::UnlockAll, unplanned),
        (key_file, unplanned),
        (plan(Action::Export("tank".parse().unwrap())), unplanned),
    ] {
        assert_eq!(
            refusal_of(&fake, &plan(action.clone())).as_deref(),
            refused,
            "{action:?}"
        );
    }
}

#[test]
fn the_session_refuses_the_plan_of_what_has_none() {
    let fake = shown();
    let snapshot = |name: &str| -> zbm_types::SnapshotName { name.parse().unwrap() };
    let environment: DatasetName = "tank/ROOT/be".parse().unwrap();
    let unplanned = Some("only a change has a plan");

    for action in [
        // Of a boot environment the machine has, and still no plan: the
        // differences change nothing, and nothing is read for them.
        Action::Diff(snapshot("tank/ROOT/be@old"), None),
        Action::Diff(
            snapshot("tank/ROOT/be@old"),
            Some(snapshot("tank/ROOT/be@new")),
        ),
        Action::ResetBootTries(environment.clone()),
        Action::LoadSnapshots(environment.clone()),
        Action::Boot {
            environment,
            kernel: None,
            command_line: None,
            countdown: false,
        },
        Action::Chroot("tank/ROOT/be".parse().unwrap()),
        Action::ScanPools,
    ] {
        assert_eq!(
            refusal_of(&fake, &plan(action.clone())).as_deref(),
            unplanned,
            "{action:?}"
        );
    }
    let lines = fake.runner.lines();
    assert!(
        lines.iter().all(|line| !line.starts_with("zfs list")),
        "{lines:?}"
    );
    // The boot of a snapshot has its clone's plan.
    let boot = Action::BootSnapshot(
        snapshot("tank/ROOT/be@old"),
        "tank/ROOT/new".parse().unwrap(),
    );
    assert_eq!(refusal_of(&fake, &plan(boot)), None);
}

#[test]
fn a_refused_plan_reads_nothing_of_what_it_names() {
    let fake = shown();

    let messages = answered(&fake, plan(forced("other", Some(9))));

    let [ToMenu::Event(event), ToMenu::Done] = messages.as_slice() else {
        panic!("{messages:?}");
    };
    let Event::Finished(Outcome::Failed { why, .. }) = &**event else {
        panic!("{event:?}");
    };
    assert_eq!(why, &["the machine shows no such pool"]);
    let lines = fake.runner.lines();
    assert!(
        lines
            .iter()
            .all(|line| line != "zpool import" && !line.starts_with("zdb")),
        "{lines:?}"
    );
}

#[test]
fn a_remote_menu_destroys_and_plans_to_destroy_snapshots_of_boot_environments_alone() {
    let fake = shown();
    let destroy =
        |names: &[&str]| Action::Destroy(names.iter().map(|name| name.parse().unwrap()).collect());
    let environment = Some("the machine has no such boot environment");

    for (action, refused) in [
        (destroy(&["tank/ROOT/be@old", "tank/ROOT/be@new"]), None),
        (destroy(&["tank/ROOT@old"]), environment),
        (destroy(&["tank/ROOT/be/home@old"]), environment),
        // The first names a boot environment, the second does not.
        (destroy(&["tank/ROOT/be@old", "tank/ROOT@old"]), environment),
        (destroy(&["tank/gone@old"]), environment),
    ] {
        assert_eq!(refusal_of(&fake, &action).as_deref(), refused, "{action:?}");
        assert_eq!(
            refusal_of(&fake, &plan(action.clone())).as_deref(),
            refused,
            "{action:?}"
        );
    }
}

#[test]
fn a_remote_menu_saves_no_line_with_a_control_character_nor_plans_one() {
    let fake = shown();
    let line = zbm_types::Untrusted::new("quiet\nsingle".to_owned());
    let saved = Action::SetCommandLine("tank/ROOT/be".parse().unwrap(), line);
    // Its plan is of a boot environment the menu shows: the service takes
    // it, and the plan itself refuses the line, with no step.
    assert_eq!(refusal_of(&fake, &plan(saved.clone())), None);
    let messages = answered(&fake, plan(saved.clone()));
    let [ToMenu::Event(event), ToMenu::Done] = messages.as_slice() else {
        panic!("{messages:?}");
    };
    let Event::Planned { plan: planned, .. } = &**event else {
        panic!("{event:?}");
    };
    assert!(planned.steps.is_empty(), "{planned:?}");
    let Outcome::Failed { why, .. } = &planned.outcome else {
        panic!("{planned:?}");
    };
    assert_eq!(why, &["the command line holds a control character"]);

    let (ended, messages) = served(&fake, &no_console, &never, |menu| {
        ToSession::Send { action: saved }.send(menu, &[]).unwrap();
        let messages = until_done(menu);
        ToSession::Detach.send(menu, &[]).unwrap();
        messages
    });
    ended.unwrap();

    let failed = messages.iter().find_map(|message| match message {
        ToMenu::Event(event) => match &**event {
            Event::Finished(Outcome::Failed { why, .. }) => Some(why.clone()),
            _ => None,
        },
        _ => None,
    });
    assert_eq!(
        failed,
        Some(vec![
            "the command line holds a control character".to_owned()
        ]),
        "{messages:?}"
    );
    let lines = fake.runner.lines();
    assert!(
        !lines.iter().any(|line| line.starts_with("zfs set")
            || line.starts_with("zfs inherit")
            || line.contains("readonly tank")),
        "{lines:#?}"
    );
}
