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
    waited: Vec<Duration>,
    secret: bool,
    accent: Option<char>,
    next_layouts: usize,
    /// Where the scrollback is saved; it cannot be, without.
    save_in: Option<PathBuf>,
    saved: Vec<u8>,
}

impl FakeHost {
    fn new(script: Vec<Input>) -> FakeHost {
        FakeHost {
            script: script.into(),
            sent: Vec::new(),
            waited: Vec::new(),
            secret: false,
            accent: None,
            next_layouts: 0,
            save_in: None,
            saved: Vec::new(),
        }
    }
}

impl Host for FakeHost {
    fn wait(&mut self, timeout: Duration) -> io::Result<Input> {
        self.waited.push(timeout);
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

    fn next_layout(&mut self) {
        self.next_layouts += 1;
    }

    fn pending_accent(&self) -> Option<char> {
        self.accent
    }

    fn is_secret(&self) -> bool {
        self.secret
    }

    fn save_scrollback(&mut self, text: &[u8]) -> io::Result<PathBuf> {
        let path = self
            .save_in
            .clone()
            .ok_or(io::ErrorKind::ReadOnlyFilesystem)?;
        self.saved = text.to_vec();
        Ok(path)
    }
}

fn press(key: Key) -> KeyPress {
    KeyPress {
        key,
        shift: false,
        ctrl: false,
        alt: false,
        logo: false,
        keypad: false,
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

    let ended = run(
        &mut console,
        &mut screen,
        &mut host,
        Some(&mut |text: &str| {
            texts.push(text.to_owned());
        }),
    )
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

    let ended = run(
        &mut console,
        &mut screen,
        &mut host,
        Some(&mut |text: &str| {
            texts.push(text.to_owned());
        }),
    )
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
    // The first wait ends the burst; the second lets the update's time run
    // out.
    let mut host = FakeHost::new(vec![
        output("\x1b[?2026hlate"),
        Input::Nothing,
        Input::Nothing,
    ]);
    let mut last = String::new();

    run(
        &mut console,
        &mut screen,
        &mut host,
        Some(&mut |text: &str| {
            text.clone_into(&mut last);
        }),
    )
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

    let error = run(&mut console, &mut screen, &mut host, None).unwrap_err();

    assert_eq!(error.to_string(), "no display");
    assert_eq!(screen.size(), (320, 140));
}

#[test]
fn output_that_keeps_coming_is_drawn_once() {
    let mut console = console();
    let mut screen = FakeScreen {
        presented: 0,
        broken: false,
    };
    let mut script: Vec<Input> = (0..20).map(|n| output(&format!("line {n}\r\n"))).collect();
    script.extend([Input::Nothing, Input::Hangup]);
    let mut host = FakeHost::new(script);
    let mut texts = Vec::new();

    run(
        &mut console,
        &mut screen,
        &mut host,
        Some(&mut |text: &str| {
            texts.push(text.to_owned());
        }),
    )
    .unwrap();

    // The first frame, and one for the twenty reads: they came within a
    // burst, as the fake host gives them at once.
    assert_eq!(screen.presented, 2, "{texts:?}");
    assert!(texts.last().unwrap().contains("line 19"));
}

/// A mirror whose line is full for its first `full` tries.
struct Slow {
    full: usize,
    shown: Vec<String>,
}

impl Shown for Slow {
    fn show(&mut self, text: &str) {
        self.shown.push(text.to_owned());
    }

    fn flush(&mut self) {
        self.full = self.full.saturating_sub(1);
    }

    fn is_pending(&self) -> bool {
        self.full > 0
    }
}

#[test]
fn a_screen_the_mirror_could_not_take_is_tried_again_soon() {
    let mut console = console();
    let mut screen = FakeScreen {
        presented: 0,
        broken: false,
    };
    // A hidden cursor: nothing blinks, and the loop would sleep.
    let mut host = FakeHost::new(vec![
        output("\x1b[?25l"),
        Input::Nothing,
        Input::Nothing,
        Input::Nothing,
    ]);
    let mut slow = Slow {
        full: 3,
        shown: Vec::new(),
    };

    run(&mut console, &mut screen, &mut host, Some(&mut slow)).unwrap();

    assert_eq!(host.waited[0], MIRROR_RETRY);
    assert_eq!(*host.waited.last().unwrap(), TICK);
    assert!(!slow.shown.is_empty());
}

#[test]
fn the_console_takes_its_own_keys_and_shows_what_is_being_typed() {
    let ctrl_shift_u = KeyPress {
        ctrl: true,
        shift: true,
        ..press(Key::Char('U'))
    };
    let super_space = KeyPress {
        logo: true,
        ..press(Key::Char(' '))
    };
    let mut console = console();
    let mut screen = FakeScreen {
        presented: 0,
        broken: false,
    };
    let mut host = FakeHost::new(vec![Input::Keys(vec![ctrl_shift_u]), chars("4")]);
    run(&mut console, &mut screen, &mut host, None).unwrap();
    // A code point being typed shows at the cursor; nothing went out.
    assert_eq!(
        console.preview().map(|preview| preview.chars().to_vec()),
        Some(vec!['u', '4'])
    );
    assert!(host.sent.is_empty());

    let mut console = self::console();
    let script = vec![
        Input::Keys(vec![ctrl_shift_u]),
        chars("41 "),
        Input::Keys(vec![super_space]),
    ];
    let mut host = FakeHost::new(script);
    host.accent = Some('^');
    run(&mut console, &mut screen, &mut host, None).unwrap();
    assert_eq!(host.sent, b"A");
    assert_eq!(host.next_layouts, 1);
    // The dead key's accent waits at the cursor.
    assert_eq!(
        console.preview().map(|preview| preview.chars().to_vec()),
        Some(vec!['^'])
    );

    // At a prompt that does not echo, nothing is shown.
    let mut console = self::console();
    let mut host = FakeHost::new(vec![Input::Keys(vec![ctrl_shift_u]), chars("4")]);
    host.secret = true;
    host.accent = Some('^');
    run(&mut console, &mut screen, &mut host, None).unwrap();
    assert_eq!(console.preview(), None);
}

#[test]
fn ctrl_shift_s_saves_the_scrollback_and_says_where() {
    let ctrl_shift_s = KeyPress {
        ctrl: true,
        shift: true,
        ..press(Key::Char('S'))
    };
    let mut screen = FakeScreen {
        presented: 0,
        broken: false,
    };
    let script = || {
        vec![
            output("$ ls\r\nboot\r\n$ "),
            Input::Keys(vec![ctrl_shift_s]),
        ]
    };
    let mut console = console();
    let mut host = FakeHost::new(script());
    host.save_in = Some(PathBuf::from("/zfsbootmenu/console-scrollback.txt"));
    run(&mut console, &mut screen, &mut host, None).unwrap();
    assert_eq!(host.saved, b"$ ls\nboot\n$\n");
    assert!(host.sent.is_empty());
    let text = console.text();
    assert!(
        text.ends_with("\nSaved in /zfsbootmenu/console-scrollback\n"),
        "{text:?}"
    );

    let mut console = self::console();
    let mut host = FakeHost::new(script());
    run(&mut console, &mut screen, &mut host, None).unwrap();
    assert!(console.text().contains("\nNot saved: read-only filesystem"));
}
