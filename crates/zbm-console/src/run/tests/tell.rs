//! What the console asks of its host: the status line, the bell and
//! screenshots; and what a host does without them.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn the_host_says_the_status_sounds_the_bell_and_keeps_screenshots() {
    use crate::screen::{Locks, Ring};
    let print = press(Key::Print);
    let mut screen = FakeScreen {
        presented: 0,
        broken: false,
    };
    let mut console = console().with_status_line().with_bell(Ring::Both);
    let mut host = FakeHost::new(vec![
        output("$ \x07"),
        Input::Keys(vec![print]),
        Input::Nothing,
    ]);
    host.status = Some(Status {
        layout: "fr".to_owned(),
        locks: Locks {
            caps: true,
            ..Locks::default()
        },
        ..Status::default()
    });
    run(&mut console, &mut screen, &mut host, None).unwrap();
    assert_eq!(host.rings, 1);
    assert_eq!(host.shots.len(), 1);
    assert!(host.shots[0].starts_with("$\n"), "{:?}", host.shots);
    let text = console.text();
    assert!(
        text.contains("\nScreenshot saved in /zfsbootmenu/screens\n"),
        "{text:?}"
    );
    assert!(text.ends_with("  fr  Caps\n"), "{text:?}");

    // The default host saves none.
    let mut bare = Bare;
    assert!(bare.save_screenshot(b"", b"").is_err());
    assert!(bare.status().is_none());
    bare.ring();
    bare.switch_layout("fr");
    bare.send_client(&FromConsole::Bell);
    bare.note("nothing");
    assert_eq!(bare.layout_name(), "");
}
