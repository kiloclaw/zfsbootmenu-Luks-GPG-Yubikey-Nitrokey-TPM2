use pretty_assertions::assert_eq;
use zbm_display::keys::Key;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::theme::Theme;

#[test]
fn what_changes_without_the_person_moving_is_said_once() {
    let mut session = Session::new(&DemoOptions::default(), Theme::new(Capabilities::FULL));
    session.start();
    let mut announcer = Announcer::default();

    let first = announcer.next(session.model());
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(first[0].contains("zroot/ROOT/void"), "{first:?}");
    // Nothing changed: nothing said; moving says nothing either.
    assert_eq!(announcer.next(session.model()), Vec::<String>::new());
    session.key(Key::Escape);
    assert_eq!(
        announcer.next(session.model()),
        [text!("lines-countdown-stopped")]
    );
    session.key(Key::Down);
    assert_eq!(announcer.next(session.model()), Vec::<String>::new());

    // A dialog opens: its title and first line.
    session.key(Key::Char('s'));
    session.key(Key::Enter);
    let opened = announcer.next(session.model());
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert!(opened[0].contains(": "), "{opened:?}");
    session.key(Key::Escape);
    assert_eq!(announcer.next(session.model()), Vec::<String>::new());

    // The menu is back after the shell: its view's title.
    announcer.back();
    assert_eq!(
        announcer.next(session.model()),
        [words::title(session.model())]
    );
}
