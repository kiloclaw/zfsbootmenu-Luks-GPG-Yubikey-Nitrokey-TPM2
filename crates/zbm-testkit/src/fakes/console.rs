//! A scripted console.

use std::collections::VecDeque;
use std::io;
use std::sync::Mutex;
use std::time::Duration;

use zbm_platform::console::{Console, Screen, Size, Timed};
use zbm_platform::secret::Secret;

use super::clock::FakeClock;
use crate::sync::lock;

/// One thing the user does, in the order the code under test should ask for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConsoleInput {
    /// A key press, for [`Console::read_key`].
    Key(u8),
    /// No key press (or line) before the timeout, for [`Console::read_key`]
    /// and [`Console::read_secret_within`].
    Timeout,
    /// A line of text, for [`Console::read_line`].
    Line(String),
    /// A passphrase or PIN, for [`Console::read_secret`] and
    /// [`Console::read_secret_within`].
    Secret(Vec<u8>),
    /// The end of the input: every read returns `None`.
    End,
}

/// A [`Console`] that plays a script of user input and records the output.
///
/// When the code reads something other than what the script holds next, the
/// test fails at once, naming both. An exhausted script acts as the end of the
/// input. A timed-out key press lets its timeout pass on the clock given to
/// [`FakeConsole::with_clock`].
pub struct FakeConsole {
    script: Mutex<VecDeque<ConsoleInput>>,
    output: Mutex<Vec<u8>>,
    size: Mutex<Size>,
    clock: Option<FakeClock>,
    echo: Mutex<Vec<bool>>,
    screens: Mutex<Vec<Screen>>,
    /// Run on every change of the screen.
    on_screen: Mutex<Option<ScreenHook>>,
}

/// What runs on a change of the screen.
type ScreenHook = Box<dyn Fn(Screen) + Send + Sync>;

impl Default for FakeConsole {
    fn default() -> FakeConsole {
        FakeConsole::new()
    }
}

impl FakeConsole {
    /// A console of 25 rows and 80 columns with an empty script.
    pub fn new() -> FakeConsole {
        FakeConsole {
            script: Mutex::new(VecDeque::new()),
            output: Mutex::new(Vec::new()),
            size: Mutex::new(Size {
                rows: 25,
                columns: 80,
            }),
            clock: None,
            echo: Mutex::new(Vec::new()),
            screens: Mutex::new(Vec::new()),
            on_screen: Mutex::new(None),
        }
    }

    /// The same console, letting time pass on `clock` when a key press times
    /// out.
    #[must_use]
    pub fn with_clock(mut self, clock: FakeClock) -> FakeConsole {
        self.clock = Some(clock);
        self
    }

    /// Adds input to the script.
    pub fn push(&self, input: ConsoleInput) -> &FakeConsole {
        lock(&self.script).push_back(input);
        self
    }

    /// Adds key presses.
    pub fn keys(&self, keys: &[u8]) -> &FakeConsole {
        for key in keys {
            self.push(ConsoleInput::Key(*key));
        }
        self
    }

    /// Adds a timed-out key press.
    pub fn timeout(&self) -> &FakeConsole {
        self.push(ConsoleInput::Timeout)
    }

    /// Adds a line of text.
    pub fn line(&self, line: &str) -> &FakeConsole {
        self.push(ConsoleInput::Line(line.to_owned()))
    }

    /// Adds a passphrase or PIN.
    pub fn secret(&self, secret: &[u8]) -> &FakeConsole {
        self.push(ConsoleInput::Secret(secret.to_vec()))
    }

    /// Sets the screen size.
    pub fn set_size(&self, rows: u16, columns: u16) {
        *lock(&self.size) = Size { rows, columns };
    }

    /// Everything written so far.
    pub fn output(&self) -> Vec<u8> {
        lock(&self.output).clone()
    }

    /// Everything written so far, as text.
    pub fn output_text(&self) -> String {
        String::from_utf8_lossy(&lock(&self.output)).into_owned()
    }

    /// Every change of the screen, in order. They are kept apart from the
    /// output, which holds the text alone, as the legacy code's did with its
    /// `tput` stubbed.
    pub fn screens(&self) -> Vec<Screen> {
        lock(&self.screens).clone()
    }

    /// Runs `hook` on every change of the screen, as when a prompt starts,
    /// so that a test changes the machine at that moment.
    pub fn on_screen(&self, hook: impl Fn(Screen) + Send + Sync + 'static) {
        *lock(&self.on_screen) = Some(Box::new(hook));
    }

    /// How many times the screen changed as `screen` says.
    pub fn screen_count(&self, screen: Screen) -> usize {
        lock(&self.screens)
            .iter()
            .filter(|&&made| made == screen)
            .count()
    }

    /// Every switch of echo, in order: `true` for on.
    pub fn echo_changes(&self) -> Vec<bool> {
        lock(&self.echo).clone()
    }

    /// The input the code did not read.
    pub fn remaining(&self) -> Vec<ConsoleInput> {
        lock(&self.script).iter().cloned().collect()
    }

    /// The next input, or [`ConsoleInput::End`] when the script is exhausted.
    fn next(&self, wanted: &str) -> ConsoleInput {
        let mut script = lock(&self.script);
        match script.front() {
            None | Some(ConsoleInput::End) => ConsoleInput::End,
            Some(_) => {
                let input = script.pop_front().expect("the script is not empty");
                let fits = matches!(
                    (wanted, &input),
                    ("key", ConsoleInput::Key(_) | ConsoleInput::Timeout)
                        | ("line", ConsoleInput::Line(_))
                        | ("secret", ConsoleInput::Secret(_))
                        | (
                            "timed secret",
                            ConsoleInput::Secret(_) | ConsoleInput::Timeout
                        )
                );
                assert!(
                    fits,
                    "the code reads a {wanted}, but the script holds {input:?}"
                );
                input
            }
        }
    }
}

impl Console for FakeConsole {
    fn write(&self, bytes: &[u8]) -> io::Result<()> {
        lock(&self.output).extend_from_slice(bytes);
        Ok(())
    }

    fn size(&self) -> io::Result<Size> {
        Ok(*lock(&self.size))
    }

    fn screen(&self, screen: Screen) {
        lock(&self.screens).push(screen);
        if let Some(hook) = lock(&self.on_screen).as_ref() {
            hook(screen);
        }
    }

    fn resize(&self, size: Size) -> io::Result<()> {
        *lock(&self.size) = size;
        Ok(())
    }

    fn read_key(&self, timeout: Option<Duration>) -> io::Result<Option<u8>> {
        match self.next("key") {
            ConsoleInput::Key(key) => Ok(Some(key)),
            ConsoleInput::Timeout => {
                let timeout = timeout.expect("a key press without a timeout cannot time out");
                if let Some(clock) = &self.clock {
                    clock.advance(timeout);
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn read_secret(&self) -> io::Result<Option<Secret>> {
        match self.next("secret") {
            ConsoleInput::Secret(bytes) => Ok(Some(Secret::from_vec(bytes))),
            _ => Ok(None),
        }
    }

    fn read_line(&self) -> io::Result<Option<String>> {
        match self.next("line") {
            ConsoleInput::Line(line) => Ok(Some(line)),
            _ => Ok(None),
        }
    }

    fn set_echo(&self, on: bool) {
        lock(&self.echo).push(on);
    }

    fn read_secret_within(&self, timeout: Duration) -> io::Result<Timed<Secret>> {
        match self.next("timed secret") {
            ConsoleInput::Secret(bytes) => Ok(Timed::Read(Secret::from_vec(bytes))),
            ConsoleInput::Timeout => {
                if let Some(clock) = &self.clock {
                    clock.advance(timeout);
                }
                Ok(Timed::TimedOut)
            }
            _ => Ok(Timed::Ended),
        }
    }
}
