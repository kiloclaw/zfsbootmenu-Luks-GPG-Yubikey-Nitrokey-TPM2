use std::path::Path;

use zbm_platform::Context;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::{Application as _, BootSettings, Runtime, Through};

/// A machine whose pool `tank` reads `readonly` as given.
fn machine(readonly: &str) -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner.respond(
        "zpool get -H -o value readonly tank",
        0,
        &format!("{readonly}\n"),
    );
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    fake
}

/// The lines that would have changed the machine.
fn changes(fake: &FakeSystem) -> Vec<String> {
    let writes = ["zfs snapshot", "zpool set", "zpool import", "zpool export"];
    fake.runner
        .lines()
        .into_iter()
        .filter(|line| writes.iter().any(|write| line.starts_with(write)))
        .collect()
}

#[test]
fn a_snapshot_plans_its_command_and_takes_nothing() {
    let fake = machine("off");
    let settings = BootSettings::default();
    let runtime = Runtime::default()
        .with_base(Path::new("/zfsbootmenu"))
        .with_boot(settings.clone());
    let app = App::new(Context::new(fake.system()), &runtime);
    let take = Action::CreateSnapshot("tank/ROOT/be@new".parse().unwrap());

    let plan = app.plan(&take, fake.system()).unwrap();

    assert_eq!(
        plan.steps,
        [Step {
            kind: StepKind::Snapshot("tank/ROOT/be@new".parse().unwrap()),
            command: Some("zfs snapshot tank/ROOT/be@new".into()),
        }]
    );
    assert!(matches!(plan.outcome, Outcome::Done(_)), "{plan:?}");
    assert!(changes(&fake).is_empty(), "{:?}", changes(&fake));
}

#[test]
fn a_read_only_pool_plans_its_import_first() {
    let fake = machine("on");
    let settings = BootSettings::default();
    let runtime = Runtime::default()
        .with_base(Path::new("/zfsbootmenu"))
        .with_boot(settings.clone());
    let app = App::new(Context::new(fake.system()), &runtime);
    let default = Action::SetDefaultEnvironment("tank/ROOT/be".parse().unwrap());

    let events = app.apply(
        &Action::Plan(Box::new(default.clone())),
        Through::Quietly(fake.system()),
    );

    let [Event::Planned { action, plan }] = events.as_slice() else {
        panic!("not a plan: {events:?}");
    };
    assert_eq!(**action, default);
    let kinds: Vec<&StepKind> = plan.steps.iter().map(|step| &step.kind).collect();
    assert_eq!(
        kinds,
        [
            &StepKind::ImportReadWrite("tank".parse().unwrap()),
            &StepKind::SetDefault {
                pool: "tank".parse().unwrap(),
                environment: "tank/ROOT/be".into(),
            },
        ]
    );
    assert_eq!(plan.steps[0].command, None);
    assert_eq!(
        plan.steps[1].command.as_deref(),
        Some("zpool set bootfs=tank/ROOT/be tank")
    );
    assert!(changes(&fake).is_empty(), "{:?}", changes(&fake));
}

#[test]
fn what_changes_nothing_has_no_plan() {
    let fake = machine("off");
    let settings = BootSettings::default();
    let runtime = Runtime::default()
        .with_base(Path::new("/zfsbootmenu"))
        .with_boot(settings.clone());
    let app = App::new(Context::new(fake.system()), &runtime);
    let snapshot: zbm_types::SnapshotName = "tank/ROOT/be@old".parse().unwrap();
    assert!(
        app.plan(&Action::Diff(snapshot, None), fake.system())
            .is_none()
    );
    assert!(app.plan(&Action::Reboot, fake.system()).is_none());

    // Without a plan, the action is answered as one that cannot be done.
    let events = app.apply(
        &Action::Plan(Box::new(Action::Reboot)),
        Through::Quietly(fake.system()),
    );
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { .. }))
    ));

    // Every kind of ZFS change has its step and command.
    let cloned = step(ZfsChange::Clone {
        snapshot: "tank/ROOT/be@old".parse().unwrap(),
        target: "tank/ROOT/new".parse().unwrap(),
        properties: Vec::new(),
    });
    assert_eq!(
        cloned.command.as_deref(),
        Some("zfs clone tank/ROOT/be@old tank/ROOT/new")
    );
    let property = step(ZfsChange::SetPool {
        pool: "tank".parse().unwrap(),
        property: "comment".into(),
        value: "x".into(),
    });
    assert!(matches!(property.kind, StepKind::SetPoolProperty { .. }));
}

#[test]
fn every_change_of_zfs_is_a_step_with_its_command() {
    let snapshot: zbm_types::SnapshotName = "tank/ROOT/be@old".parse().unwrap();
    let dataset: zbm_types::DatasetName = "tank/ROOT/new".parse().unwrap();
    let pool: zbm_types::PoolName = "tank".parse().unwrap();
    let changes = [
        ZfsChange::Export(pool.clone()),
        ZfsChange::Set {
            dataset: dataset.clone(),
            property: "canmount".into(),
            value: "noauto".into(),
        },
        ZfsChange::Inherit {
            dataset: dataset.clone(),
            property: "canmount".into(),
        },
        ZfsChange::Rollback(snapshot.clone()),
        ZfsChange::Promote(dataset.clone()),
        ZfsChange::SendReceive {
            snapshot: snapshot.clone(),
            target: dataset.clone(),
            properties: Vec::new(),
        },
        ZfsChange::InheritKey(dataset.clone()),
    ];
    let kinds: Vec<StepKind> = changes
        .iter()
        .map(|change| {
            let step = step(change.clone());
            assert_eq!(step.command, Some(change.to_string()));
            step.kind
        })
        .collect();
    assert_eq!(
        kinds,
        [
            StepKind::Export(pool),
            StepKind::SetProperty {
                dataset: dataset.clone(),
                property: "canmount".into(),
            },
            StepKind::InheritProperty {
                dataset: dataset.clone(),
                property: "canmount".into(),
            },
            StepKind::Rollback(snapshot.clone()),
            StepKind::Promote(dataset.clone()),
            StepKind::Copy {
                snapshot,
                target: dataset.clone(),
            },
            StepKind::InheritKey(dataset),
        ]
    );
}

#[test]
fn a_rollback_the_change_would_refuse_fails_its_plan() {
    let fake = machine("off");
    let settings = BootSettings::default();
    let runtime = Runtime::default()
        .with_base(Path::new("/zfsbootmenu"))
        .with_boot(settings.clone());
    let app = App::new(Context::new(fake.system()), &runtime);
    // The snapshot has no kernel: nothing would be left to boot.
    let rollback = Action::Rollback("tank/ROOT/be@old".parse().unwrap());

    let plan = app.plan(&rollback, fake.system()).unwrap();

    assert!(matches!(plan.outcome, Outcome::Failed { .. }), "{plan:?}");
    assert!(plan.steps.is_empty());
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("zfs rollback"))
    );
}
