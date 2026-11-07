//! Tests of the marks on snapshots, a set in both maps: as many as the
//! person likes, none evicted; `f` diffs the selection, or the one marked,
//! against now, or the two marked, the older against the newer, in the
//! order ZFS took them whatever the menu's sort; three marks or more, or
//! marks whose list is not read, are no diff, which the status line says;
//! and a list read again takes the marks of the snapshots it no longer has.

use std::collections::BTreeSet;

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};
use zbm_types::SnapshotName;

use super::{Message, update};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::model::Model;
use crate::menu::theme::Theme;
use crate::menu::world::{Action, Event, Outcome, SnapshotList};
use crate::text::text;

/// The demo, started, its countdown stopped, on the snapshots of the
/// default environment, read: in the workspace when `workspace`.
fn tab(workspace: bool) -> Session {
    let options = DemoOptions {
        classic: !workspace,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    session.key(Key::Char(if workspace { '2' } else { 's' }));
    session
}

/// The selected snapshot.
fn selected(model: &Model) -> SnapshotName {
    model.selected_snapshot().expect("a snapshot").name.clone()
}

/// The snapshots selected and marked in `session` in turn, `count` of
/// them, one after the other down the list.
fn mark_down(session: &mut Session, count: usize) -> Vec<SnapshotName> {
    let mut marked = Vec::new();
    for _ in 0..count {
        marked.push(selected(session.model()));
        session.key(Key::Char(' '));
        session.key(Key::Down);
    }
    marked
}

/// `model` after `f`, and the actions it asks for.
fn diff(model: Model) -> (Model, Vec<Action>) {
    update(model, Message::Key(KeyPress::new(Key::Char('f'))))
}

/// The one taken earlier of `a` and `b`, of the snapshots `model` lists,
/// then the later.
fn by_age(model: &Model, a: SnapshotName, b: SnapshotName) -> (SnapshotName, SnapshotName) {
    let created = |name: &SnapshotName| {
        let listed = model.focus_snapshots().iter();
        listed.filter(|s| s.name == *name).map(|s| s.created).next()
    };
    if created(&a) < created(&b) {
        (a, b)
    } else {
        (b, a)
    }
}

#[test]
fn with_no_mark_f_diffs_the_selection_against_now() {
    for workspace in [true, false] {
        let mut session = tab(workspace);
        session.key(Key::Down);
        let model = session.into_model();
        let name = selected(&model);

        let (_, actions) = diff(model);

        assert_eq!(actions, [Action::Diff(name, None)], "{workspace}");
    }
}

#[test]
fn with_one_mark_f_diffs_it_against_now_whatever_is_selected() {
    for workspace in [true, false] {
        let mut session = tab(workspace);
        let marked = mark_down(&mut session, 1);
        session.key(Key::Down);

        let (_, actions) = diff(session.into_model());

        let [only] = <[SnapshotName; 1]>::try_from(marked).unwrap();
        assert_eq!(actions, [Action::Diff(only, None)], "{workspace}");
    }
}

#[test]
fn with_two_marks_f_diffs_the_older_against_the_newer_in_either_order() {
    for workspace in [true, false] {
        let mut session = tab(workspace);
        let [first, second] = <[SnapshotName; 2]>::try_from(mark_down(&mut session, 2)).unwrap();
        let mut backwards = tab(workspace);
        backwards.key(Key::Down);
        backwards.key(Key::Char(' '));
        backwards.key(Key::Up);
        backwards.key(Key::Char(' '));
        assert_eq!(session.model().marked, backwards.model().marked);

        let (older, newer) = by_age(session.model(), first, second);
        for marked in [session, backwards] {
            let (_, actions) = diff(marked.into_model());
            let wanted = Action::Diff(older.clone(), Some(newer.clone()));
            assert_eq!(actions, [wanted], "{workspace}");
        }
    }
    // The timeline lists the newest first: the first marked is the newer.
    let mut timeline = tab(true);
    let [newest, _] = <[SnapshotName; 2]>::try_from(mark_down(&mut timeline, 2)).unwrap();
    let (_, actions) = diff(timeline.into_model());
    assert!(matches!(&actions[..], [Action::Diff(_, Some(newer))] if *newer == newest));
}

/// `model` after the machine sent `list`, as the reply to its reading
/// or, when `changed`, with a snapshot of the machine.
fn sent(model: Model, list: SnapshotList, changed: bool) -> Model {
    let event = if changed {
        let mut snapshot = model.snapshot.clone();
        snapshot.loaded = vec![list];
        Event::Changed(Box::new(snapshot))
    } else {
        Event::SnapshotsLoaded(list)
    };
    update(model, Message::Event(Box::new(event))).0
}

#[test]
fn two_marks_taken_in_the_same_second_diff_in_the_order_zfs_took_them() {
    for (newer_first, changed) in [(true, false), (false, false), (true, true)] {
        let mut session = tab(true);
        let marked = mark_down(&mut session, 2);
        let mut model = session.into_model();
        // The machine lists the newer first, whatever the names say.
        let (newer, older) = if newer_first {
            (marked[0].clone(), marked[1].clone())
        } else {
            (marked[1].clone(), marked[0].clone())
        };
        let mut snapshots = model.focus_snapshots().to_vec();
        snapshots.retain(|s| !marked.contains(&s.name));
        let twin = |name: &SnapshotName| {
            let mut twin = model.focus_snapshots()[0].clone();
            (twin.name, twin.created) = (name.clone(), 1_000);
            twin
        };
        snapshots.extend([twin(&newer), twin(&older)]);
        let list = SnapshotList {
            environment: newer.dataset().clone(),
            snapshots,
        };
        model = sent(model, list, changed);

        // By name, by when taken and by size, as `o` sorts them in turn.
        for _ in 0..3 {
            let (after, actions) = diff(model);
            let wanted = Action::Diff(older.clone(), Some(newer.clone()));
            assert_eq!(actions, [wanted], "{newer_first} {changed}");
            model = update(after, Message::Key(KeyPress::new(Key::Char('o')))).0;
        }
    }
}

#[test]
fn marks_whose_list_is_no_longer_read_are_kept_but_diff_nothing() {
    for workspace in [true, false] {
        let mut session = tab(workspace);
        let marked = mark_down(&mut session, 2);
        let model = session.into_model();
        let mut snapshot = model.snapshot.clone();
        snapshot.loaded.clear();
        let event = Message::Event(Box::new(Event::Changed(Box::new(snapshot))));
        let (model, _) = update(model, event);

        let (model, actions) = diff(model);

        assert!(actions.is_empty(), "{workspace}: {actions:?}");
        let said = Outcome::Warning(text!("diff-unread"));
        assert_eq!(model.notification, Some(said), "{workspace}");
        let kept: BTreeSet<SnapshotName> = marked.into_iter().collect();
        assert_eq!(model.marked, kept, "{workspace}");
    }
}

#[test]
fn with_three_marks_f_sends_nothing_and_says_why() {
    for workspace in [true, false] {
        let mut session = tab(workspace);
        mark_down(&mut session, 3);

        let (model, actions) = diff(session.into_model());

        assert!(actions.is_empty(), "{workspace}: {actions:?}");
        let said = Outcome::Warning(text!("diff-marks", count = 3));
        assert_eq!(model.notification, Some(said), "{workspace}");
        assert_eq!(model.marked.len(), 3);
        // Whole on the narrowest screen.
        let drawn = crate::menu::view::tests::screen(&model, 52, 20);
        assert!(drawn.contains(&text!("diff-marks", count = 3)), "{drawn}");
    }
}

#[test]
fn marks_past_two_keep_every_mark_and_one_unmarks_alone() {
    for workspace in [true, false] {
        let mut session = tab(workspace);
        let marked = mark_down(&mut session, 5);
        let set: BTreeSet<SnapshotName> = marked.iter().cloned().collect();
        assert_eq!(session.model().marked, set, "{workspace}");

        session.key(Key::Up);
        session.key(Key::Char(' '));

        let mut left = set;
        left.remove(&marked[4]);
        assert_eq!(session.model().marked, left, "{workspace}");
    }
}

#[test]
fn a_list_read_again_takes_the_marks_of_the_snapshots_it_lost() {
    for workspace in [true, false] {
        let mut session = tab(workspace);
        let marked = mark_down(&mut session, 3);
        let model = session.into_model();
        let mut snapshots = model.focus_snapshots().to_vec();
        snapshots.retain(|snapshot| snapshot.name != marked[1]);
        let list = SnapshotList {
            environment: marked[0].dataset().clone(),
            snapshots,
        };

        let event = Message::Event(Box::new(Event::SnapshotsLoaded(list.clone())));
        let (model, _) = update(model, event);

        let left: BTreeSet<SnapshotName> = [marked[0].clone(), marked[2].clone()].into();
        assert_eq!(model.marked, left, "{workspace}");

        // A snapshot of the machine that carries the list does as well.
        let mut snapshot = model.snapshot.clone();
        let mut fewer = list;
        fewer.snapshots.retain(|s| s.name != marked[2]);
        snapshot.loaded = vec![fewer];
        let event = Message::Event(Box::new(Event::Changed(Box::new(snapshot))));
        let (model, _) = update(model, event);

        assert_eq!(model.marked, [marked[0].clone()].into(), "{workspace}");
    }
}
