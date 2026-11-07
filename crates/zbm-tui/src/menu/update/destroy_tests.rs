//! Tests of the destroy of snapshots, the workspace's alone (ADR 0016,
//! point 14): `Delete` on the Snapshots tab destroys the marked snapshots,
//! the oldest first, or else the selected one, after a review page that
//! asks for the selected one's short name, or for the environment's last
//! component when several are marked; a clone or a hold stops it
//! and nothing is sent; `Esc` sends nothing; the demo destroys the rows.
//! The `Enter` sheet and the palette destroy too, under the workspace
//! alone; the classic map never offers it (point 10).

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::{Message, palette_items, update};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::keymap::{self, Context, Input, Intent, Map};
use crate::menu::model::{Dialog, Model, Reviewing, Then};
use crate::menu::theme::Theme;
use crate::menu::world::{Action, Outcome};
use crate::text::text;

/// The demo, started, its countdown stopped, on the snapshots of the
/// default environment, `down` below the newest: 3 is the daily snapshot
/// of 20 October, 11 that of 18 October, which has a clone; in the
/// workspace when `workspace`.
fn tab(workspace: bool, down: usize) -> Session {
    let options = DemoOptions {
        classic: !workspace,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    session.key(Key::Char(if workspace { '2' } else { 's' }));
    for _ in 0..down {
        session.key(Key::Down);
    }
    session
}

/// The daily snapshot of `day` October.
fn daily(day: u32) -> zbm_types::SnapshotName {
    let name = format!("zroot/ROOT/void@autosnap_2026-10-{day}_daily");
    name.parse().unwrap()
}

/// The page over `model`.
fn page(model: &Model) -> &Reviewing {
    match &model.dialog {
        Some(Dialog::Review(reviewing)) => reviewing,
        other => panic!("no review page: {other:?}"),
    }
}

/// Whether the column of `model` lists `name`.
fn lists(model: &Model, name: &zbm_types::SnapshotName) -> bool {
    model.focus_snapshots().iter().any(|s| s.name == *name)
}

#[test]
fn delete_is_bound_on_the_workspaces_snapshots_alone() {
    let delete = KeyPress::new(Key::Delete);
    let destroy = Some(Input::Intent(Intent::Destroy));

    assert_eq!(
        keymap::input(Map::Workspace, Context::Snapshots, delete),
        destroy
    );
    for (map, context) in [
        (Map::Classic, Context::Snapshots),
        (Map::Workspace, Context::Main),
        (Map::Workspace, Context::Pools),
        (Map::Workspace, Context::Startup),
    ] {
        assert_ne!(
            keymap::input(map, context, delete),
            destroy,
            "{map:?} {context:?}"
        );
    }
    // Not in the toolbar; help lists it.
    let offered = |_| true;
    let said = |intent| keymap::describe(Context::Snapshots, intent);
    let tools = keymap::footer(Map::Workspace, Context::Snapshots, 200, &offered, &said);
    assert!(tools.iter().all(|tool| tool.key != text!("key-delete")));
    let help = keymap::help(Map::Workspace, Context::Snapshots, &offered, &said);
    assert!(help.contains(&(text!("key-delete"), crate::text::id!("action-destroy"))));
}

#[test]
fn the_selected_snapshot_is_destroyed_once_its_short_name_is_typed() {
    let mut session = tab(true, 3);
    session.key(Key::Delete);
    let reviewing = page(session.model());
    assert_eq!(reviewing.action, Action::Destroy(vec![daily(20)]));
    // The snapshot's short name, not the environment's, which confirms
    // any change of it.
    assert_eq!(reviewing.name, "autosnap_2026-10-20_daily");
    assert!(!reviewing.is_refused());
    assert_eq!(session.model().context(), Context::Review);

    session.type_text("void");
    session.key(Key::Enter);
    assert_eq!(session.actions, []);
    session.press(keymap::press(Key::Char('u'), keymap::Modifiers::CTRL));
    session.type_text("autosnap_2026-10-20_daily");
    session.key(Key::Enter);

    assert_eq!(session.actions, [Action::Destroy(vec![daily(20)])]);
    assert!(session.model().dialog.is_none());
    // As the machine says it.
    let Some(Outcome::Done(said)) = &session.model().notification else {
        panic!("{:?}", session.model().notification);
    };
    assert!(
        said.starts_with("zroot/ROOT/void@autosnap_2026-10-20_daily destroyed"),
        "{said}"
    );
    // The demo destroyed it: its row is gone.
    assert!(!lists(session.model(), &daily(20)));
    assert!(lists(session.model(), &daily(21)));
}

#[test]
fn the_marked_snapshots_are_destroyed_the_oldest_first() {
    let mut session = tab(true, 3);
    // 20, 22 then 21 October.
    session.key(Key::Char(' '));
    session.key(Key::Up);
    session.key(Key::Up);
    session.key(Key::Char(' '));
    session.key(Key::Down);
    session.key(Key::Char(' '));
    session.key(Key::Delete);
    let marked = vec![daily(20), daily(21), daily(22)];
    assert_eq!(
        page(session.model()).action,
        Action::Destroy(marked.clone())
    );
    assert_eq!(page(session.model()).name, "void");

    session.type_text("void");
    session.key(Key::Enter);

    assert_eq!(session.actions, [Action::Destroy(marked.clone())]);
    let model = session.model();
    assert!(
        marked.iter().all(|name| !lists(model, name)),
        "{:?}",
        model.marked
    );
    // The list read again takes their marks.
    assert!(model.marked.is_empty(), "{:?}", model.marked);
}

#[test]
fn what_stops_a_destroy_is_named_and_nothing_is_sent() {
    let mut session = tab(true, 11);
    session.key(Key::Delete);
    let reviewing = page(session.model());
    assert_eq!(reviewing.action, Action::Destroy(vec![daily(18)]));
    assert!(reviewing.is_refused());
    assert_eq!(session.model().context(), Context::Choose);

    let mut model = session.into_model();
    if let Some(Dialog::Review(reviewing)) = &mut model.dialog {
        reviewing.typed = Some(reviewing.name.clone());
    }
    let (model, actions) = update(model, Message::Key(KeyPress::new(Key::Enter)));
    assert_eq!(actions, []);
    assert!(model.dialog.is_none());
    assert!(lists(&model, &daily(18)));
}

#[test]
fn escape_closes_the_destroy_page_and_sends_nothing() {
    let mut session = tab(true, 3);
    session.key(Key::Char(' '));
    session.key(Key::Delete);
    session.type_text("void");
    session.key(Key::Escape);

    assert!(session.model().dialog.is_none());
    assert_eq!(session.actions, []);
    assert!(lists(session.model(), &daily(20)));
}

#[test]
fn a_folded_group_without_marks_destroys_nothing_and_says_to_open_it() {
    let mut session = tab(true, 0);
    // The oldest month, folded.
    session.key(Key::End);
    assert!(session.model().folded_group().is_some());
    session.key(Key::Delete);

    assert!(session.model().dialog.is_none());
    let said = text!("timeline-open-group");
    assert_eq!(session.model().notification, Some(Outcome::Warning(said)));
    assert_eq!(session.actions, []);
}

#[test]
fn the_workspaces_sheet_and_palette_destroy_and_the_classic_ones_do_not() {
    let destroys = |then: &Then| matches!(then, Then::Review(Action::Destroy(_)));
    for workspace in [true, false] {
        let mut session = tab(workspace, 3);
        session.key(Key::Char(' '));
        let palette: Vec<_> = palette_items(session.model())
            .into_iter()
            .filter(|item| destroys(&item.then))
            .map(|item| item.label)
            .collect();
        session.key(Key::Enter);
        let Some(Dialog::Choose { choices, .. }) = &session.model().dialog else {
            panic!("no sheet: {:?}", session.model().dialog);
        };
        let sheet: Vec<&str> = choices
            .iter()
            .filter(|choice| destroys(&choice.then))
            .map(|choice| choice.label.as_str())
            .collect();

        if workspace {
            let selected = daily(20).to_string();
            let one = text!("palette-destroy", snapshot = selected.as_str());
            let marked = text!("palette-destroy-marked", count = 1);
            assert_eq!(palette, [one, marked]);
            assert_eq!(sheet, [text!("snapshot-destroy")]);
        } else {
            assert_eq!(palette, Vec::<String>::new());
            assert_eq!(sheet, Vec::<&str>::new());
        }
    }
}

#[test]
fn the_palette_destroys_nothing_away_from_the_snapshots_view() {
    let destroys = |session: &Session| {
        palette_items(session.model())
            .iter()
            .filter(|item| matches!(item.then, Then::Review(Action::Destroy(_))))
            .count()
    };
    let mut session = tab(true, 3);
    session.key(Key::Char(' '));
    session.key(Key::Down);
    session.key(Key::Char(' '));
    assert_eq!(destroys(&session), 2);

    // The selection and the marks stay, but are not shown.
    for tab in ['3', '4', '1'] {
        session.key(Key::Char(tab));
        assert_eq!(destroys(&session), 0, "tab {tab}");
    }
}

#[test]
fn the_palette_destroys_two_entries_at_most_whatever_the_snapshots() {
    let options = DemoOptions {
        classic: false,
        snapshots: 10_000,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    session.key(Key::Char('2'));
    session.key(Key::Home);
    session.key(Key::Down);
    session.key(Key::Char(' '));
    session.key(Key::Down);
    session.key(Key::Char(' '));

    let items = palette_items(session.model());
    let destroys = items
        .iter()
        .filter(|item| matches!(item.then, Then::Review(Action::Destroy(_))))
        .count();
    assert_eq!(
        destroys, 2,
        "the selection and the marks, not one a snapshot"
    );
}
