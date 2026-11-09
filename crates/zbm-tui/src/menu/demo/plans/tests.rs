//! The demo's plans, against what `zbm-app` plans on the machine.

use pretty_assertions::assert_eq;

use super::super::{DemoOptions, FakeMachine, data, untrusted};
use super::plan_of;
use crate::menu::world::{Access, Action, Event, ImportOptions, Outcome, Review, StepKind};

/// One action of each kind that has a plan, on the demo machine, and some
/// that have none.
fn changes() -> Vec<Action> {
    let snapshot: zbm_types::SnapshotName = "zroot/ROOT/ubuntu@2026-10-01".parse().unwrap();
    let environment: zbm_types::DatasetName = "zroot/ROOT/ubuntu".parse().unwrap();
    let new: zbm_types::DatasetName = "zroot/ROOT/new".parse().unwrap();
    let tank: zbm_types::PoolName = "tank".parse().unwrap();
    let import = |pool: &str, access, policy| {
        let options = ImportOptions {
            access,
            policy,
            rewind: false,
            id: None,
        };
        Action::Import(pool.parse().unwrap(), options)
    };
    vec![
        Action::CreateSnapshot("zroot/ROOT/ubuntu@new".parse().unwrap()),
        Action::Clone(snapshot.clone(), new.clone()),
        Action::CloneAndPromote(snapshot.clone(), new.clone()),
        Action::Duplicate(snapshot.clone(), new.clone()),
        Action::BootSnapshot(snapshot.clone(), new),
        Action::RestoreFiles {
            snapshot,
            paths: vec![untrusted("/etc/fstab")],
        },
        Action::SetDefaultEnvironment(environment.clone()),
        Action::SetDefaultKernel(environment.clone(), untrusted("6.8.0-48-generic")),
        Action::SetDefaultKernel("tank/ROOT/arch".parse().unwrap(), untrusted("6.11.5")),
        Action::InheritDefaultKernel(environment.clone()),
        import("tank", Access::ReadWrite, zbm_types::ImportPolicy::Strict),
        import(
            "archive",
            Access::ReadWrite,
            zbm_types::ImportPolicy::Strict,
        ),
        import("archive", Access::ReadOnly, zbm_types::ImportPolicy::Hostid),
        Action::Export(tank),
        Action::Rollback("zroot/ROOT/void@pre-upgrade".parse().unwrap()),
        Action::Destroy(vec!["tank/ROOT/arch@2026-10-01".parse().unwrap()]),
        Action::ResetBootTries(environment.clone()),
        Action::Diff("zroot/ROOT/ubuntu@2026-10-01".parse().unwrap(), None),
        Action::Chroot(environment.into()),
        Action::Reboot,
    ]
}

#[test]
fn the_demo_plans_every_change_it_offers() {
    let mut machine = FakeMachine::new(&DemoOptions {
        locked: true,
        ..DemoOptions::default()
    });

    for action in changes() {
        assert!(machine.snapshot().is_offered(&action), "{action:?}");
        let events = machine.apply(Action::Plan(Box::new(action.clone())));
        let [event] = events.as_slice() else {
            panic!("{events:?}");
        };
        match event {
            Event::Planned { plan, .. } => {
                assert!(action.has_plan(), "{action:?}");
                assert!(matches!(plan.outcome, Outcome::Done(_)), "{plan:?}");
                // Each changes something, with the command a shell takes.
                let commands: Vec<&str> = plan
                    .steps
                    .iter()
                    .filter_map(|s| s.command.as_deref())
                    .collect();
                assert!(!commands.is_empty(), "{action:?}");
            }
            Event::Finished(Outcome::Failed { what, .. }) => {
                assert!(!action.has_plan(), "{action:?}");
                assert_eq!(what, "nothing to plan");
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn a_demo_plan_needs_what_the_change_needs_and_says_its_undo() {
    let mut machine = FakeMachine::new(&DemoOptions {
        locked: true,
        ..DemoOptions::default()
    });
    let planned = |machine: &mut FakeMachine, action: Action| {
        let events = machine.apply(Action::Plan(Box::new(action)));
        let [Event::Planned { plan, .. }] = events.as_slice() else {
            panic!("{events:?}");
        };
        plan.clone()
    };
    let ubuntu: zbm_types::DatasetName = "zroot/ROOT/ubuntu".parse().unwrap();
    let pool: zbm_types::PoolName = "zroot".parse().unwrap();

    let take = Action::CreateSnapshot("zroot/ROOT/ubuntu@new".parse().unwrap());
    let plan = planned(&mut machine, take);
    let kinds: Vec<&StepKind> = plan.steps.iter().map(|step| &step.kind).collect();
    assert_eq!(
        kinds,
        [
            &StepKind::ImportReadWrite(pool.clone()),
            &StepKind::LoadKey(ubuntu.clone()),
            &StepKind::Snapshot("zroot/ROOT/ubuntu@new".parse().unwrap()),
        ]
    );
    assert!(matches!(
        plan.review.undo.as_deref(),
        Some(Action::Destroy(_))
    ));

    // The default it replaces undoes a new one.
    let plan = planned(&mut machine, Action::SetDefaultEnvironment(ubuntu.clone()));
    assert_eq!(
        plan.review.undo.as_deref(),
        Some(&Action::SetDefaultEnvironment(
            "zroot/ROOT/void".parse().unwrap()
        ))
    );
    assert_eq!(
        plan.steps.last().and_then(|step| step.command.as_deref()),
        Some("zpool set bootfs=zroot/ROOT/ubuntu zroot")
    );

    // A kernel's selector is quoted; a kernel set where one was is kept.
    let set = Action::SetDefaultKernel(ubuntu.clone(), untrusted("6.8.0-48-generic"));
    let plan = planned(&mut machine, set);
    assert_eq!(
        plan.steps.last().and_then(|step| step.command.as_deref()),
        Some("zfs set org.zfsbootmenu:kernel='/vmlinuz-6\\.8\\.0-48-generic$' zroot/ROOT/ubuntu")
    );
    assert_eq!(plan.review.undo, None);
    let void = Action::SetDefaultKernel("zroot/ROOT/void".parse().unwrap(), untrusted("6.12.8_1"));
    let plan = planned(&mut machine, void);
    assert!(matches!(
        plan.review.undo.as_deref(),
        Some(Action::InheritDefaultKernel(_))
    ));

    // The read/write toggle is undone by the access the pool has, and a
    // pool with the access asked changes nothing.
    let toggle = |access| {
        let options = ImportOptions {
            access,
            ..ImportOptions::default()
        };
        Action::Import(pool.clone(), options)
    };
    let plan = planned(&mut machine, toggle(Access::ReadWrite));
    assert_eq!(plan.review.undo.as_deref(), Some(&toggle(Access::ReadOnly)));
    assert!(
        planned(&mut machine, toggle(Access::ReadOnly))
            .steps
            .is_empty()
    );
}

#[test]
fn a_demo_import_as_the_last_host_adopts_a_host_id_only_of_another_host_s_pool() {
    let options = DemoOptions {
        exported: true,
        ..DemoOptions::default()
    };
    let machine = FakeMachine::new(&options);
    let mut snapshot = machine.snapshot().clone();
    snapshot.pools.extend(data::exported(&options));
    let hostid = |id| ImportOptions {
        policy: zbm_types::ImportPolicy::Hostid,
        id,
        ..ImportOptions::default()
    };
    let archive: zbm_types::PoolName = "archive".parse().unwrap();
    let action = Action::Import(archive.clone(), hostid(None));
    assert!(plan_of(&snapshot, &action).review.host_id_stays);

    // A pool this host used last imports as it is, and is exported again.
    for pool in &mut snapshot.pools {
        pool.host = None;
    }
    let review = plan_of(&snapshot, &action).review;
    assert!(!review.host_id_stays, "{review:?}");
    assert_eq!(review.undo.as_deref(), Some(&Action::Export(archive)));
    // The exported `tank` is not the one imported: nothing undoes it.
    let tank = Action::Import("tank".parse().unwrap(), hostid(Some(5)));
    let plan = plan_of(&snapshot, &tank);
    assert!(plan.steps.is_empty() && plan.review == Review::default());
    // What has no plan has no steps.
    assert!(plan_of(&snapshot, &Action::Reboot).steps.is_empty());
}

#[test]
fn a_demo_command_quotes_a_name_a_shell_would_split() {
    let machine = FakeMachine::new(&DemoOptions::default());
    let take = Action::CreateSnapshot("zroot/ROOT/ubuntu@before update".parse().unwrap());

    let plan = plan_of(machine.snapshot(), &take);

    let commands: Vec<&str> = plan
        .steps
        .iter()
        .filter_map(|s| s.command.as_deref())
        .collect();
    assert_eq!(commands, ["zfs snapshot 'zroot/ROOT/ubuntu@before update'"]);
}
