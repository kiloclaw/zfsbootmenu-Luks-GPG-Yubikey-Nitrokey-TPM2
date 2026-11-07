//! Tests of the Snapshots tab as the workspace draws it: the inspector of
//! a folded group and of a marked snapshot, the matches of a filter, and
//! long names taken at the edges of time.

use zbm_display::keys::Key;

use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::model::Screen;
use crate::menu::theme::Theme;
use crate::menu::view::tests::screen;
use crate::menu::world::{SnapshotInfo, SnapshotList};

/// The demo in the workspace, started, on the Snapshots tab of the
/// default environment, after `keys`.
fn session_after(keys: &[Key]) -> Session {
    let options = DemoOptions {
        workspace: true,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    session.key(Key::Char('2'));
    for key in keys {
        session.key(*key);
    }
    session
}

#[test]
fn the_inspector_of_a_folded_group_says_how_many_snapshots_it_holds() {
    let mut keys = vec![Key::Down; 7];
    keys.push(Key::Char('z'));
    let session = session_after(&keys);

    for (width, height) in [(120, 40), (80, 24), (52, 20)] {
        let shown = screen(session.model(), width, height);
        assert!(shown.contains("┌ Week of 12 October ─"), "{shown}");
        assert!(shown.contains("Snapshots  7 snapshots"), "{shown}");
        assert!(shown.contains("Used  301 MiB"), "{shown}");
        assert!(shown.contains("zroot › zroot/ROOT/void\n"), "{shown}");
        // A snapshot's actions are not offered there: `Enter` opens it.
        assert!(!shown.contains("Enter Choose"), "{shown}");
        assert!(!shown.contains("r roll back"), "{shown}");
        assert!(!shown.contains("r Roll Back"), "{shown}");
    }
}

#[test]
fn a_marked_snapshot_is_marked_in_the_inspector_s_title() {
    let session = session_after(&[Key::Down, Key::Char(' ')]);

    let shown = screen(session.model(), 120, 40);

    assert!(
        shown.contains("┌ autosnap_2026-10-22_daily ──── • ┐"),
        "{shown}"
    );
}

#[test]
fn a_filter_draws_its_matches_without_groups() {
    let mut session = session_after(&[Key::Char('/')]);
    session.type_text("10-1");

    for (width, height) in [(120, 40), (80, 24)] {
        let shown = screen(session.model(), width, height);
        assert!(shown.contains("ENVIRONMENTS"), "{shown}");
        assert!(shown.contains("/ 10-1"), "{shown}");
        assert!(!shown.contains("Week of"), "{shown}");
    }
}

#[test]
fn long_names_taken_at_the_edges_of_time_are_drawn_at_every_size() {
    let session = session_after(&[]);
    let mut model = session.into_model();
    let now = model.snapshot.now;
    let environment = "zroot/ROOT/void".parse().unwrap();
    let taken = |name: String, created| SnapshotInfo {
        name: format!("zroot/ROOT/void@{name}").parse().unwrap(),
        created,
        used: u64::MAX,
    };
    model.snapshot.loaded = vec![SnapshotList {
        environment,
        snapshots: vec![
            taken("a".repeat(200), 0),
            taken("b".repeat(200), u64::MAX),
            taken("c".to_owned(), now + 1),
            taken("d".to_owned(), u64::MAX),
        ],
    }];
    model.rank_again();
    model.refresh();
    assert_eq!(model.screen, Screen::Snapshots);

    for (width, height) in [(120, 40), (80, 25), (80, 24), (52, 20)] {
        let shown = screen(&model, width, height);
        // The month is folded, and its label cut short when narrow.
        assert!(shown.contains("January 1"), "{shown}");
        assert!(shown.contains("folded"), "{shown}");
        assert!(shown.contains("Today"), "{shown}");
        assert!(shown.contains("3 snapshots"), "{shown}");
    }
}
