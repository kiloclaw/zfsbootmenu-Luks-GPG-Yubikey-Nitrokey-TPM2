use pretty_assertions::assert_eq;

use super::*;
use crate::decor::{Decoration, Underline};
use crate::faces::FontFace;
use crate::frame::Rgb;
use crate::keys::{Key, Media};

fn style() -> CellStyle {
    CellStyle {
        fg: Rgb(1, 2, 3),
        bg: Rgb(4, 5, 6),
        decoration: Decoration {
            underline: Underline::Curly,
            underline_colour: Some(Rgb(7, 8, 9)),
            strikeout: true,
        },
        face: FontFace::BoldItalic,
    }
}

fn run(row: u16, cells: &[&str]) -> Run {
    Run {
        row,
        column: 3,
        style: style(),
        cells: cells.iter().map(|cell| (*cell).to_owned()).collect(),
    }
}

#[test]
fn what_the_menu_sends_comes_back_as_it_was() {
    let messages = [
        FromClient::Hello { version: VERSION },
        FromClient::Cells(vec![run(0, &["a", "漢", "", "é"]), run(9, &[])]),
        FromClient::Cells(Vec::new()),
        FromClient::Cursor(Some((12, 3))),
        FromClient::Cursor(None),
        FromClient::Secret(true),
        FromClient::Secret(false),
        FromClient::SwitchLayout("de(nodeadkeys)".to_owned()),
        FromClient::Announce("Booting zroot/ROOT/void".to_owned()),
        FromClient::Lend,
        FromClient::Take,
    ];
    for message in messages {
        assert_eq!(FromClient::decode(&message.encode()), Some(message));
    }
}

#[test]
fn what_the_console_sends_comes_back_as_it_was() {
    let presses = [
        KeyPress::new(Key::Char('й')).with_ctrl(),
        KeyPress::new(Key::F(12))
            .with_shift()
            .with_alt()
            .with_logo(),
        KeyPress::new(Key::Media(Media::Previous)),
        KeyPress {
            keypad: true,
            ..KeyPress::new(Key::Enter)
        },
        KeyPress::new(Key::Menu),
        KeyPress::new(Key::BackTab),
    ];
    let mut messages = vec![
        FromConsole::Hello { version: VERSION },
        FromConsole::Caps(Caps {
            columns: 240,
            rows: 67,
            true_colour: true,
        }),
        FromConsole::Resize {
            columns: 80,
            rows: 25,
        },
        FromConsole::Bell,
    ];
    messages.extend(presses.into_iter().map(|press| FromConsole::Key {
        press,
        layout: "ru".to_owned(),
    }));
    for message in messages {
        assert_eq!(FromConsole::decode(&message.encode()), Some(message));
    }
}

#[test]
fn every_key_has_a_number_of_its_own() {
    let mut keys: Vec<Key> = wire_keys();
    keys.extend((1..=24).map(Key::F));
    for key in keys {
        let message = FromConsole::Key {
            press: KeyPress::new(key),
            layout: String::new(),
        };
        assert_eq!(
            FromConsole::decode(&message.encode()),
            Some(message),
            "{key:?}"
        );
    }
}

/// The keys without a value, and the media keys.
fn wire_keys() -> Vec<Key> {
    let mut keys = vec![
        Key::Enter,
        Key::Escape,
        Key::Backspace,
        Key::Tab,
        Key::BackTab,
        Key::Up,
        Key::Down,
        Key::Left,
        Key::Right,
        Key::Home,
        Key::End,
        Key::PageUp,
        Key::PageDown,
        Key::Insert,
        Key::Delete,
        Key::Print,
        Key::Pause,
        Key::Menu,
    ];
    keys.extend(
        [
            Media::Mute,
            Media::VolumeDown,
            Media::VolumeUp,
            Media::PlayPause,
            Media::Stop,
            Media::Next,
            Media::Previous,
        ]
        .map(Key::Media),
    );
    keys
}

#[test]
fn what_is_not_exactly_one_message_is_refused() {
    let hello = FromClient::Hello { version: 1 }.encode();
    let mut longer = hello.clone();
    longer.push(0);
    let cells = FromClient::Cells(vec![run(0, &["a"])]).encode();
    let mut bad_text = FromClient::SwitchLayout("ab".to_owned()).encode();
    bad_text[3] = 0xff;
    let refused: [&[u8]; 9] = [
        // A hidden cursor with a place, which no encoding gives.
        &[3, 0, 1, 0, 0, 0],
        &[],
        &[0],
        &[99],
        &longer,
        &hello[..2],
        &cells[..cells.len() - 1],
        &bad_text,
        // A boolean that is neither.
        &[4, 2],
    ];
    for bytes in refused {
        assert_eq!(FromClient::decode(bytes), None, "{bytes:?}");
        assert_eq!(FromConsole::decode(bytes).filter(|_| bytes.len() < 3), None);
    }
    // Keys that are not: no such character, function key or media key,
    // modifiers beyond the five.
    for (tag, value, bits) in [
        (0_u8, 0xd800_u32, 0_u8),
        (100, 25, 0),
        (101, 9, 0),
        (1, 0, 0x20),
    ] {
        let mut bytes = vec![3, tag];
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes.extend_from_slice(&[bits, 0, 0]);
        assert_eq!(FromConsole::decode(&bytes), None, "{tag} {value} {bits}");
    }
    assert_eq!(FromConsole::decode(&[2, 1, 0, 1, 0]), None);
}

#[test]
fn a_frame_is_split_into_packets_that_fit() {
    let wide: Vec<&str> = std::iter::repeat_n("x", 3000).collect();
    let runs = vec![run(0, &wide), run(1, &["a", "b"]), run(2, &["c"])];

    let messages = cells_messages(runs, 1000);

    assert!(messages.len() > 3, "{}", messages.len());
    let mut cells = 0;
    for message in &messages {
        assert!(message.encode().len() <= 1000);
        let FromClient::Cells(runs) = message else {
            panic!("{message:?}");
        };
        cells += runs.iter().map(|run| run.cells.len()).sum::<usize>();
    }
    assert_eq!(cells, 3003);
    // The parts of the long run follow each other.
    let FromClient::Cells(first) = &messages[0] else {
        panic!()
    };
    let FromClient::Cells(second) = &messages[1] else {
        panic!()
    };
    assert_eq!(
        second[0].column,
        first[0].column + u16::try_from(first[0].cells.len()).unwrap()
    );
    assert_eq!(cells_messages(Vec::new(), 1000), []);
}

#[test]
fn long_text_is_cut_at_a_character() {
    let long = "é".repeat(100);
    let message = FromClient::Cells(vec![run(0, &[&long])]);
    let Some(FromClient::Cells(runs)) = FromClient::decode(&message.encode()) else {
        panic!()
    };
    assert_eq!(runs[0].cells[0], "é".repeat(32));
}
