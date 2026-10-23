//! The loop: output drawn and shown, keys typed and wiped, answers sent,
//! and how it ends.

#![allow(
    clippy::disallowed_methods,
    reason = "the fake host lets real time pass, which the parser counts"
)]

use std::collections::VecDeque;

use pretty_assertions::assert_eq;
use zbm_display::frame::Frame;
use zbm_display::grid::Grid;
use zbm_display::keys::Key;
use zbm_types::psf::Font;

use super::*;

const TER_V14B: &[u8] = include_bytes!("../../../../legacy/zfsbootmenu/fonts/ter-v14b.psf");

fn console() -> Console {
    Console::new(
        Grid::new(Font::parse(TER_V14B).unwrap(), 1),
        (40 * 8, 10 * 14),
    )
}

/// A screen that counts the frames shown, or fails.
struct FakeScreen {
    presented: usize,
    broken: bool,
}

impl Screen for FakeScreen {
    fn size(&self) -> (u32, u32) {
        (40 * 8, 10 * 14)
    }

    fn present(&mut self, frame: &mut Frame) -> io::Result<()> {
        if self.broken {
            return Err(io::Error::other("no display"));
        }
        frame.take_damage();
        self.presented += 1;
        Ok(())
    }
}

/// A host that plays a script and keeps what was sent; time passes for
/// real when nothing happens, so that synchronized updates end.
struct FakeHost {
    script: VecDeque<Input>,
    sent: Vec<u8>,
}

impl FakeHost {
    fn new(script: Vec<Input>) -> FakeHost {
        FakeHost {
            script: script.into(),
            sent: Vec::new(),
        }
    }
}

impl Host for FakeHost {
    fn wait(&mut self, timeout: Duration) -> io::Result<Input> {
        let input = self.script.pop_front().unwrap_or(Input::Hangup);
        if input == Input::Nothing {
            std::thread::sleep(timeout);
        }
        Ok(input)
    }

    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.sent.extend_from_slice(bytes);
        Ok(())
    }
}

fn press(key: Key) -> KeyPress {
    KeyPress {
        key,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

fn output(text: &str) -> Input {
    Input::Output(text.as_bytes().to_vec())
}

fn chars(text: &str) -> Input {
    Input::Keys(text.chars().map(|c| press(Key::Char(c))).collect())
}

#[test]
fn output_is_shown_keys_are_typed_and_answers_sent() {
    let mut console = console();
    let mut screen = FakeScreen {
        presented: 0,
        broken: false,
    };
    let mut host = FakeHost::new(vec![
        output("$ "),
        chars("ls"),
        Input::Keys(vec![press(Key::Enter)]),
        output("ls\r\nboot\r\n$ \x1b[6n\x1b[?1h"),
        Input::Keys(vec![press(Key::Up)]),
        Input::Nothing,
        Input::Hangup,
    ]);
    let mut texts = Vec::new();

    let ended = run(&mut console, &mut screen, &mut host, &mut |text| {
        texts.push(text.to_owned());
    })
    .unwrap();

    assert_eq!(ended, Ended::Hangup);
    assert_eq!(host.sent, b"ls\r\x1b[3;3R\x1bOA");
    assert_eq!(screen.presented, texts.len());
    assert!(texts.first().unwrap().trim().is_empty());
    assert!(texts.last().unwrap().starts_with("$ ls\nboot\n$\n"));
}

#[test]
fn a_passphrase_typed_reaches_the_program_and_nothing_else() {
    let mut console = console();
    let mut screen = FakeScreen {
        presented: 0,
        broken: false,
    };
    // The program turned echo off: what is typed is not written back.
    let mut host = FakeHost::new(vec![
        output("Enter passphrase for tank: "),
        chars("hunter2"),
        Input::Keys(vec![press(Key::Enter)]),
        output("\r\nKey loaded\r\n"),
        Input::Release,
    ]);
    let mut texts = Vec::new();

    let ended = run(&mut console, &mut screen, &mut host, &mut |text| {
        texts.push(text.to_owned());
    })
    .unwrap();

    assert_eq!(ended, Ended::Released);
    assert_eq!(host.sent, b"hunter2\r");
    assert!(texts.iter().all(|text| !text.contains("hunter2")));
    assert!(!console.text().contains("hunter2"));
    assert!(console.take_replies().is_empty());
}

#[test]
fn a_synchronized_update_is_drawn_when_its_time_runs_out() {
    let mut console = console();
    let mut screen = FakeScreen {
        presented: 0,
        broken: false,
    };
    let mut host = FakeHost::new(vec![output("\x1b[?2026hlate"), Input::Nothing]);
    let mut last = String::new();

    run(&mut console, &mut screen, &mut host, &mut |text| {
        text.clone_into(&mut last);
    })
    .unwrap();

    assert!(last.starts_with("late"), "{last:?}");
}

#[test]
fn a_display_that_fails_ends_the_loop() {
    let mut console = console();
    let mut screen = FakeScreen {
        presented: 0,
        broken: true,
    };
    let mut host = FakeHost::new(Vec::new());

    let error = run(&mut console, &mut screen, &mut host, &mut |_| {}).unwrap_err();

    assert_eq!(error.to_string(), "no display");
    assert_eq!(screen.size(), (320, 140));
}
