//! The console on the display under the supervisor (`zbm.console=drm`,
//! ROADMAP.md #19): it runs as a child of the supervisor with the
//! controlling side of a pseudo-terminal, and the start-up program and the
//! recovery shell run on the other side, each in a session of its own with
//! that terminal as its controlling terminal (`setsid -c`), so that
//! `Ctrl-C` and job control work as on the kernel console.
//!
//! The supervisor keeps the programs' side open between programs, so that
//! the console sees the programs gone only when the supervisor is, and has
//! what is written to `/dev/console` go there (`TIOCCONS`). If the
//! console ends, the supervisor gives the virtual terminal back to the
//! kernel console (text mode, the keyboard as it was), and the programs,
//! which lose their terminal, go on there after the recovery shell. A
//! program that ended with the terminal may be collected before the
//! console: the hung-up terminal tells the supervisor that the console is
//! ending, and it waits for it before it starts anything else.
//!
//! Like the rest of the supervisor, this cannot panic, and it starts the
//! console itself.

use std::fs::File;
use std::io::{self, Write as _};
use std::os::fd::AsFd as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use zbm_platform::RealFiles;
use zbm_state::State;
use zbm_sys::console::{
    DisplayMode, KeyboardMode, Watch, keyboard_mode, redirect_console, set_display_mode,
    set_keyboard_mode, wait_ready,
};

use super::Program;
use zbm_console::keys::Flavour;
use zbm_console::relay::SavedModes;

/// The supervisor's terminal: its standard input.
fn terminal() -> std::io::Stdin {
    std::io::stdin()
}

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
    /// The modes the supervisor's terminal had, which the console's relay
    /// changes and a console killed by a signal cannot give back.
    modes: Option<SavedModes>,
    /// The terminal description the programs run with, as the console
    /// sends keys for it.
    flavour: Flavour,
}

/// The variable that tells programs they run on the console on the
/// display, whose palette is the Linux console's and which draws lines
/// whatever the locale (the menu's capabilities).
pub const MARKER: &str = "ZBM_CONSOLE";

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
        let keyboard = keyboard_mode(terminal()).ok();
        let modes = SavedModes::of(io::stdin());
        let (master, slave) = zbm_console::host::open_pty()?;
        let mut command = Command::new(&program.path);
        command.args(&program.args).stdin(Stdio::from(master));
        // Without the supervisor's blocked `SIGCHLD`, when it starts again.
        zbm_sys::prepare_child(&mut command, &zbm_sys::ChildSetup::default());
        let child = command.spawn()?;
        let pid = child.id();
        Ok(Attached {
            pid,
            slave,
            keyboard,
            modes,
            flavour: Flavour::of_image(Path::exists),
        })
    }

    /// A console with process ID `pid` on the terminal `slave`, for tests.
    #[cfg(test)]
    pub fn for_tests(pid: u32, slave: File) -> Attached {
        Attached {
            pid,
            slave,
            keyboard: None,
            modes: None,
            flavour: Flavour::Xterm,
        }
    }

    /// The command that runs `program` on the console's terminal, which it
    /// starts with as its standard streams; the caller has it start a
    /// session of its own with that terminal ([`zbm_sys::Session::NewOnTerminal`],
    /// as `setsid -c` does).
    ///
    /// # Errors
    ///
    /// When the terminal cannot be shared with it.
    pub fn command(&self, program: &Program, extra: &[&str]) -> io::Result<Command> {
        let mut command = Command::new(&program.path);
        command
            .args(&program.args)
            .args(extra)
            .stdin(Stdio::from(self.slave.try_clone()?))
            .stdout(Stdio::from(self.slave.try_clone()?))
            .stderr(Stdio::from(self.slave.try_clone()?))
            .env("TERM", self.flavour.term())
            .env(MARKER, "drm");
        if self.flavour == Flavour::Xterm {
            command.env("COLORTERM", "truecolor");
        }
        Ok(command)
    }

    /// Has what programs write to `/dev/console` go to the console's
    /// terminal. The kernel undoes it when the console ends, which hangs
    /// the terminal up.
    ///
    /// # Errors
    ///
    /// When the kernel refuses: the process may not, or another terminal
    /// has `/dev/console` already.
    pub fn redirect_console(&self) -> io::Result<()> {
        redirect_console(&self.slave)
    }

    /// The programs' side of the terminal.
    pub fn terminal(&self) -> &File {
        &self.slave
    }

    /// Whether the terminal was hung up: the console closed its side,
    /// which it does only as it ends. Programs started on it end at once.
    pub fn is_hung_up(&self) -> bool {
        let watch = Watch::read(self.slave.as_fd());
        wait_ready(&[watch], Some(Duration::ZERO))
            .is_ok_and(|ready| ready.iter().any(|ready| ready.closed))
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
    /// console ended, with the modes it had, and forgets the console in the
    /// state directory `base`.
    pub fn detach(self, base: &Path) {
        if let Some(keyboard) = self.keyboard {
            let _ = set_display_mode(terminal(), DisplayMode::Text);
            let _ = set_keyboard_mode(terminal(), keyboard);
        }
        if let Some(modes) = &self.modes {
            let _ = modes.restore();
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
        let (master, slave) = zbm_console::host::open_pty().unwrap();
        // The supervisor's terminal, whose modes come back when the console
        // ended (`relay::SavedModes` has the test of the restoring).
        let (_kernel_master, kernel) = zbm_console::host::open_pty().unwrap();
        let modes = SavedModes::of(&kernel);
        assert!(modes.is_some());
        let attached = Attached {
            pid: 1,
            slave,
            keyboard: Some(KeyboardMode::OFF),
            modes,
            flavour: Flavour::Linux,
        };
        State::new(&RealFiles, dir.path()).set_console(1).unwrap();

        let command = attached
            .command(
                &program("/libexec/zbm", &["zfsbootmenu-init"]),
                &["--resume"],
            )
            .unwrap();
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(command.get_program(), "/libexec/zbm");
        assert_eq!(args, ["zfsbootmenu-init", "--resume"]);

        attached.write("hello").unwrap();
        let mut shown = [0_u8; 5];
        std::io::Read::read_exact(&mut &master, &mut shown).unwrap();
        assert_eq!(&shown, b"hello");

        attached.detach(dir.path());
        assert!(!dir.join("console").exists());
    }

    #[test]
    fn dev_console_goes_only_to_a_terminal() {
        let attached = Attached::for_tests(1, File::open("/dev/null").unwrap());
        assert!(attached.redirect_console().is_err());
    }
}
