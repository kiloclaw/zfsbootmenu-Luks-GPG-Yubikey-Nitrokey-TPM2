//! The console: prompts, key presses, passphrases and the screen size.
//!
//! The legacy code reads from its standard input and writes to its standard
//! output, which are the control terminal; it uses `stty` to switch echo off
//! and ignores `stty`'s failures, so the same code also reads from a pipe,
//! which is how the LUKS unit suites feed it passphrases. [`Terminal`] does the
//! same with the terminal's settings themselves (`termios`), without `stty`
//! (ROADMAP.md, PM11). Full-screen output, which the legacy code drew with
//! `tput`, is a [`Screen`] change written with [`Console::screen`]; `fzf` and
//! other interactive programs use the terminal directly.

use std::fs::File;
use std::io::{self, Read, Write};
use std::time::Duration;

use rustix::termios::{self, LocalModes, OptionalActions, SpecialCodeIndex};

use crate::secret::Secret;

/// The size of the screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    /// Lines.
    pub rows: u16,
    /// Columns.
    pub columns: u16,
}

/// A change of a full screen, as the legacy code made it with `tput`: the
/// sequences of ECMA-48 (ANSI) that the Linux console, xterm and the serial
/// terminals ZFSBootMenu runs on all understand, so no terminal description
/// is needed to find them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    /// Clears the screen, the cursor at the top left (`tput clear`).
    Clear,
    /// Shows the cursor (`tput cnorm`).
    ShowCursor,
    /// Hides the cursor (`tput civis`).
    HideCursor,
    /// Moves the cursor to `row` and `column`, from 0 (`tput cup`).
    MoveTo {
        /// The row.
        row: u16,
        /// The column.
        column: u16,
    },
}

impl Screen {
    /// The sequence that makes the change.
    pub fn sequence(self) -> String {
        match self {
            Screen::Clear => "\x1b[H\x1b[2J".to_owned(),
            Screen::ShowCursor => "\x1b[?25h".to_owned(),
            Screen::HideCursor => "\x1b[?25l".to_owned(),
            Screen::MoveTo { row, column } => {
                format!("\x1b[{};{}H", u32::from(row) + 1, u32::from(column) + 1)
            }
        }
    }
}

/// What a read with a time limit got.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Timed<T> {
    /// What was read.
    Read(T),
    /// Nothing arrived in time.
    TimedOut,
    /// The input ended.
    Ended,
}

/// Input from and output to the user.
pub trait Console: Send + Sync {
    /// Writes text (`echo -n`).
    ///
    /// # Errors
    ///
    /// When the console cannot be read or written.
    fn write(&self, bytes: &[u8]) -> io::Result<()>;

    /// The screen size (`stty size`).
    ///
    /// # Errors
    ///
    /// When the console cannot be read or written.
    fn size(&self) -> io::Result<Size>;

    /// Changes the screen as `screen` says, ignoring failures, as `tput`'s
    /// were.
    fn screen(&self, screen: Screen) {
        let _ = self.write(screen.sequence().as_bytes());
    }

    /// Tells the programs on the console that the screen is `size`, as a
    /// serial terminal cannot (`stty rows`, `stty cols`).
    ///
    /// # Errors
    ///
    /// When the console is not a terminal.
    fn resize(&self, size: Size) -> io::Result<()> {
        let _ = size;
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    /// Waits for one key press, without echo and without Enter, for at most
    /// `timeout` (`None`: without limit), as `read -s -N 1 -t` does. Returns the
    /// byte read, or `None` if the time passed or the input ended.
    ///
    /// # Errors
    ///
    /// When the console cannot be read or written.
    fn read_key(&self, timeout: Option<Duration>) -> io::Result<Option<u8>>;

    /// Reads a line with echo off, as the legacy LUKS prompt does, without its
    /// newline. Returns `None` if the input ends before a newline, where `read`
    /// fails.
    ///
    /// # Errors
    ///
    /// When the console cannot be read or written.
    fn read_secret(&self) -> io::Result<Option<Secret>>;

    /// Reads a line with echo on, without its newline. Returns `None` if the
    /// input ends before a newline.
    ///
    /// # Errors
    ///
    /// When the console cannot be read or written.
    fn read_line(&self) -> io::Result<Option<String>>;

    /// Switches echo on or off (`stty echo`, `stty -echo`), ignoring failures,
    /// as when the input is not a terminal.
    fn set_echo(&self, on: bool);

    /// Reads a line as a secret, without changing echo, if input arrives within
    /// `timeout`, as `IFS= read -r -t <seconds>` does on a terminal, where a
    /// line arrives whole. A line that the end of the input cuts short counts
    /// as the end.
    ///
    /// # Errors
    ///
    /// When the console cannot be read or written.
    fn read_secret_within(&self, timeout: Duration) -> io::Result<Timed<Secret>>;
}

/// No console, for programs that must never interact, such as `zbm report`:
/// writes go nowhere and every read finds the end of the input.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoConsole;

impl Console for NoConsole {
    fn write(&self, _bytes: &[u8]) -> io::Result<()> {
        Ok(())
    }

    fn size(&self) -> io::Result<Size> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn read_key(&self, _timeout: Option<Duration>) -> io::Result<Option<u8>> {
        Ok(None)
    }

    fn read_secret(&self) -> io::Result<Option<Secret>> {
        Ok(None)
    }

    fn read_line(&self) -> io::Result<Option<String>> {
        Ok(None)
    }

    fn set_echo(&self, _on: bool) {}

    fn read_secret_within(&self, _timeout: Duration) -> io::Result<Timed<Secret>> {
        Ok(Timed::Ended)
    }
}

/// The console on standard input and standard output, as the legacy code
/// uses it; the start-up sequence connects both to the control terminal,
/// whose settings are those of standard input, as `stty`'s are.
pub struct Terminal {
    input: File,
    output: File,
}

impl Terminal {
    /// The terminal on the process's standard input and output.
    ///
    /// # Errors
    ///
    /// When standard input or output cannot be duplicated.
    pub fn standard() -> io::Result<Terminal> {
        let input = io::stdin().as_fd_clone()?;
        let output = io::stdout().as_fd_clone()?;
        Ok(Terminal::new(input, output))
    }

    /// A terminal that reads `input`, whose settings it changes, and writes
    /// `output`.
    pub fn new(input: File, output: File) -> Terminal {
        Terminal { input, output }
    }

    /// Changes the settings of the terminal with `change`; nothing when it
    /// is not a terminal, as `stty` failed.
    fn change_modes(&self, change: impl FnOnce(&mut termios::Termios)) {
        if let Ok(mut modes) = termios::tcgetattr(&self.input) {
            change(&mut modes);
            let _ = termios::tcsetattr(&self.input, OptionalActions::Now, &modes);
        }
    }

    /// Runs `body` with the local modes `off` switched off (`stty -icanon
    /// -echo`), and restores the settings afterwards. When the input is not
    /// a terminal, `body` runs without changes.
    fn without<T>(&self, off: LocalModes, body: impl FnOnce() -> T) -> T {
        let saved = termios::tcgetattr(&self.input).ok();
        if saved.is_some() {
            self.change_modes(|modes| {
                modes.local_modes.remove(off);
                // A read returns with each byte, as after `stty -icanon`.
                modes.special_codes[SpecialCodeIndex::VMIN] = 1;
                modes.special_codes[SpecialCodeIndex::VTIME] = 0;
            });
        }
        let result = body();
        if let Some(saved) = saved {
            let _ = termios::tcsetattr(&self.input, OptionalActions::Now, &saved);
        }
        result
    }

    /// Reads one byte, or `None` at the end of the input.
    fn read_byte(&self) -> io::Result<Option<u8>> {
        let mut byte = [0_u8; 1];
        loop {
            match (&self.input).read(&mut byte) {
                Ok(0) => return Ok(None),
                Ok(_) => return Ok(Some(byte[0])),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }
}

/// Reading lines byte by byte keeps the terminal's input after the newline for
/// the next reader, as the shell's `read` does.
fn read_until_newline(
    mut next: impl FnMut() -> io::Result<Option<u8>>,
) -> io::Result<Option<Secret>> {
    let mut line = Secret::new();
    loop {
        match next()? {
            Some(b'\n') => return Ok(Some(line)),
            Some(byte) => line.push(byte),
            None => return Ok(None),
        }
    }
}

impl Console for Terminal {
    fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let mut output = &self.output;
        output.write_all(bytes)?;
        output.flush()
    }

    fn size(&self) -> io::Result<Size> {
        let size = termios::tcgetwinsize(&self.input)?;
        Ok(Size {
            rows: size.ws_row,
            columns: size.ws_col,
        })
    }

    fn resize(&self, size: Size) -> io::Result<()> {
        let mut winsize = termios::tcgetwinsize(&self.input)?;
        winsize.ws_row = size.rows;
        winsize.ws_col = size.columns;
        termios::tcsetwinsize(&self.input, winsize)?;
        Ok(())
    }

    fn read_key(&self, timeout: Option<Duration>) -> io::Result<Option<u8>> {
        self.without(LocalModes::ICANON | LocalModes::ECHO, || {
            if zbm_sys::wait_readable(&self.input, timeout)? {
                self.read_byte()
            } else {
                Ok(None)
            }
        })
    }

    fn read_secret(&self) -> io::Result<Option<Secret>> {
        self.without(LocalModes::ECHO, || read_until_newline(|| self.read_byte()))
    }

    fn read_line(&self) -> io::Result<Option<String>> {
        let line = read_until_newline(|| self.read_byte())?;
        Ok(line.map(|line| String::from_utf8_lossy(line.expose()).into_owned()))
    }

    fn set_echo(&self, on: bool) {
        self.change_modes(|modes| modes.local_modes.set(LocalModes::ECHO, on));
    }

    fn read_secret_within(&self, timeout: Duration) -> io::Result<Timed<Secret>> {
        if !zbm_sys::wait_readable(&self.input, Some(timeout))? {
            return Ok(Timed::TimedOut);
        }
        Ok(match read_until_newline(|| self.read_byte())? {
            Some(line) => Timed::Read(line),
            None => Timed::Ended,
        })
    }
}

/// Parses the output of `stty size`: rows and columns.
pub fn parse_size(text: &str) -> Option<Size> {
    let mut fields = text.split_whitespace();
    let rows = fields.next()?.parse().ok()?;
    let columns = fields.next()?.parse().ok()?;
    fields.next().is_none().then_some(Size { rows, columns })
}

/// Duplicating the standard streams lets [`Terminal`] own them as files.
trait AsFdClone {
    fn as_fd_clone(&self) -> io::Result<File>;
}

impl<T: std::os::fd::AsFd> AsFdClone for T {
    fn as_fd_clone(&self) -> io::Result<File> {
        Ok(File::from(self.as_fd().try_clone_to_owned()?))
    }
}
