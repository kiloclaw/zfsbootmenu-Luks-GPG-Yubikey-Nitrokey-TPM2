//! The menu tells its machine when it counted the PIN's proof out.

use std::cell::Cell;
use std::rc::Rc;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::keymap::Map;
use crate::menu::session::Session;
use crate::menu::theme::Theme;

/// A machine whose PIN's proof lasts a second, that sends `sent` with its
/// next tick, and counts how often the menu told it the proof lapsed.
struct Lapsing {
    snapshot: Snapshot,
    sent: Vec<Event>,
    relocked: Rc<Cell<usize>>,
}

impl Machine for Lapsing {
    fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    fn tick(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.sent)
    }

    fn apply(&mut self, _action: Action) -> Vec<Event> {
        Vec::new()
    }

    fn relock(&mut self) {
        self.relocked.set(self.relocked.get() + 1);
    }
}

#[test]
fn the_machine_is_told_when_the_menu_counted_the_proof_out() {
    let snapshot = Snapshot {
        proof_left: Some(1),
        ..Snapshot::default()
    };
    let relocked = Rc::new(Cell::new(0));
    let machine = Lapsing {
        snapshot: snapshot.clone(),
        sent: Vec::new(),
        relocked: relocked.clone(),
    };
    let mut session = Session::on(machine, Theme::new(Capabilities::FULL), Map::default());
    session.tick(999);
    assert_eq!(relocked.get(), 0);
    session.tick(1);
    assert!(session.model().snapshot.menu_locked);
    assert_eq!(relocked.get(), 1);
    // Once.
    session.tick(1000);
    assert_eq!(relocked.get(), 1);

    // A machine read as locked knows it already.
    let mut session = Session::on(
        Lapsing {
            snapshot: Snapshot::default(),
            sent: vec![Event::Changed(Box::new(Snapshot {
                menu_locked: true,
                ..snapshot
            }))],
            relocked: relocked.clone(),
        },
        Theme::new(Capabilities::FULL),
        Map::default(),
    );
    session.tick(0);
    assert!(session.model().snapshot.menu_locked);
    assert_eq!(relocked.get(), 1);
}
