//! The reviews plans bring on a machine, whose reads change nothing.

use pretty_assertions::assert_eq;

use super::*;
use crate::model::{Access, ImportOptions, Loss, PoolUse};

/// An app on `fake`, whose base is `/zfsbootmenu`, with what it plans.
fn planned(fake: &FakeSystem, action: Action) -> Plan {
    let runtime = Runtime::default().with_base(Path::new("/zfsbootmenu"));
    let app = App::new(Context::new(fake.system()), &runtime);
    let events = app.apply(
        &Action::Plan(Box::new(action)),
        Through::Quietly(fake.system()),
    );
    let [Event::Planned { plan, .. }] = events.as_slice() else {
        panic!("not a plan: {events:?}");
    };
    plan.clone()
}

#[test]
fn a_rollback_plan_reviews_the_newer_snapshots_and_destroys_none() {
    let fake = machine("off");
    fake.runner.respond("zfs version -j", 1, "");
    fake.runner.respond(
        "zfs list -H -p -t snapshot,bookmark -o name,createtxg,used,userrefs,clones -d 1 \
         -s createtxg tank/ROOT/be",
        0,
        "tank/ROOT/be@old\t10\t4096\t0\t-\ntank/ROOT/be@new\t20\t8192\t0\t-\n\
         tank/ROOT/be#new\t20\t-\t-\t-\n",
    );
    fake.runner.respond(
        "zfs get -H -p -o name,property,value,source written@old tank/ROOT/be",
        0,
        "tank/ROOT/be\twritten@old\t65536\t-\n",
    );
    fake.runner.respond(
        "zfs destroy -n -v -p tank/ROOT/be@new%new",
        0,
        "destroy\ttank/ROOT/be@new\nreclaim\t8192\n",
    );
    let old: zbm_types::SnapshotName = "tank/ROOT/be@old".parse().unwrap();

    let plan = planned(&fake, Action::Rollback(old.clone()));

    assert_eq!(
        plan.review.loses,
        [
            Loss::Snapshot {
                name: "tank/ROOT/be@new".parse().unwrap(),
                used: Some(8192),
            },
            Loss::Bookmark {
                dataset: "tank/ROOT/be".parse().unwrap(),
                name: "new".to_owned(),
            },
            Loss::Written {
                since: old,
                bytes: Some(65_536),
            },
        ]
    );
    assert_eq!(plan.review.freed, Some(8192));
    // The snapshot has no kernel: the change refuses it, and the review
    // still says what it would lose.
    assert!(matches!(plan.outcome, Outcome::Failed { .. }), "{plan:?}");
    assert!(changes(&fake).is_empty(), "{:?}", changes(&fake));
    assert!(
        fake.runner
            .lines()
            .contains(&"zfs destroy -n -v -p tank/ROOT/be@new%new".to_owned())
    );
}

#[test]
fn reads_no_one_answers_leave_the_review_unknown() {
    let fake = machine("off");
    let old: zbm_types::SnapshotName = "tank/ROOT/be@old".parse().unwrap();

    let plan = planned(&fake, Action::Rollback(old.clone()));

    assert_eq!(
        plan.review.loses,
        [
            Loss::Newer(old.clone()),
            Loss::Written {
                since: old,
                bytes: None,
            },
        ]
    );
    assert_eq!((plan.review.freed, plan.review.stops.len()), (None, 0));

    // A forced import: nobody known, nothing stops it, an export undoes it.
    let forced = ImportOptions {
        access: Access::ReadOnly,
        policy: zbm_types::ImportPolicy::Force,
        rewind: false,
        id: None,
    };
    let plan = planned(&fake, Action::Import("tank".parse().unwrap(), forced));
    assert_eq!(plan.review.pool_use, Some(PoolUse::default()));
    assert_eq!(
        plan.review.undo,
        Some(Box::new(Action::Export("tank".parse().unwrap())))
    );
    assert!(matches!(plan.outcome, Outcome::Done(_)), "{plan:?}");
    assert!(changes(&fake).is_empty(), "{:?}", changes(&fake));
}

#[test]
fn a_forced_import_of_a_pool_in_use_elsewhere_now_plans_a_refusal() {
    let fake = machine("off");
    fake.runner.respond(
        "zpool import",
        0,
        "   pool: tank\n     id: 42\n  state: ONLINE\n status: The pool is currently imported \
         by another system.\n action: The pool must be exported from otherhost \
         (hostid=deadbeef) before it can be safely imported.\n config:\n\n\
         \ttank        ONLINE\n\t  sda       ONLINE\n",
    );
    let forced = ImportOptions {
        access: Access::ReadOnly,
        policy: zbm_types::ImportPolicy::Force,
        rewind: false,
        id: None,
    };

    let plan = planned(&fake, Action::Import("tank".parse().unwrap(), forced));

    let Outcome::Failed { why, .. } = &plan.outcome else {
        panic!("{plan:?}");
    };
    assert_eq!(why, &["tank is imported by another system now"]);
    assert!(plan.review.pool_use.is_some_and(|used| used.active));
    assert!(changes(&fake).is_empty(), "{:?}", changes(&fake));
}
