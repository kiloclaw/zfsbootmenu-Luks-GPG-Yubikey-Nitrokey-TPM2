//! Tests of the review page of a change (ADR 0016, points 12 to 15), the
//! workspace's: it opens and asks the machine for the plan; it asks for
//! the change only once the plan came, nothing stops it and the name is
//! typed as it is; what stops it is named and nothing is sent; `Esc`
//! sends nothing; a plan of another change, or one that comes after the
//! page closed, is dropped; a change of the machine plans it again;
//! closing the page while the machine plans, and only plans, cancels the
//! plan; the safer action names a clone or imports read-only; the classic
//! map keeps its confirmation; the line view and `Announce` say what the
//! page asks. The page as drawn is tested in `view/review/tests.rs`.

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};
use zbm_types::ImportPolicy;

use super::{Message, update};
use crate::menu::announce::Announcer;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine, Session};
use crate::menu::keymap::{Context, Map};
use crate::menu::lines::{self, Typed};
use crate::menu::machine::Machine;
use crate::menu::model::{Dialog, Make, Model, Reviewing};
use crate::menu::theme::Theme;
use crate::menu::view::words;
use crate::menu::world::{
    Access, Action, Event, ImportOptions, Loss, Outcome, Plan, Review, Snapshot,
};
use crate::text::text;

fn theme() -> Theme {
    Theme::new(Capabilities::FULL)
}

/// The demo under the workspace, started, its countdown stopped, on the
/// Snapshots tab, `down` snapshots below the newest: 3 is the daily
/// snapshot of 20 October, 10 that of 17 October, older than the one
/// with a clone and the one with a hold.
fn snapshots(down: usize) -> Session {
    let mut session = Session::new(&DemoOptions::default(), theme());
    session.start();
    session.key(Key::Escape);
    session.key(Key::Char('2'));
    for _ in 0..down {
        session.key(Key::Down);
    }
    session
}

/// The page over `model`.
fn page(model: &Model) -> &Reviewing {
    match &model.dialog {
        Some(Dialog::Review(reviewing)) => reviewing,
        other => panic!("no review page: {other:?}"),
    }
}

/// `model` after `key`, and what it asks for.
fn press(model: Model, key: Key) -> (Model, Vec<Action>) {
    update(model, Message::Key(KeyPress::new(key)))
}

/// `model` after `event`, and what it asks for.
fn event(model: Model, event: Event) -> (Model, Vec<Action>) {
    update(model, Message::Event(Box::new(event)))
}

/// The daily snapshot of `day` October.
fn daily(day: u32) -> zbm_types::SnapshotName {
    let name = format!("zroot/ROOT/void@autosnap_2026-10-{day}_daily");
    name.parse().unwrap()
}

/// The rollback to the daily snapshot of `day` October.
fn rollback(day: u32) -> Action {
    Action::Rollback(daily(day))
}

#[test]
fn a_rollback_is_sent_only_once_its_environments_name_is_typed() {
    let mut session = snapshots(3);
    session.key(Key::Char('r'));
    let reviewing = page(session.model());
    assert_eq!(reviewing.action, rollback(20));
    assert!(reviewing.plan.is_some(), "the demo plans at once");
    // The last component of the environment, as the classic map asks.
    assert_eq!(reviewing.name, "void");
    assert_eq!(session.model().context(), Context::Review);

    session.key(Key::Enter);
    assert_eq!(session.actions, []);
    let first = text!("confirm-type-first", name = "void");
    assert_eq!(session.model().notification, Some(Outcome::Warning(first)));
    // Letters type, the map's keys among them; case counts.
    session.type_text("VOID");
    session.key(Key::Enter);
    assert_eq!(session.actions, []);
    session.press(KeyPress::new(Key::Char('u')).with_ctrl());
    session.type_text("voidx");
    session.key(Key::Backspace);
    session.key(Key::Enter);

    assert_eq!(session.actions, [rollback(20)]);
    assert!(session.model().dialog.is_none());
}

#[test]
fn what_stops_a_rollback_is_named_and_nothing_is_sent() {
    let mut session = snapshots(10);
    session.key(Key::Char('r'));
    let reviewing = page(session.model());
    assert_eq!(reviewing.action, rollback(17));
    assert!(reviewing.is_refused());
    // No field: letters do not type, `Enter` closes.
    assert_eq!(session.model().context(), Context::Choose);
    let said = words::dialog(session.model()).unwrap();
    assert_eq!(said.lines[0], text!("review-refused"));
    assert!(said.lines[1].contains("zroot/ROOT/void-test"), "{said:?}");
    assert!(said.lines[2].contains("backup-keep"), "{said:?}");

    // Even with the name typed, the change is not sent.
    let mut model = session.into_model();
    if let Some(Dialog::Review(reviewing)) = &mut model.dialog {
        reviewing.typed = Some(reviewing.name.clone());
    }
    let (model, actions) = press(model, Key::Down);
    assert_eq!(page(&model).scroll, 1);
    let (model, actions_too) = press(model, Key::Enter);
    assert_eq!((actions, actions_too), (Vec::new(), Vec::new()));
    assert!(model.dialog.is_none());
}

#[test]
fn escape_closes_the_page_and_sends_nothing() {
    let mut session = snapshots(3);
    session.key(Key::Char('r'));
    session.type_text("void");
    session.key(Key::Escape);

    assert!(session.model().dialog.is_none());
    assert_eq!(session.actions, []);
}

#[test]
fn the_classic_map_keeps_its_confirmation_and_asks_for_no_plan() {
    let options = DemoOptions {
        classic: true,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, theme());
    session.start();
    session.key(Key::Escape);
    session.key(Key::Char('s'));
    let (model, actions) = press(session.into_model(), Key::Char('r'));

    assert_eq!(actions, []);
    assert!(matches!(model.dialog, Some(Dialog::Confirm { .. })));
}

#[test]
fn only_the_plan_of_the_change_reviewed_fills_the_page() {
    let session = snapshots(3);
    let (model, actions) = press(session.into_model(), Key::Char('r'));
    assert_eq!(actions, [Action::Plan(Box::new(rollback(20)))]);
    assert!(page(&model).plan.is_none());
    let said = words::dialog(&model).unwrap();
    assert_eq!(said.lines, [text!("review-planning")]);
    // Typed ahead of the plan, the name sends nothing until it comes.
    let model = "void"
        .chars()
        .fold(model, |model, c| press(model, Key::Char(c)).0);
    let (model, actions) = press(model, Key::Enter);
    assert_eq!(actions, []);
    let wait = Outcome::Warning(text!("review-wait"));
    assert_eq!(model.notification, Some(wait));

    let planned = |action: Action| Event::Planned {
        action: Box::new(action),
        plan: Plan {
            steps: Vec::new(),
            outcome: Outcome::Done(String::new()),
            review: Review::default(),
        },
    };
    let (model, _) = event(model, planned(rollback(19)));
    assert!(page(&model).plan.is_none());
    let (model, _) = event(model, planned(rollback(20)));
    assert!(page(&model).plan.is_some());
    let (model, actions) = press(model, Key::Enter);
    assert_eq!(actions, [rollback(20)]);
    // A plan after the page closed opens nothing.
    let (model, _) = event(model, planned(rollback(20)));
    assert!(model.dialog.is_none());
}

#[test]
fn a_change_of_the_machine_plans_the_page_again_once() {
    let session = snapshots(3);
    let snapshot = session.model().snapshot.clone();
    let (model, _) = press(session.into_model(), Key::Char('r'));
    let plan = Plan {
        steps: Vec::new(),
        outcome: Outcome::Done(String::new()),
        review: Review::default(),
    };
    let planned = Event::Planned {
        action: Box::new(rollback(20)),
        plan,
    };
    let (model, _) = event(model, planned);

    let (model, actions) = event(model, Event::Changed(Box::new(snapshot.clone())));
    assert_eq!(actions, [Action::Plan(Box::new(rollback(20)))]);
    assert!(page(&model).plan.is_none());
    let (_, actions) = event(model, Event::Changed(Box::new(snapshot)));
    assert_eq!(actions, []);
}

/// The demo machine, whose plans, and the actions `holds` names, take
/// until the next tick, one by one, as a machine that reads its pools for
/// seconds; a cancel ends every action not done, as the service's does,
/// and it notes it.
struct Planner {
    inner: FakeMachine,
    holds: fn(&Action) -> bool,
    waiting: VecDeque<Action>,
    cancelled: Rc<Cell<bool>>,
}

impl Planner {
    /// The demo machine, its plans and what `holds` names held.
    fn new(holds: fn(&Action) -> bool) -> (Planner, Rc<Cell<bool>>) {
        let cancelled = Rc::new(Cell::new(false));
        let machine = Planner {
            inner: FakeMachine::new(&DemoOptions::default()),
            holds,
            waiting: VecDeque::new(),
            cancelled: Rc::clone(&cancelled),
        };
        (machine, cancelled)
    }
}

impl Machine for Planner {
    fn snapshot(&self) -> &Snapshot {
        self.inner.snapshot()
    }

    fn tick(&mut self) -> Vec<Event> {
        match self.waiting.pop_front() {
            Some(action) => self.inner.apply(action),
            None => self.inner.tick(),
        }
    }

    fn apply(&mut self, action: Action) -> Vec<Event> {
        if matches!(action, Action::Plan(_)) || (self.holds)(&action) {
            self.waiting.push_back(action);
            return Vec::new();
        }
        self.inner.apply(action)
    }

    fn is_busy(&self) -> bool {
        !self.waiting.is_empty()
    }

    fn cancel(&mut self) {
        self.waiting.clear();
        self.cancelled.set(true);
    }
}

#[test]
fn closing_the_page_while_the_machine_plans_cancels_the_plan() {
    let (machine, cancelled) = Planner::new(|_| false);
    let mut session = Session::on(machine, theme(), Map::Workspace);
    session.start();
    session.key(Key::Escape);
    session.key(Key::Char('2'));
    session.key(Key::Char('r'));
    assert!(page(session.model()).plan.is_none());
    assert!(session.model().working.is_some());

    session.key(Key::Escape);

    assert!(cancelled.get());
    assert!(session.model().dialog.is_none());
    assert_eq!(session.actions, []);
}

#[test]
fn closing_the_page_while_the_plan_waits_behind_another_action_cancels_nothing() {
    let (machine, cancelled) = Planner::new(|action| matches!(action, Action::Duplicate(..)));
    let mut session = Session::on(machine, theme(), Map::Workspace);
    session.start();
    session.key(Key::Escape);
    let copy = "zroot/ROOT/copy".parse().unwrap();
    session.perform(Action::Duplicate(daily(21), copy));
    session.key(Key::Char('2'));
    session.key(Key::Char('r'));
    assert!(page(session.model()).plan.is_none());

    session.key(Key::Escape);

    assert!(!cancelled.get());
    assert!(session.model().dialog.is_none());
    session.tick(0);
    session.tick(0);
    assert!(session.model().dialog.is_none());
    assert!(!session.model().is_starting_up());
}

#[test]
fn the_safer_action_of_a_rollback_names_a_clone_of_its_snapshot() {
    let mut session = snapshots(3);
    session.key(Key::Char('r'));
    session.key(Key::Tab);
    session.key(Key::Tab);
    session.key(Key::BackTab);
    session.key(Key::Enter);

    let Some(Dialog::Name(naming)) = &session.model().dialog else {
        panic!("{:?}", session.model().dialog);
    };
    let snapshot = daily(20);
    assert_eq!(naming.make, Make::Clone(snapshot));
    assert_eq!(session.actions, []);
}

/// The demo under the workspace with the pool of another host, started:
/// start-up asks what to do over it.
fn foreign() -> Session {
    let options = DemoOptions {
        foreign: true,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, theme());
    session.start();
    session
}

#[test]
fn a_forced_import_asks_for_the_pools_name_and_says_who_used_it() {
    let mut session = foreign();
    session.key(Key::Down);
    session.key(Key::Down);
    session.key(Key::Enter);
    let reviewing = page(session.model());
    assert_eq!(reviewing.name, "backup");
    let said = words::dialog(session.model()).unwrap();
    assert!(said.lines.iter().any(|line| line.contains("backup-server")));
    session.type_text("backup");
    session.key(Key::Enter);

    let forced = ImportOptions {
        access: Access::ReadOnly,
        policy: ImportPolicy::Force,
        rewind: false,
        id: None,
    };
    let import = Action::Import("backup".parse().unwrap(), forced);
    assert_eq!(session.actions.first(), Some(&import));
}

#[test]
fn the_safer_action_of_a_rewind_imports_the_pool_read_only() {
    let mut session = Session::new(&DemoOptions::default(), theme());
    session.start();
    session.key(Key::Escape);
    session.key(Key::Char('3'));
    for _ in 0..7 {
        session.key(Key::Down);
    }
    session.key(Key::Char('i'));
    session.key(Key::Down);
    session.key(Key::Enter);
    assert_eq!(page(session.model()).name, "tank");
    session.key(Key::Tab);
    session.key(Key::Enter);

    let read_only = ImportOptions {
        access: Access::ReadOnly,
        policy: ImportPolicy::Strict,
        rewind: false,
        id: None,
    };
    let import = Action::Import("tank".parse().unwrap(), read_only);
    assert_eq!(session.actions, [import]);
}

#[test]
fn the_line_view_types_the_name_and_says_what_the_page_asks() {
    let mut session = snapshots(3);
    session.key(Key::Char('r'));
    let shown = lines::page(session.model());
    let asks = text!("confirm-type", name = "void");
    assert!(shown.contains(&format!("  {asks}")), "{shown:#?}");
    assert_eq!(shown.last(), Some(&text!("lines-review")));

    let Typed::Keys(keys) = lines::typed("void\n", Context::Review, Map::Workspace) else {
        panic!("not keys");
    };
    for key in keys {
        session.press(key);
    }
    assert_eq!(session.actions, [rollback(20)]);
}

#[test]
fn announce_says_what_the_page_asks_when_its_plan_comes() {
    let session = snapshots(3);
    let (model, _) = press(session.into_model(), Key::Char('r'));
    let mut announcer = Announcer::default();
    let planning = announcer.next(&model);
    assert!(
        planning[0].ends_with(&text!("review-planning")),
        "{planning:?}"
    );

    let plan = Plan {
        steps: Vec::new(),
        outcome: Outcome::Done(String::new()),
        review: Review::default(),
    };
    let planned = Event::Planned {
        action: Box::new(rollback(20)),
        plan,
    };
    let (model, _) = event(model, planned);
    let asks = text!("confirm-type", name = "void");
    assert_eq!(announcer.next(&model).len(), 1);
    let (model, _) = press(model, Key::Char('a'));
    assert_eq!(announcer.next(&model), Vec::<String>::new());
    assert_eq!(words::dialog(&model).unwrap().lines[0], asks);
}

/// The page of `action` on the workspace's Snapshots tab, with `plan`.
fn reviewed(action: Action, plan: Plan) -> Model {
    let session = snapshots(0);
    let mut model = session.into_model();
    let mut reviewing = crate::menu::model::Reviewing::new(action).unwrap();
    reviewing.plan = Some(plan);
    model.dialog = Some(Dialog::Review(Box::new(reviewing)));
    model
}

#[test]
fn the_line_view_says_twenty_losses_then_how_many_more() {
    let losses = (0..25)
        .map(|day| Loss::Snapshot {
            name: format!("zroot/ROOT/void@lost-{day}").parse().unwrap(),
            used: None,
        })
        .collect();
    let plan = Plan {
        steps: Vec::new(),
        outcome: Outcome::Done(String::new()),
        review: Review {
            loses: losses,
            ..Review::default()
        },
    };
    let model = reviewed(rollback(20), plan);
    let said = words::dialog(&model).unwrap();
    assert!(said.lines.contains(&"lost-19".to_owned()));
    assert!(!said.lines.contains(&"lost-20".to_owned()));
    assert!(said.lines.contains(&text!("review-more", count = 5)));
    // The screen scrolls them, no further than the last.
    let model = (0..10).fold(model, |model, _| press(model, Key::PageDown).0);
    assert_eq!(page(&model).scroll, 24);
    let (model, _) = press(model, Key::PageUp);
    assert_eq!(page(&model).scroll, 19);
}

#[test]
fn a_step_that_would_fail_refuses_the_change_and_says_why() {
    let plan = Plan {
        steps: Vec::new(),
        outcome: Outcome::Failed {
            what: "unable to roll back".to_owned(),
            why: vec!["no kernel in the snapshot".to_owned()],
            next: Vec::new(),
        },
        review: Review::default(),
    };
    let model = reviewed(rollback(20), plan);

    assert_eq!(model.context(), Context::Choose);
    let said = words::dialog(&model).unwrap();
    assert_eq!(
        said.lines[..3],
        [
            text!("review-refused"),
            "unable to roll back".to_owned(),
            "no kernel in the snapshot".to_owned()
        ]
    );
}
