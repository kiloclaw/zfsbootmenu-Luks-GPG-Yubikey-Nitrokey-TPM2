//! The console: prompts, key presses, passphrases and the screen size.
//!
//! The legacy code reads from its standard input and writes to its standard
//! output, which are the control terminal; it uses `stty` to switch echo off
//! and ignores `stty`'s failures, so the same code also reads from a pipe,
//! which is how the LUKS unit suites feed it passphrases. [`Terminal`] does the
//! same. Full-screen output (`tput clear`, cursor movement) is text written
//! with [`Console::write`]; `fzf` and other interactive programs use the
//! terminal directly.

#![allow(
    clippy::disallowed_methods,
    reason = "the real implementation runs this program"
)]

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::time::Duration;

use crate::exec::{Command, Input, Runner, Stream};
use crate::secret::Secret;

/// The size of the screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    /// Lines.
    pub rows: u16,
    /// Columns.
    pub columns: u16,
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
/// uses it; the start-up sequence connects both to the control terminal.
pub struct Terminal<'a> {
    runner: &'a dyn Runner,
    input: File,
    output: File,
}

impl<'a> Terminal<'a> {
    /// The terminal on the process's standard input and output; `runner` runs
    /// `stty`, which acts on the inherited standard input.
    ///
    /// # Errors
    ///
    /// When standard input or output cannot be duplicated.
    pub fn standard(runner: &'a dyn Runner) -> io::Result<Terminal<'a>> {
        let input = io::stdin().as_fd_clone()?;
        let output = io::stdout().as_fd_clone()?;
        Ok(Terminal::new(runner, input, output))
    }

    /// A terminal that reads `input` and writes `output`.
    pub fn new(runner: &'a dyn Runner, input: File, output: File) -> Terminal<'a> {
        Terminal {
            runner,
            input,
            output,
        }
    }

    /// Runs `stty` with `args` and returns its output, or `None` if it failed,
    /// as when the input is not a terminal.
    fn stty(&self, args: &[&str]) -> Option<String> {
        let command = Command::new("stty")
            .args(args)
            .stdin(Input::Inherit)
            .stderr(Stream::Null);
        match self.runner.run(&command) {
            Ok(output) if output.status.is_success() => Some(output.stdout_text()),
            _ => None,
        }
    }

    /// Runs `body` with the terminal settings `mode` and restores the previous
    /// settings afterwards. If the settings cannot be read (no terminal), `body`
    /// runs without changes.
    fn with_mode<T>(&self, mode: &[&str], body: impl FnOnce() -> T) -> T {
        let saved = self.stty(&["-g"]);
        let saved = saved
            .as_deref()
            .map(str::trim)
            .filter(|saved| !saved.is_empty());
        if saved.is_some() {
            self.stty(mode);
        }
        let result = body();
        if let Some(saved) = saved {
            self.stty(&[saved]);
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

impl Console for Terminal<'_> {
    fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let mut output = &self.output;
        output.write_all(bytes)?;
        output.flush()
    }

    fn size(&self) -> io::Result<Size> {
        let text = self
            .stty(&["size"])
            .ok_or_else(|| io::Error::other("stty size failed"))?;
        parse_size(&text).ok_or_else(|| io::Error::other(format!("unexpected stty size: {text}")))
    }

    fn read_key(&self, timeout: Option<Duration>) -> io::Result<Option<u8>> {
        self.with_mode(&["-icanon", "-echo"], || {
            if zbm_sys::wait_readable(self.input.as_raw_fd(), timeout)? {
                self.read_byte()
            } else {
                Ok(None)
            }
        })
    }

    fn read_secret(&self) -> io::Result<Option<Secret>> {
        self.with_mode(&["-echo"], || read_until_newline(|| self.read_byte()))
    }

    fn read_line(&self) -> io::Result<Option<String>> {
        let line = read_until_newline(|| self.read_byte())?;
        Ok(line.map(|line| String::from_utf8_lossy(line.expose()).into_owned()))
    }

    fn set_echo(&self, on: bool) {
        self.stty(&[if on { "echo" } else { "-echo" }]);
    }

    fn read_secret_within(&self, timeout: Duration) -> io::Result<Timed<Secret>> {
        if !zbm_sys::wait_readable(self.input.as_raw_fd(), Some(timeout))? {
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
