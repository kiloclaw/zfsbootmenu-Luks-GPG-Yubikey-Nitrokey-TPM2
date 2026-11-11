//! Tests of the dry run as drawn: one that loses nothing is not drawn as
//! a danger, and runs with **Run**; it names its undo, or says that it
//! loses nothing, which a review page does not say; every change with a
//! plan has a title of its own; the line view says its steps and asks for
//! no name.

use pretty_assertions::assert_eq;
use ratatui::style::{Modifier, Style};
use zbm_types::{ImportPolicy, PoolName, SnapshotName, Untrusted};

use super::super::tests::{buffer, button, plan, prose, rollback_page, theme};
use crate::menu::model::{Dialog, Model, Reviewing};
use crate::menu::theme::Role;
use crate::menu::view::words;
use crate::menu::world::{Access, Action, ImportOptions, Loss, Outcome, Plan, Review};
use crate::text::text;

/// The dry run of `action` over the demo's Snapshots tab, with `plan`.
fn dry_run(action: Action, plan: Plan) -> Model {
    let mut model = rollback_page().into_model();
    let mut reviewing = Reviewing::dry_run(action);
    reviewing.plan = Some(plan);
    model.dialog = Some(Dialog::Review(Box::new(reviewing)));
    model
}

/// The snapshot `new` of the demo's environment.
fn new_snapshot() -> SnapshotName {
    "zroot/ROOT/void@new".parse().unwrap()
}

/// The style of the frame's top-left corner, as drawn at 80 by 24, below
/// the tab bar.
fn frame_style(model: &Model) -> Style {
    buffer(model, 80, 24)[(0, 1)].style()
}

#[test]
fn a_dry_run_that_loses_nothing_is_not_drawn_as_danger() {
    let theme = theme();
    let take = Action::CreateSnapshot(new_snapshot());
    let undo = Action::Destroy(vec![new_snapshot()]);
    let review = Review {
        undo: Some(Box::new(undo)),
        ..Review::default()
    };
    let model = dry_run(take, plan(review));
    let run = crate::text::title(crate::text::id!("action-run"));

    assert_eq!(frame_style(&model).fg, theme.style(Role::Border).fg);
    let style = button(&model, &run);
    assert!(!style.add_modifier.contains(Modifier::REVERSED));
    assert_eq!(style.bg, theme.style(Role::Selected).bg);
    let asks = text!("dry-run-asks");
    assert!(prose(&model, 120, 40).contains(&asks));

    // One that loses something is.
    let snapshot = new_snapshot();
    let lost = Review {
        loses: vec![Loss::Snapshot {
            name: snapshot.clone(),
            used: None,
        }],
        ..Review::default()
    };
    let model = dry_run(Action::Rollback(snapshot), plan(lost));
    assert_eq!(frame_style(&model).fg, theme.style(Role::Danger).fg);
    let verb = button(&model, &text!("button-roll-back"));
    assert!(verb.add_modifier.contains(Modifier::REVERSED));
    assert!(prose(&model, 120, 40).contains(&text!("review-no-undo")));
}

#[test]
fn a_dry_run_names_its_undo_or_says_it_loses_nothing() {
    let environment: zbm_types::DatasetName = "zroot/ROOT/void".parse().unwrap();
    let old: zbm_types::DatasetName = "zroot/ROOT/old".parse().unwrap();
    let pool: PoolName = "tank".parse().unwrap();
    let read_only = ImportOptions {
        access: Access::ReadOnly,
        policy: ImportPolicy::Strict,
        rewind: false,
        id: None,
    };
    let undone = |undo: Action| Review {
        undo: Some(Box::new(undo)),
        ..Review::default()
    };
    let cases = [
        (
            Action::CreateSnapshot(new_snapshot()),
            undone(Action::Destroy(vec![new_snapshot()])),
            text!("review-undo-destroy", snapshot = "zroot/ROOT/void@new"),
        ),
        (
            Action::Import(pool.clone(), read_only),
            undone(Action::Export(pool.clone())),
            text!("review-undo-export", pool = "tank"),
        ),
        (
            Action::Import(pool.clone(), read_only),
            undone(Action::Import(pool.clone(), read_only)),
            text!(
                "review-undo-import",
                pool = "tank",
                access = text!("pool-read-only")
            ),
        ),
        (
            Action::SetDefaultEnvironment(environment.clone()),
            undone(Action::SetDefaultEnvironment(old)),
            text!("review-undo-default", environment = "zroot/ROOT/old"),
        ),
        (
            Action::SetDefaultKernel(environment.clone(), Untrusted::new("6.12".to_owned())),
            undone(Action::InheritDefaultKernel(environment.clone())),
            text!(
                "review-undo-inherit-kernel",
                environment = "zroot/ROOT/void"
            ),
        ),
        (
            Action::Import(pool.clone(), read_only),
            Review {
                host_id_stays: true,
                ..Review::default()
            },
            text!("review-host-id-stays"),
        ),
        (
            Action::Clone(new_snapshot(), environment),
            Review::default(),
            text!("dry-run-loses-nothing"),
        ),
    ];
    for (action, review, undo) in cases {
        let model = dry_run(action, plan(review));
        let said = words::dialog(&model).unwrap();
        assert!(said.lines.contains(&undo), "{undo}: {said:?}");
        assert!(!said.lines.contains(&text!("review-no-undo")), "{said:?}");
        assert!(prose(&model, 120, 40).contains(&undo), "{undo}");
    }
}

#[test]
fn a_review_page_that_loses_nothing_says_it_has_no_undo() {
    // The forced import of a pool this host has imported.
    let options = ImportOptions {
        access: Access::ReadOnly,
        policy: ImportPolicy::Force,
        rewind: false,
        id: None,
    };
    let mut model = rollback_page().into_model();
    let mut reviewing = Reviewing::new(Action::Import("tank".parse().unwrap(), options)).unwrap();
    reviewing.plan = Some(plan(Review::default()));
    model.dialog = Some(Dialog::Review(Box::new(reviewing)));

    let drawn = prose(&model, 120, 40);

    assert!(drawn.contains(&text!("review-no-undo")), "{drawn}");
    assert!(!drawn.contains(&text!("dry-run-loses-nothing")), "{drawn}");
}

/// Whether `action` has a plan, said of each kind of action, so that a new
/// kind is said too.
fn has_plan(action: &Action) -> bool {
    match action {
        Action::BootSnapshot(..)
        | Action::SetDefaultEnvironment(_)
        | Action::SetDefaultKernel(..)
        | Action::InheritDefaultKernel(_)
        | Action::SetCommandLine(..)
        | Action::InheritCommandLine(_)
        | Action::CreateSnapshot(_)
        | Action::Rollback(_)
        | Action::Destroy(_)
        | Action::Clone(..)
        | Action::CloneAndPromote(..)
        | Action::Duplicate(..)
        | Action::RestoreFiles { .. }
        | Action::Import(..)
        | Action::Export(_) => true,
        Action::Boot { .. }
        | Action::Diff(..)
        | Action::ResetBootTries(_)
        | Action::Reseal(_)
        | Action::Chroot(_)
        | Action::Unlock { .. }
        | Action::SkipUnlock(_)
        | Action::Answer(_)
        | Action::UnlockAll
        | Action::Open { .. }
        | Action::OpenWithKeyFile { .. }
        | Action::Close(_)
        | Action::Shell
        | Action::FirmwareSetup
        | Action::Reboot
        | Action::PowerOff
        | Action::Plan(_)
        | Action::ScanPools
        | Action::LoadDeviceLog(_)
        | Action::LoadSnapshots(_)
        | Action::LoadHistory => false,
    }
}

#[test]
fn every_action_with_a_plan_has_a_dry_run_title() {
    let snapshot = new_snapshot();
    let environment: zbm_types::DatasetName = "zroot/ROOT/void".parse().unwrap();
    let copy: zbm_types::DatasetName = "zroot/ROOT/copy".parse().unwrap();
    let pool: PoolName = "tank".parse().unwrap();
    let import = |policy, rewind| {
        let options = ImportOptions {
            access: Access::ReadWrite,
            policy,
            rewind,
            id: None,
        };
        Action::Import(pool.clone(), options)
    };
    let actions = [
        Action::BootSnapshot(snapshot.clone(), copy.clone()),
        Action::SetDefaultEnvironment(environment.clone()),
        Action::SetDefaultKernel(environment.clone(), Untrusted::new("6.12".to_owned())),
        Action::InheritDefaultKernel(environment.clone()),
        Action::SetCommandLine(environment.clone(), Untrusted::new("quiet".to_owned())),
        Action::InheritCommandLine(environment.clone()),
        Action::CreateSnapshot(snapshot.clone()),
        Action::Rollback(snapshot.clone()),
        Action::Destroy(vec![snapshot.clone()]),
        Action::Clone(snapshot.clone(), copy.clone()),
        Action::CloneAndPromote(snapshot.clone(), copy.clone()),
        Action::Duplicate(snapshot.clone(), copy),
        Action::RestoreFiles {
            snapshot,
            paths: vec![Untrusted::new("/etc/fstab".to_owned())],
        },
        import(ImportPolicy::Strict, false),
        import(ImportPolicy::Hostid, false),
        import(ImportPolicy::Force, false),
        import(ImportPolicy::Strict, true),
        Action::Export(pool.clone()),
    ];
    let generic = text!("review-title");
    let mut titles = std::collections::BTreeSet::new();
    for action in actions {
        assert!(has_plan(&action) && action.has_plan(), "{action:?}");
        let title = super::super::title(&Reviewing::dry_run(action.clone()));
        assert_ne!(title, generic, "{action:?}");
        // The effect is named for each, with its target.
        assert!(titles.insert(title.clone()), "{title}");
        assert!(
            title.contains("tank") || title.contains("zroot/ROOT/"),
            "{title}"
        );
    }
    assert!(!has_plan(&Action::Reboot));
    assert_eq!(
        super::super::title(&Reviewing::dry_run(Action::Reboot)),
        generic
    );
}

#[test]
fn the_line_view_says_the_steps_and_asks_for_no_name() {
    let steps = vec![crate::menu::world::Step {
        kind: crate::menu::world::StepKind::Export("tank".parse().unwrap()),
        command: Some("zpool export tank".to_owned()),
    }];
    let plan = Plan {
        steps,
        outcome: Outcome::Done(String::new()),
        review: Review::default(),
    };
    let model = dry_run(Action::Export("tank".parse().unwrap()), plan);

    let shown = crate::menu::lines::page(&model);

    assert!(
        shown.contains(&format!("  {}", text!("dry-run-asks"))),
        "{shown:#?}"
    );
    assert!(
        shown.iter().any(|line| line.contains("zpool export tank")),
        "{shown:#?}"
    );
    assert!(!shown.contains(&text!("lines-review")), "{shown:#?}");
    assert!(
        !shown
            .iter()
            .any(|line| line.contains(&text!("confirm-type", name = "")))
    );
}

#[test]
fn a_saved_line_s_dry_run_says_the_line_it_replaces_and_what_still_comes_first() {
    let name = |text: &str| -> zbm_types::DatasetName { text.parse().unwrap() };
    let line = |text: &str| Untrusted::new(text.to_owned());
    let undone_by = |undo: Action| Review {
        undo: Some(Box::new(undo)),
        ..Review::default()
    };
    // `tank/ROOT/arch` sets its own line; `tank/ROOT/debian` none; and
    // `zroot/ROOT/void` inherits one, shown expanded.
    let set_back = Action::SetCommandLine(name("tank/ROOT/arch"), line("rw quiet splash"));
    let pages = [
        (
            Action::SetCommandLine(name("tank/ROOT/arch"), line("rw")),
            undone_by(set_back),
            ("rw quiet splash", text!("origin-local")),
            text!(
                "review-undo-command-line",
                environment = "tank/ROOT/arch",
                line = "rw quiet splash"
            ),
        ),
        (
            Action::SetCommandLine(name("tank/ROOT/debian"), line("quiet")),
            undone_by(Action::InheritCommandLine(name("tank/ROOT/debian"))),
            ("(empty)", text!("label-default")),
            text!(
                "review-undo-inherit-command-line",
                environment = "tank/ROOT/debian"
            ),
        ),
        (
            Action::InheritCommandLine(name("zroot/ROOT/void")),
            undone_by(Action::InheritCommandLine(name("zroot/ROOT/void"))),
            (
                "quiet loglevel=4",
                text!("origin-inherited", from = "zroot/ROOT"),
            ),
            text!(
                "review-undo-inherit-command-line",
                environment = "zroot/ROOT/void"
            ),
        ),
    ];
    for (action, review, (old, origin), undo) in pages {
        let drawn = prose(&dry_run(action, plan(review)), 160, 40);

        let was = text!("dry-run-command-line-was", line = old, origin = origin);
        assert!(drawn.contains(&was), "{was}: {drawn}");
        assert!(drawn.contains(&undo), "{undo}: {drawn}");
        assert!(drawn.contains(&text!("dry-run-command-line-kernels")));
        assert!(drawn.contains(&text!("dry-run-command-line-measured")));
    }
    // Another change says none of it.
    let take = Action::CreateSnapshot(new_snapshot());
    let drawn = prose(&dry_run(take, plan(Review::default())), 160, 40);
    assert!(!drawn.contains(&text!("dry-run-command-line-measured")));
}
