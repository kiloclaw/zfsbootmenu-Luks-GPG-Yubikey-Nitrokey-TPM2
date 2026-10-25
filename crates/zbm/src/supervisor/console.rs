//! The console on the display under the supervisor (`zbm.console=drm`,
//! ROADMAP.md #19): it runs as a child of the supervisor with the
//! controlling side of a pseudo-terminal, and the start-up program and the
//! recovery shell run on the other side, each in a session of its own with
//! that terminal as its controlling terminal (`setsid -c`), so that
//! `Ctrl-C` and job control work as on the kernel console.
//!
//! The supervisor keeps the programs' side open between programs, so that
//! the console sees the programs gone only when the supervisor is. If the
//! console ends, the supervisor gives the virtual terminal back to the
//! kernel console (text mode, the keyboard as it was), and the programs,
//! which lose their terminal, go on there after the recovery shell.
//!
//! Like the rest of the supervisor, this cannot panic.

use std::fs::File;
use std::io::{self, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};

use zbm_platform::RealFiles;
use zbm_state::State;
use zbm_sys::console::{
    DisplayMode, KeyboardMode, keyboard_mode, set_display_mode, set_keyboard_mode,
};

use super::Program;

/// The supervisor's terminal: its standard input.
const TERMINAL: i32 = 0;

/// The console, running.
#[derive(Debug)]
pub struct Attached {
    /// The console's process.
    pub pid: u32,
    /// The programs' side of the pseudo-terminal.
    slave: File,
    /// The keyboard mode the virtual terminal had, if the supervisor's
    /// terminal is one.
    keyboard: Option<KeyboardMode>,
}

impl Attached {
    /// Starts the console `program` on a new pseudo-terminal; it records
    /// itself in the state directory (`zbm console`), so that a console
    /// that ended at once is not recorded after it cleared it. The
    /// start-up options ask for it only when the control terminal is a
    /// virtual terminal (`zbm_startup::console::from_image`).
    ///
    /// # Errors
    ///
    /// When the pseudo-terminal or the console cannot be had.
    pub fn start(program: &Program) -> io::Result<Attached> {
        let keyboard = keyboard_mode(TERMINAL).ok();
        let (master, slave) = zbm_console::host::open_pty()?;
        let child = Command::new(&program.path)
            .args(&program.args)
            .stdin(Stdio::from(master))
            .spawn()?;
        let pid = child.id();
        Ok(Attached {
            pid,
            slave,
            keyboard,
        })
    }

    /// A console with process ID `pid` on the terminal `slave`, for tests.
    #[cfg(test)]
    pub fn for_tests(pid: u32, slave: File) -> Attached {
        Attached {
            pid,
            slave,
            keyboard: None,
        }
    }

    /// The command that runs `program` on the console's terminal.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be shared with it.
    pub fn command(&self, program: &Program, extra: &[&str]) -> io::Result<Command> {
        let mut command = Command::new("setsid");
        command
            .arg("-c")
            .arg(&program.path)
            .args(&program.args)
            .args(extra)
            .stdin(Stdio::from(self.slave.try_clone()?))
            .stdout(Stdio::from(self.slave.try_clone()?))
            .stderr(Stdio::from(self.slave.try_clone()?));
        Ok(command)
    }

    /// The programs' side of the terminal.
    pub fn terminal(&self) -> &File {
        &self.slave
    }

    /// Writes `text` on the console's terminal, as the programs would.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be written.
    pub fn write(&self, text: &str) -> io::Result<()> {
        (&self.slave).write_all(text.as_bytes())
    }

    /// Gives the virtual terminal back to the kernel console once the
    /// console ended, and forgets it in the state directory `base`.
    pub fn detach(self, base: &Path) {
        if let Some(keyboard) = self.keyboard {
            let _ = set_display_mode(TERMINAL, DisplayMode::Text);
            let _ = set_keyboard_mode(TERMINAL, keyboard);
        }
        let files = RealFiles;
        State::new(&files, base).clear_console();
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests may panic; the rules are for process 1"
)]
mod tests {
    use std::path::PathBuf;

    use zbm_testkit::TempDir;

    use super::*;

    fn program(path: &str, args: &[&str]) -> Program {
        Program {
            path: PathBuf::from(path),
            args: args.iter().map(|&arg| arg.to_owned()).collect(),
        }
    }

    #[test]
    fn a_console_that_cannot_start_is_not_recorded() {
        let dir = TempDir::new("zbm-supervisor-console");
        let missing = dir.join("missing");

        assert!(Attached::start(&program(missing.to_str().unwrap(), &[])).is_err());
        assert!(!dir.join("console").exists());
    }

    #[test]
    fn programs_run_in_a_session_of_their_own_on_the_terminal() {
        let dir = TempDir::new("zbm-supervisor-console");
        let (_master, slave) = zbm_console::host::open_pty().unwrap();
        let attached = Attached {
            pid: 1,
            slave,
            keyboard: Some(KeyboardMode::OFF),
        };
        State::new(&RealFiles, dir.path()).set_console(1).unwrap();

        let command = attached
            .command(
                &program("/libexec/zbm", &["zfsbootmenu-init"]),
                &["--resume"],
            )
            .unwrap();
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(command.get_program(), "setsid");
        assert_eq!(args, ["-c", "/libexec/zbm", "zfsbootmenu-init", "--resume"]);

        attached.write("hello").unwrap();
        let mut shown = [0_u8; 5];
        std::io::Read::read_exact(&mut &_master, &mut shown).unwrap();
        assert_eq!(&shown, b"hello");

        attached.detach(dir.path());
        assert!(!dir.join("console").exists());
    }
}
