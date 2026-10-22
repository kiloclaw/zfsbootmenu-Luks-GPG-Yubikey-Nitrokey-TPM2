//! A scripted console.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use zbm_core::secret::Secret;
use zbm_system::console::{Console, Size, Timed};

use super::clock::FakeClock;

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
    script: RefCell<VecDeque<ConsoleInput>>,
    output: RefCell<Vec<u8>>,
    size: Cell<Size>,
    clock: Option<FakeClock>,
    echo: RefCell<Vec<bool>>,
}

impl Default for FakeConsole {
    fn default() -> FakeConsole {
        FakeConsole::new()
    }
}

impl FakeConsole {
    /// A console of 25 rows and 80 columns with an empty script.
    pub fn new() -> FakeConsole {
        FakeConsole {
            script: RefCell::new(VecDeque::new()),
            output: RefCell::new(Vec::new()),
            size: Cell::new(Size {
                rows: 25,
                columns: 80,
            }),
            clock: None,
            echo: RefCell::new(Vec::new()),
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
        self.script.borrow_mut().push_back(input);
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
        self.size.set(Size { rows, columns });
    }

    /// Everything written so far.
    pub fn output(&self) -> Vec<u8> {
        self.output.borrow().clone()
    }

    /// Everything written so far, as text.
    pub fn output_text(&self) -> String {
        String::from_utf8_lossy(&self.output.borrow()).into_owned()
    }

    /// Every switch of echo, in order: `true` for on.
    pub fn echo_changes(&self) -> Vec<bool> {
        self.echo.borrow().clone()
    }

    /// The input the code did not read.
    pub fn remaining(&self) -> Vec<ConsoleInput> {
        self.script.borrow().iter().cloned().collect()
    }

    /// The next input, or [`ConsoleInput::End`] when the script is exhausted.
    fn next(&self, wanted: &str) -> ConsoleInput {
        let mut script = self.script.borrow_mut();
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
        self.output.borrow_mut().extend_from_slice(bytes);
        Ok(())
    }

    fn size(&self) -> io::Result<Size> {
        Ok(self.size.get())
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
        self.echo.borrow_mut().push(on);
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
