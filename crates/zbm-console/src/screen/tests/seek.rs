//! Search and vi mode in the scrollback.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::*;

fn plain(key: Key) -> KeyPress {
    KeyPress {
        key,
        ..KeyPress::BLANK
    }
}

fn ctrl(c: char) -> KeyPress {
    KeyPress {
        ctrl: true,
        ..plain(Key::Char(c))
    }
}

fn ctrl_shift(c: char) -> KeyPress {
    KeyPress {
        shift: true,
        ..ctrl(c)
    }
}

/// Types `keys`, which the console must all take.
fn type_keys(console: &mut Console, keys: &str) {
    for c in keys.chars() {
        assert_eq!(console.take_key(plain(Key::Char(c))), None, "{c}");
    }
}

/// A console of 20×5 cells with 12 numbered lines, 8 in its history.
fn scrolled() -> Console {
    let mut console = console().with_scrollback(20);
    for line in 1..=12 {
        console.feed(format!("line {line}\r\n").as_bytes());
    }
    console
}

/// The text of the line the vi cursor is on.
fn at_cursor(console: &Console) -> String {
    let line = console.term.vi_mode_cursor.point.line;
    let row = &console.term.grid()[line];
    let text: String = (0..console.term.grid().columns())
        .map(|column| row[Column(column)].c)
        .collect();
    text.trim_end().to_owned()
}

#[test]
fn vi_mode_moves_over_the_history_and_takes_every_key() {
    let mut console = scrolled();
    assert_eq!(console.take_key(ctrl_shift(' ')), None);
    assert!(console.is_vi_mode());
    // The cursor starts at the terminal's; `x` is not typed.
    type_keys(&mut console, "kx");
    assert_eq!(at_cursor(&console), "line 12");
    type_keys(&mut console, "g");
    assert_eq!(at_cursor(&console), "line 1");
    let text = console.text();
    assert!(text.starts_with("line 1         [8/8]\n"), "{text:?}");
    type_keys(&mut console, "Gk");
    assert_eq!(at_cursor(&console), "line 12");
    assert!(!console.is_scrolled_back());
    // Pages, and half pages.
    assert_eq!(console.take_key(ctrl('b')), None);
    assert_eq!(console.term.grid().display_offset(), 5);
    assert_eq!(console.take_key(ctrl('d')), None);
    assert_eq!(console.term.grid().display_offset(), 3);
    assert_eq!(console.take_key(plain(Key::PageDown)), None);
    assert_eq!(console.take_key(ctrl('u')), None);
    assert_eq!(console.take_key(plain(Key::Up)), None);
    assert_eq!(
        console.take_key(KeyPress {
            alt: true,
            ..plain(Key::Up)
        }),
        None
    );
    // Escape ends it, and the programs' screen is shown.
    assert_eq!(console.take_key(plain(Key::Escape)), None);
    assert!(!console.is_vi_mode() && !console.is_scrolled_back());
    assert_eq!(
        console.take_key(plain(Key::Char('k'))),
        Some(plain(Key::Char('k')))
    );
    for end in [ctrl('c'), ctrl_shift(' '), plain(Key::Char('q'))] {
        console.take_key(ctrl_shift(' '));
        assert_eq!(console.take_key(end), None);
        assert!(!console.is_vi_mode());
    }
}

#[test]
fn a_search_up_finds_the_text_typed_and_the_matches_before() {
    let mut console = scrolled();
    assert_eq!(console.take_key(ctrl_shift('B')), None);
    assert!(console.is_searching() && console.is_vi_mode());
    type_keys(&mut console, "LINE 1");
    assert!(console.text().ends_with("\nSearch up: LINE 1_ (\n"));
    // Upper case letters make the search match case.
    console.take_key(plain(Key::Backspace));
    console.take_key(plain(Key::Backspace));
    console.take_key(plain(Key::Backspace));
    type_keys(&mut console, "E 1".to_lowercase().as_str());
    let text = console.text();
    assert!(text.ends_with("\nSearch up: LINe 1_ (\n"), "{text:?}");
    for _ in 0..6 {
        console.take_key(plain(Key::Backspace));
    }
    type_keys(&mut console, "line 1");
    assert_eq!(at_cursor(&console), "line 12");
    assert!(console.text().ends_with("\nSearch up: line 1_\n"));
    assert_eq!(console.take_key(plain(Key::Enter)), None);
    assert!(!console.is_searching() && console.is_vi_mode());
    let mut found = Vec::new();
    for key in "nnnNn".chars() {
        console.take_key(plain(Key::Char(key)));
        found.push(at_cursor(&console));
    }
    assert_eq!(found, ["line 11", "line 10", "line 1", "line 10", "line 1"]);
    // `/` searches down from the cursor.
    type_keys(&mut console, "/ne 9");
    assert_eq!(at_cursor(&console), "line 9");
    console.take_key(plain(Key::Enter));
    type_keys(&mut console, "N");
    assert_eq!(at_cursor(&console), "line 9");
}

#[test]
fn escape_closes_the_search_where_it_started() {
    let mut console = scrolled();
    // From vi mode: back to the cursor, vi mode kept.
    console.take_key(ctrl_shift(' '));
    type_keys(&mut console, "g?zzz");
    assert_eq!(at_cursor(&console), "line 1");
    console.take_key(plain(Key::Escape));
    // A search goes round: up from the top, it goes on from the bottom.
    type_keys(&mut console, "?line 5");
    assert_eq!(at_cursor(&console), "line 5");
    console.take_key(plain(Key::Escape));
    type_keys(&mut console, "/line 5");
    assert_eq!(at_cursor(&console), "line 5");
    console.take_key(plain(Key::Escape));
    assert_eq!(at_cursor(&console), "line 1");
    assert!(console.is_vi_mode() && console.is_scrolled_back());
    // Emptied, the search goes back to where it started.
    type_keys(&mut console, "/line 5");
    for _ in 0..6 {
        console.take_key(plain(Key::Backspace));
    }
    assert_eq!(at_cursor(&console), "line 1");
    // Other keys do nothing to the bar.
    assert_eq!(console.take_key(plain(Key::F(1))), None);
    assert_eq!(console.take_key(ctrl('a')), None);
    console.take_key(plain(Key::Escape));
    console.take_key(plain(Key::Escape));
    // Without vi mode: it ends too.
    console.take_key(ctrl_shift('f'));
    type_keys(&mut console, "line");
    console.take_key(plain(Key::Escape));
    assert!(!console.is_vi_mode() && !console.is_scrolled_back());
}

#[test]
fn the_match_is_drawn_black_on_yellow_until_output_comes() {
    let mut console = scrolled();
    console.draw();
    let before = colours(&mut console, 1, 3);
    console.take_key(ctrl_shift('b'));
    type_keys(&mut console, "12");
    console.take_key(plain(Key::Enter));
    console.draw();
    let yellow = colour::by_index(console.term.colors(), 3);
    // `line 12` is on the fourth row: `1` is in its sixth column.
    assert!(colours(&mut console, 6, 3).contains(&yellow));
    assert_eq!(colours(&mut console, 1, 3), before);
    console.feed(b"x");
    console.draw();
    assert!(!colours(&mut console, 6, 3).contains(&yellow));
    // A search not found: nothing is shown, and `n` does nothing.
    type_keys(&mut console, "?zzz");
    console.take_key(plain(Key::Enter));
    type_keys(&mut console, "n");
    assert_eq!(console.overlay().found, None);
}

#[test]
fn neither_opens_at_a_secret_prompt_or_on_the_alternate_screen() {
    let mut console = scrolled();
    console.take_key(ctrl_shift('b'));
    type_keys(&mut console, "line");
    console.set_secret(true);
    assert!(!console.is_vi_mode() && !console.is_searching());
    for key in [ctrl_shift(' '), ctrl_shift('f'), ctrl_shift('b')] {
        assert_eq!(console.take_key(key), Some(key));
    }
    console.set_secret(false);
    console.feed(b"\x1b[?1049h");
    assert_eq!(console.take_key(ctrl_shift(' ')), Some(ctrl_shift(' ')));
}

#[test]
fn the_text_searched_for_is_matched_as_it_is() {
    assert_eq!(
        crate::screen::seek::literal(&['a', '.', 'b', '(', 'é']),
        "a\\.b\\(é"
    );
}

#[test]
fn ctrl_shift_s_asks_for_the_scrollback_once_and_a_notice_says_where() {
    let mut console = scrolled();
    assert_eq!(console.take_save(), None);
    assert_eq!(console.take_key(ctrl_shift('S')), None);
    let text = console.take_save().unwrap();
    let lines: Vec<String> = (1..=12).map(|line| format!("line {line}")).collect();
    assert_eq!(text, lines.join("\n") + "\n");
    assert_eq!(console.take_save(), None);
    // In vi mode too; the notice goes with the next key.
    console.take_key(ctrl_shift(' '));
    console.take_key(ctrl_shift('s'));
    assert!(console.take_save().is_some());
    console.set_notice("Saved in /x");
    assert!(console.text().ends_with("\nSaved in /x\n"));
    console.take_key(plain(Key::Escape));
    assert!(console.text().ends_with("\nline 12\n\n"));
    // Not at a secret prompt.
    console.set_secret(true);
    assert_eq!(console.take_key(ctrl_shift('s')), Some(ctrl_shift('s')));
    assert_eq!(console.take_save(), None);
}

#[test]
fn the_text_saved_keeps_wide_characters_and_marks_once() {
    let mut console = console().with_scrollback(20);
    console.feed("日本 e\u{301}  \r\n\r\n\r\n".as_bytes());
    assert_eq!(console.take_key(ctrl_shift('s')), None);
    assert_eq!(console.take_save().unwrap(), "日本 e\u{301}\n");
}
