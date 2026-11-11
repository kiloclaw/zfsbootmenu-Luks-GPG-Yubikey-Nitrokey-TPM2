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

/// The lines that would have changed the machine: not the scan of the
/// pools, `zpool import` alone, nor the count of a destroy, `zfs destroy
/// -n`.
fn changes(fake: &FakeSystem) -> Vec<String> {
    let writes = [
        "zfs snapshot",
        "zfs set",
        "zfs inherit",
        "zfs rollback",
        "zfs destroy",
        "zpool set",
        "zpool import",
        "zpool export",
    ];
    fake.runner
        .lines()
        .into_iter()
        .filter(|line| writes.iter().any(|write| line.starts_with(write)))
        .filter(|line| line != "zpool import" && !line.starts_with("zfs destroy -n "))
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
fn an_export_plans_its_command_and_exports_nothing() {
    let fake = machine("off");
    let runtime = Runtime::default().with_base(Path::new("/zfsbootmenu"));
    let app = App::new(Context::new(fake.system()), &runtime);

    let plan = app
        .plan(&Action::Export("tank".parse().unwrap()), fake.system())
        .unwrap();

    assert_eq!(
        plan.steps,
        [Step {
            kind: StepKind::Export("tank".parse().unwrap()),
            command: Some("zpool export tank".into()),
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
        ZfsChange::Destroy(vec![snapshot.clone()]),
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
            StepKind::Destroy(vec![snapshot.clone()]),
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

#[test]
fn a_default_kernel_and_a_restore_plan_their_steps_and_change_nothing() {
    let fake = machine("off");
    let mnt = "/zfsbootmenu/environments/tank/ROOT/be/mnt";
    fake.files
        .file(format!("{mnt}/boot/vmlinuz-6.12.9+"), "kernel")
        .file(format!("{mnt}/boot/initramfs-6.12.9+.img"), "initramfs")
        .file(format!("{mnt}/.zfs/snapshot/old/etc/fstab"), "fstab");
    let runtime = Runtime::default().with_base(Path::new("/zfsbootmenu"));
    let app = App::new(Context::new(fake.system()), &runtime);
    let environment: zbm_types::DatasetName = "tank/ROOT/be".parse().unwrap();
    let version = zbm_types::Untrusted::new("6.12.9+".to_owned());

    let set = Action::SetDefaultKernel(environment.clone(), version);
    let plan = app.plan(&set, fake.system()).unwrap();
    assert_eq!(
        plan.steps,
        [Step {
            kind: StepKind::SetProperty {
                dataset: environment.clone(),
                property: "org.zfsbootmenu:kernel".into(),
            },
            command: Some(
                "zfs set org.zfsbootmenu:kernel='/vmlinuz-6\\.12\\.9\\+$' tank/ROOT/be".into()
            ),
        }]
    );
    let inherit = Action::InheritDefaultKernel(environment.clone());
    let plan = app.plan(&inherit, fake.system()).unwrap();
    assert!(matches!(
        plan.steps.as_slice(),
        [Step {
            kind: StepKind::InheritProperty { .. },
            ..
        }]
    ));

    let paths = vec![zbm_types::Untrusted::new("/etc/fstab".to_owned())];
    let restore = Action::RestoreFiles {
        snapshot: "tank/ROOT/be@old".parse().unwrap(),
        paths: paths.clone(),
    };
    let plan = app.plan(&restore, fake.system()).unwrap();
    let kinds: Vec<&StepKind> = plan.steps.iter().map(|step| &step.kind).collect();
    let [
        StepKind::Snapshot(safety),
        StepKind::Restore {
            snapshot,
            paths: restored,
        },
    ] = kinds.as_slice()
    else {
        panic!("not a safety snapshot and a restore: {plan:?}");
    };
    assert!(safety.name().starts_with("before-restore-"), "{safety}");
    assert_eq!(safety.dataset(), &environment);
    assert_eq!((snapshot.name(), restored), ("old", &paths));
    assert!(matches!(plan.outcome, Outcome::Done(_)), "{plan:?}");
    assert!(changes(&fake).is_empty(), "{:?}", changes(&fake));
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("cp"))
    );
}

#[test]
fn an_import_plans_the_export_and_the_import_and_moves_nothing() {
    let fake = machine("on");
    fake.runner.respond("zpool list -H -o name", 0, "tank\n");
    let runtime = Runtime::default().with_base(Path::new("/zfsbootmenu"));
    let app = App::new(Context::new(fake.system()), &runtime);
    let options = crate::model::ImportOptions {
        access: crate::model::Access::ReadWrite,
        policy: zbm_types::ImportPolicy::Strict,
        rewind: false,
        id: None,
    };
    let import = Action::Import("tank".parse().unwrap(), options);

    let events = app.apply(
        &Action::Plan(Box::new(import)),
        Through::Quietly(fake.system()),
    );

    let [Event::Planned { plan, .. }] = events.as_slice() else {
        panic!("not a plan: {events:?}");
    };
    let commands: Vec<Option<&str>> = plan
        .steps
        .iter()
        .map(|step| step.command.as_deref())
        .collect();
    assert_eq!(
        commands,
        [
            Some("zpool export tank"),
            Some("zpool import -N -o readonly=off tank")
        ]
    );
    assert_eq!(
        plan.outcome,
        Outcome::Done("tank is imported read/write".to_owned())
    );
    assert!(changes(&fake).is_empty(), "{:?}", changes(&fake));
}

#[test]
fn a_snapshot_boot_plans_its_clone_then_the_boot_and_does_neither() {
    let fake = machine("on");
    let runtime = Runtime::default().with_base(Path::new("/zfsbootmenu"));
    let app = App::new(Context::new(fake.system()), &runtime);
    let snapshot: zbm_types::SnapshotName = "tank/ROOT/be@old".parse().unwrap();
    let target: zbm_types::DatasetName = "tank/ROOT/new".parse().unwrap();

    let plan = app
        .plan(
            &Action::BootSnapshot(snapshot.clone(), target.clone()),
            fake.system(),
        )
        .unwrap();

    assert_eq!(
        plan.steps,
        [
            Step {
                kind: StepKind::ImportReadWrite("tank".parse().unwrap()),
                command: None,
            },
            Step {
                kind: StepKind::Clone {
                    snapshot: snapshot.clone(),
                    target: target.clone(),
                },
                command: Some(
                    "zfs clone -o mountpoint=/ -o canmount=noauto tank/ROOT/be@old tank/ROOT/new"
                        .into()
                ),
            },
            Step {
                kind: StepKind::Boot(target),
                command: None,
            },
        ]
    );
    assert!(matches!(plan.outcome, Outcome::Done(_)), "{plan:?}");
    assert!(changes(&fake).is_empty(), "{:?}", changes(&fake));
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("zfs clone") || line.starts_with("kexec"))
    );

    // A clone the change would refuse plans no boot.
    let elsewhere = Action::BootSnapshot(snapshot, "other/ROOT/new".parse().unwrap());
    let plan = app.plan(&elsewhere, fake.system()).unwrap();
    assert!(matches!(plan.outcome, Outcome::Failed { .. }), "{plan:?}");
    assert!(plan.steps.is_empty(), "{plan:?}");
}

mod command_line;
mod has_plan;
mod reviews;

#[test]
fn a_plan_of_a_volume_opens_and_closes_nothing_even_on_a_locked_menu() {
    let fake = crate::volumes::tests::machine();
    fake.files.dir("/zfsbootmenu");
    let runtime = Runtime::default().with_base(Path::new("/zfsbootmenu"));
    let mut settings = zbm_luks::unlocker::Settings::new("/zfsbootmenu");
    settings.open_check = zbm_luks::device::OpenCheck::Markers("/fake/holders".into());
    let luks = zbm_luks::unlocker::Luks {
        context: fake.context(),
        settings: &settings,
        interaction: &crate::read::tests::NoPrompts,
    };
    let authority = crate::auth::Authority::new(crate::auth::Lock::On("hash".to_owned()));
    let app = App::new(Context::new(fake.system()), &runtime)
        .with_luks(luks)
        .with_authority(&authority);
    let key_file = Action::OpenWithKeyFile {
        device: "/dev/sdb2".parse().unwrap(),
        name: "b1".parse().unwrap(),
        key_file: "/etc/key".into(),
        options: zbm_crypt::OpenOptions::default(),
    };
    let open = Action::Open {
        device: "/dev/sdb2".to_owned(),
        name: None,
    };
    let unplanned = vec![Event::Finished(Outcome::Failed {
        what: "nothing to plan".to_owned(),
        why: vec!["only a change has a plan".to_owned()],
        next: Vec::new(),
    })];

    for action in [Action::Close(None), Action::UnlockAll, key_file, open] {
        let plan = Action::Plan(Box::new(action.clone()));
        let events = app.apply(&plan, Through::Quietly(fake.system()));
        assert_eq!(events, unplanned, "{action:?}");
    }
    // Not even the volumes are listed.
    let lines = fake.runner.lines();
    assert!(
        lines.iter().all(|line| !line.starts_with("blkid")),
        "{lines:?}"
    );
}
