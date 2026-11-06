//! The boot history, read for a remote menu.

use pretty_assertions::assert_eq;

use super::*;
use crate::model::History;

#[test]
fn a_remote_menu_reads_the_boot_history_without_a_snapshot() {
    let fake = machine();

    assert_eq!(refusal_of(&fake, &Action::LoadHistory), None);
    let messages = answered(&fake, Action::LoadHistory);

    // The session saves no logs: the history is off, and nothing mounted.
    assert_eq!(
        messages,
        [
            ToMenu::Event(Box::new(Event::HistoryLoaded(History::Off))),
            ToMenu::Done
        ]
    );
    assert!(
        fake.runner
            .lines()
            .iter()
            .all(|line| !line.starts_with("mount"))
    );
}
