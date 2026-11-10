//! Tests of the dry run of the command line (ADR 0007, point 5; ADR 0016,
//! point 12), the workspace's: a change chosen there sends nothing but its
//! plan, and runs once `Enter` is pressed on its page; `Esc` sends
//! nothing; a named make, and a promote with its typed confirmation, show
//! their plan once named; a rollback opens its review page once, while the
//! dry run holds; what has
//! no plan, and a key of a view, act as before; the classic map has no dry
//! run; closing a dry run while the machine plans cancels the plan; a dry
//! run its plan refuses only closes; a dialog the machine opens over one
//! the command line opened ends the dry run, however it closes.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::review_tests::Planner;
use super::{Message, update};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::keymap::{self, Context, Intent, Map};
use crate::menu::model::{Dialog, DryRun, Make, Model, Reviewing, Then};
use crate::menu::theme::Theme;
use crate::menu::world::{Action, Event, Outcome, Plan, Question, Review};
use crate::text::id;

fn theme() -> Theme {
    Theme::new(Capabilities::FULL)
}

/// The demo under `options`, started, its countdown stopped.
fn started(options: &DemoOptions) -> Session {
    let mut session = Session::new(options, theme());
    session.start();
    session.key(Key::Escape);
    session
}

/// The demo under the workspace, started, on the Snapshots tab, `down`
/// snapshots below the newest: 3 is the daily snapshot of 20 October.
fn snapshots(down: usize) -> Session {
    let mut session = started(&DemoOptions::default());
    session.key(Key::Char('2'));
    for _ in 0..down {
        session.key(Key::Down);
    }
    session
}

/// `model` after `key`, and what it asks for.
fn press(model: Model, key: Key) -> (Model, Vec<Action>) {
    update(model, Message::Key(KeyPress::new(key)))
}

/// `model` after its field is cleared and `text` typed, each key's
/// actions dropped.
fn typed(model: Model, text: &str) -> Model {
    let clear = KeyPress::new(Key::Char('u')).with_ctrl();
    let (model, _) = update(model, Message::Key(clear));
    text.chars()
        .fold(model, |model, c| press(model, Key::Char(c)).0)
}

/// `model` after the key `c` of a view, as the command line's entry of it
/// would press it: those of a snapshot that its `Enter` reaches have none
/// yet, as `ENTER_REACHES` leaves them out.
fn entered(mut model: Model, c: char) -> (Model, Vec<Action>) {
    model.dry_run = DryRun::Pending;
    press(model, Key::Char(c))
}

/// `model` after `:` and `Enter` on the first entry of the command line
/// that `is_it` finds, and what it asks for.
fn choose(model: Model, is_it: impl Fn(&Then) -> bool) -> (Model, Vec<Action>) {
    let (mut model, _) = press(model, Key::Char(':'));
    let palette = model.palette.as_mut().expect("the command line opens");
    let index = palette.items.iter().position(|item| is_it(&item.then));
    let index = index.expect("the command line has the entry");
    palette.selected = palette
        .matches
        .iter()
        .position(|ranked| ranked.index == index)
        .unwrap();
    press(model, Key::Enter)
}

/// Whether `then` asks for the name of a new snapshot.
fn is_new_snapshot(then: &Then) -> bool {
    matches!(then, Then::Name(naming) if matches!(naming.make, Make::Snapshot(_)))
}

/// Whether `then` exports a pool.
fn is_export(then: &Then) -> bool {
    matches!(then, Then::Act(Action::Export(_)))
}

/// The page over `model`.
fn page(model: &Model) -> &Reviewing {
    match &model.dialog {
        Some(Dialog::Review(reviewing)) => reviewing,
        other => panic!("no review page: {other:?}"),
    }
}

/// The plan of what the one `actions` asks for plans, and the action.
fn planned(actions: &[Action]) -> Action {
    match actions {
        [Action::Plan(action)] => (**action).clone(),
        other => panic!("not one plan: {other:?}"),
    }
}

#[test]
fn a_change_chosen_on_the_command_line_sends_nothing_but_its_plan() {
    let session = started(&DemoOptions::default());

    let (model, actions) = choose(session.into_model(), is_export);

    let export = planned(&actions);
    assert!(matches!(export, Action::Export(_)), "{export:?}");
    let reviewing = page(&model);
    assert_eq!(reviewing.action, export);
    assert_eq!(
        (reviewing.typed.as_deref(), reviewing.plan.as_ref()),
        (None, None)
    );
    assert_eq!(model.dry_run, DryRun::Off);
    // No field: letters do not type, and `Enter` runs it.
    assert_eq!(model.context(), Context::Choose);
    let label = keymap::label(Context::Choose, Intent::Primary, &model);
    assert_eq!(label, id!("action-run"));
    // Before the plan, `Enter` sends nothing, and says to wait.
    let (model, actions) = press(model, Key::Enter);
    assert_eq!(actions, []);
    let wait = Outcome::Warning(crate::text::text!("review-wait"));
    assert_eq!(model.notification, Some(wait));
    assert!(model.dialog.is_some());
}

#[test]
fn enter_on_a_planned_dry_run_runs_the_change_once() {
    let mut session = started(&DemoOptions::default());
    session.key(Key::Char(':'));
    session.type_text("export");
    session.key(Key::Enter);
    let reviewing = page(session.model());
    assert!(reviewing.plan.is_some(), "the demo plans at once");
    assert!(reviewing.is_ready());
    // The plan only reads: nothing is recorded as sent.
    assert_eq!(session.actions, []);
    let export = reviewing.action.clone();

    session.key(Key::Enter);

    assert_eq!(session.actions, [export]);
    assert!(session.model().dialog.is_none());
}

#[test]
fn escape_on_a_dry_run_sends_nothing() {
    let mut session = started(&DemoOptions::default());
    session.key(Key::Char(':'));
    session.type_text("export");
    session.key(Key::Enter);
    assert!(page(session.model()).plan.is_some());

    session.key(Key::Escape);

    assert!(session.model().dialog.is_none());
    assert_eq!(session.actions, []);
}

#[test]
fn a_named_make_shows_its_plan_after_its_name() {
    let session = snapshots(3);

    let (model, actions) = choose(session.into_model(), is_new_snapshot);
    assert_eq!(actions, []);
    assert!(matches!(model.dialog, Some(Dialog::Name(_))));
    assert_eq!(model.dry_run, DryRun::Pending);
    let model = typed(model, "before");
    let (model, actions) = press(model, Key::Enter);

    let made = planned(&actions);
    assert!(
        matches!(&made, Action::CreateSnapshot(name) if name.name() == "before"),
        "{made:?}"
    );
    assert_eq!(page(&model).action, made);
    assert_eq!(model.dry_run, DryRun::Off);
}

#[test]
fn a_promote_while_the_dry_run_holds_shows_its_plan_after_its_typed_confirmation() {
    let session = snapshots(3);

    let (model, _) = entered(session.into_model(), 'm');
    let model = typed(model, "new");
    let (model, actions) = press(model, Key::Enter);
    assert_eq!(actions, []);
    assert!(matches!(model.dialog, Some(Dialog::Confirm { .. })));
    let model = typed(model, "void");
    let (model, actions) = press(model, Key::Enter);

    let promote = planned(&actions);
    assert!(
        matches!(promote, Action::CloneAndPromote(..)),
        "{promote:?}"
    );
    assert_eq!(page(&model).action, promote);
    assert_eq!(page(&model).typed, None);
}

#[test]
fn a_rollback_while_the_dry_run_holds_opens_its_review_page_once() {
    let session = snapshots(3);

    let (model, actions) = entered(session.into_model(), 'r');

    let rollback = planned(&actions);
    assert!(matches!(rollback, Action::Rollback(_)), "{rollback:?}");
    // Its review page, with the field: the dry run already.
    assert_eq!(page(&model).typed.as_deref(), Some(""));
    assert_eq!(model.dry_run, DryRun::Off);
    let plan = Plan {
        steps: Vec::new(),
        outcome: Outcome::Done(String::new()),
        review: Review::default(),
    };
    let event = Event::Planned {
        action: Box::new(rollback.clone()),
        plan,
    };
    let (model, _) = update(model, Message::Event(Box::new(event)));
    let model = typed(model, "void");
    let (model, actions) = press(model, Key::Enter);
    assert_eq!(actions, [rollback]);
    assert!(model.dialog.is_none());
}

#[test]
fn what_has_no_plan_acts_as_before() {
    let session = started(&DemoOptions::default());

    let (model, actions) = choose(session.into_model(), |then| {
        *then == Then::Act(Action::ScanPools)
    });

    assert_eq!(actions, [Action::ScanPools]);
    assert!(model.dialog.is_none());
    assert_eq!(model.dry_run, DryRun::Off);
}

#[test]
fn a_key_of_a_view_acts_without_a_dry_run() {
    let mut session = started(&DemoOptions::default());
    session.key(Key::Char('2'));
    session.key(Key::Char('n'));
    session.type_text("before");
    session.key(Key::Enter);

    assert!(session.model().dialog.is_none());
    assert!(
        matches!(session.actions.as_slice(), [Action::CreateSnapshot(_)]),
        "{:?}",
        session.actions
    );
}

#[test]
fn the_classic_map_shows_no_dry_run() {
    let options = DemoOptions {
        classic: true,
        ..DemoOptions::default()
    };
    let session = started(&options);
    assert_eq!(session.model().map, Map::Classic);

    let (model, actions) = choose(session.into_model(), is_export);

    assert!(
        matches!(actions.as_slice(), [Action::Export(_)]),
        "{actions:?}"
    );
    assert!(model.dialog.is_none());
    assert_eq!(model.dry_run, DryRun::Off);
}

#[test]
fn closing_a_dry_run_while_it_plans_cancels_the_plan() {
    let (machine, cancelled) = Planner::new(|_| false);
    let mut session = Session::on(machine, theme(), Map::Workspace);
    session.start();
    session.key(Key::Escape);
    session.key(Key::Char(':'));
    session.type_text("export");
    session.key(Key::Enter);
    assert!(page(session.model()).plan.is_none());
    assert!(session.model().working.is_some());

    session.key(Key::Escape);

    assert!(cancelled.get());
    assert!(session.model().dialog.is_none());
    assert_eq!(session.actions, []);
}

#[test]
fn a_dry_run_refused_by_its_plan_only_closes() {
    let session = started(&DemoOptions::default());
    let (model, actions) = choose(session.into_model(), is_export);
    let plan = Plan {
        steps: Vec::new(),
        outcome: Outcome::Failed {
            what: "unable to export".to_owned(),
            why: vec!["the pool is busy".to_owned()],
            next: Vec::new(),
        },
        review: Review::default(),
    };
    let event = Event::Planned {
        action: Box::new(planned(&actions)),
        plan,
    };
    let (model, _) = update(model, Message::Event(Box::new(event)));
    assert!(page(&model).is_refused());
    let label = keymap::label(Context::Choose, Intent::Primary, &model);
    assert_eq!(label, id!("action-close"));

    let (model, actions) = press(model, Key::Enter);

    assert_eq!(actions, []);
    assert!(model.dialog.is_none());
}

/// `model` after the machine's `event`, and what it asks for.
fn machine(model: Model, event: Event) -> (Model, Vec<Action>) {
    update(model, Message::Event(Box::new(event)))
}

/// The Name dialog of a snapshot, which `:` and "New snapshot" open, the
/// dry run pending.
fn naming() -> Model {
    let (model, _) = choose(snapshots(3).into_model(), is_new_snapshot);
    assert!(matches!(model.dialog, Some(Dialog::Name(_))));
    assert_eq!(model.dry_run, DryRun::Pending);
    model
}

/// Whether, after `model`, a snapshot named with `n` on the Snapshots tab
/// is taken at once, as a key of a view takes it.
fn takes_at_once(model: Model) -> bool {
    let (model, _) = press(model, Key::Char('n'));
    let model = typed(model, "before");
    let (model, actions) = press(model, Key::Enter);
    model.dialog.is_none() && matches!(actions.as_slice(), [Action::CreateSnapshot(_)])
}

#[test]
fn a_question_the_machine_withdraws_ends_the_dry_run() {
    let asked = Event::Asked {
        question: Question::MissingDevices,
        seconds: None,
    };
    let (model, _) = machine(naming(), asked);
    let (model, _) = machine(model, Event::Withdrawn);

    assert!(model.dialog.is_none());
    assert_eq!(model.dry_run, DryRun::Off);
    assert!(takes_at_once(model));
}

#[test]
fn a_question_whose_time_runs_out_ends_the_dry_run() {
    let asked = Event::Asked {
        question: Question::MissingDevices,
        seconds: Some(1),
    };
    let (model, _) = machine(naming(), asked);
    let (model, actions) = update(model, Message::Tick(1000));
    assert!(
        matches!(actions.as_slice(), [Action::Answer(_)]),
        "{actions:?}"
    );

    assert!(model.dialog.is_none());
    assert_eq!(model.dry_run, DryRun::Off);
    assert!(takes_at_once(model));
}

#[test]
fn the_choices_of_a_pool_that_did_not_import_show_no_dry_run() {
    let problem = Event::ImportProblem {
        pool: "backup".parse().unwrap(),
        reason: "foreign".to_owned(),
    };
    let (model, _) = machine(naming(), problem);
    assert_eq!(model.dry_run, DryRun::Off);
    // "Import with Its Last Host ID", the second choice.
    let (model, _) = press(model, Key::Down);

    let (model, actions) = press(model, Key::Enter);

    assert!(
        matches!(actions.as_slice(), [Action::Import(..)]),
        "{actions:?}"
    );
    assert!(model.dialog.is_none());
}
