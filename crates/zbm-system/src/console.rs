//! The console: prompts, key presses, passphrases and the screen size.
//!
//! The legacy code reads from its standard input and writes to its standard
//! output, which are the control terminal; it uses `stty` to switch echo off
//! and ignores `stty`'s failures, so the same code also reads from a pipe,
//! which is how the LUKS unit suites feed it passphrases. [`Terminal`] does the
//! same. Full-screen output (`tput clear`, cursor movement) is text written
//! with [`Console::write`]; `fzf` and other interactive programs use the
//! terminal directly.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::time::Duration;

use zbm_core::exec::{Command, Input, Runner, Stream};
use zbm_core::secret::Secret;

/// The size of the screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    /// Lines.
    pub rows: u16,
    /// Columns.
    pub columns: u16,
}

/// Input from and output to the user.
pub trait Console {
    /// Writes text (`echo -n`).
    fn write(&self, bytes: &[u8]) -> io::Result<()>;

    /// The screen size (`stty size`).
    fn size(&self) -> io::Result<Size>;

    /// Waits for one key press, without echo and without Enter, for at most
    /// `timeout` (`None`: without limit), as `read -s -N 1 -t` does. Returns the
    /// byte read, or `None` if the time passed or the input ended.
    fn read_key(&self, timeout: Option<Duration>) -> io::Result<Option<u8>>;

    /// Reads a line with echo off, as `luks_read_secret` does, without its
    /// newline. Returns `None` if the input ends before a newline, where `read`
    /// fails.
    fn read_secret(&self) -> io::Result<Option<Secret>>;

    /// Reads a line with echo on, without its newline. Returns `None` if the
    /// input ends before a newline.
    fn read_line(&self) -> io::Result<Option<String>>;
}

/// The control terminal: standard input and standard output.
pub struct Terminal<'a> {
    runner: &'a dyn Runner,
    input: File,
    output: File,
}

impl<'a> Terminal<'a> {
    /// The terminal on the process's standard input and output; `runner` runs
    /// `stty`, which acts on the inherited standard input.
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
            Ok(output) if output.status.success() => Some(output.stdout_text()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_core::exec::{Output, Status};
    use zbm_testutil::{FakeRunner, TempDir};

    struct Setup {
        _dir: TempDir,
        runner: FakeRunner,
        input: File,
        output: File,
        output_path: std::path::PathBuf,
    }

    fn setup(input: &[u8], tty: bool) -> Setup {
        let dir = TempDir::new("zbm-console");
        let input_path = dir.write("input", input);
        let output_path = dir.write("output", "");
        let runner = FakeRunner::new();
        if tty {
            runner.respond("stty -g", 0, "500:5:bf:8a3b\n");
            runner.respond("stty size", 0, "25 80\n");
        } else {
            runner.respond_program("stty", 1, "");
        }
        Setup {
            input: File::open(input_path).unwrap(),
            output: std::fs::OpenOptions::new()
                .write(true)
                .open(&output_path)
                .unwrap(),
            output_path,
            runner,
            _dir: dir,
        }
    }

    #[test]
    fn keys_switch_the_terminal_and_restore_it() {
        let setup = setup(b"\x1bx", true);
        let terminal = Terminal::new(&setup.runner, setup.input, setup.output);
        assert_eq!(
            terminal.read_key(Some(Duration::from_secs(1))).unwrap(),
            Some(0x1b)
        );
        assert_eq!(terminal.read_key(None).unwrap(), Some(b'x'));
        // The end of the input ends the wait.
        assert_eq!(
            terminal.read_key(Some(Duration::from_secs(1))).unwrap(),
            None
        );
        assert_eq!(
            setup.runner.lines()[..3],
            ["stty -g", "stty -icanon -echo", "stty 500:5:bf:8a3b"]
        );
        assert!(
            setup
                .runner
                .calls()
                .iter()
                .all(|call| *call.get_stdin() == Input::Inherit)
        );
    }

    #[test]
    fn secrets_are_read_without_echo() {
        let setup = setup(b"hunter2\nnext line\npartial", true);
        let terminal = Terminal::new(&setup.runner, setup.input, setup.output);
        let secret = terminal.read_secret().unwrap().unwrap();
        assert_eq!(secret.expose(), b"hunter2");
        assert_eq!(
            setup.runner.lines(),
            ["stty -g", "stty -echo", "stty 500:5:bf:8a3b"]
        );
        // The rest of the input stays for the next reader.
        assert_eq!(terminal.read_line().unwrap().as_deref(), Some("next line"));
        assert_eq!(terminal.read_line().unwrap(), None);
    }

    #[test]
    fn without_a_terminal_input_is_read_as_it_comes() {
        let setup = setup(b"pw\n", false);
        let terminal = Terminal::new(&setup.runner, setup.input, setup.output);
        assert_eq!(terminal.read_secret().unwrap().unwrap().expose(), b"pw");
        // Only the attempt to save the settings ran.
        assert_eq!(setup.runner.lines(), ["stty -g"]);
        assert!(terminal.size().is_err());
    }

    #[test]
    fn writes_and_sizes() {
        let setup = setup(b"", true);
        let terminal = Terminal::new(&setup.runner, setup.input, setup.output);
        terminal.write(b"Booting ").unwrap();
        terminal.write(b"tank/ROOT/void\n").unwrap();
        assert_eq!(
            std::fs::read(&setup.output_path).unwrap(),
            b"Booting tank/ROOT/void\n"
        );
        assert_eq!(
            terminal.size().unwrap(),
            Size {
                rows: 25,
                columns: 80
            }
        );
    }

    #[test]
    fn parses_stty_sizes() {
        assert_eq!(
            parse_size("50 132\n"),
            Some(Size {
                rows: 50,
                columns: 132
            })
        );
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("50"), None);
        assert_eq!(parse_size("50 x"), None);
        assert_eq!(parse_size("1 2 3"), None);
    }

    #[test]
    fn the_standard_terminal_can_be_opened() {
        let runner = FakeRunner::new();
        runner.on(|_| Some(Ok(Output::new(Status::exited(1), ""))));
        assert!(Terminal::standard(&runner).is_ok());
    }
}
