//! The supervisor: process 1 of the Rust runtime.
//!
//! It starts the start-up program on the console and collects every child
//! that ends with `waitpid(-1)`, which also reaps orphans. If the start-up
//! program ends for any reason, it logs that, opens a recovery shell, and then
//! starts the program again with `--resume`, which skips the countdown and the
//! automatic boot. It never uses `Runner` or `Child::wait`, whose own waiting
//! its `waitpid(-1)` would get in the way of, and it contains no ZFS logic.
//! One event loop does all of it (`supervisor/running.rs`), with the restart
//! policies of `supervisor/restart.rs`. See "The supervisor" in
//! `crates/DESIGN.md`.
//!
//! With `zbm.console=drm` it first starts the console on the display, runs
//! the programs on its terminal (`supervisor/console.rs`), and starts it
//! again a few times when it ends.
//!
//! The kernel panics if process 1 ends, so this module cannot panic: it uses no
//! `unwrap`, `expect`, `panic!` or indexing, and handles every error by
//! logging it and carrying on.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#![allow(
    clippy::disallowed_methods,
    reason = "process 1 starts, waits for and times its children itself, without the ports \
              (\"The supervisor\" in crates/DESIGN.md)"
)]

mod console;
mod restart;
mod running;

use std::fmt::Write as _;
use std::io::Write;
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use zbm_console::host::Modes;
use zbm_platform::RealFiles;

use zbm_sys::Signal;
use zbm_trace::crash::{Reason, Report};

use self::console::Attached;
pub use self::restart::Backoff;
use self::running::{Begin, Running};

/// How long the supervisor waits for the console to end once its terminal
/// hung up.
const CONSOLE_END: Duration = Duration::from_secs(5);

/// A program and its arguments.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    /// The program.
    pub path: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
}

/// What the supervisor runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Supervisor {
    /// The start-up program; `--resume` is added when it is restarted.
    pub startup: Program,
    /// The recovery shell.
    pub shell: Program,
    /// The shell run when `shell` cannot start, or fails within
    /// five seconds: Bash behind brush, so that a shell always opens.
    pub fallback: Option<Program>,
    /// How long to wait before trying again when the shell cannot start.
    pub retry: Duration,
    /// Where messages go, normally `/dev/kmsg`.
    pub log: PathBuf,
    /// The state directory, where crash reports are found and written.
    pub base: PathBuf,
    /// The console on the display, if one is wanted (`zbm console`).
    pub console: Option<Program>,
    /// Whether what programs write to `/dev/console` goes to the console's
    /// terminal (`TIOCCONS`): off in tests, where it would take the
    /// machine's console.
    pub redirect_console: bool,
    /// When the console on the display starts again once it ended.
    pub console_restart: Backoff,
}

/// What happened, for the log and for tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    /// The start-up program was started with this process ID.
    Started(u32),
    /// It ended with this raw wait status.
    Ended(i32),
    /// The recovery shell ran.
    Shell,
    /// A program could not be started.
    Failed(String),
    /// What became of the console on the display.
    Console(String),
}

/// A recovery shell that fails sooner than this did not open: the fallback
/// runs instead.
const QUICK_FAILURE: Duration = Duration::from_secs(5);

impl Supervisor {
    /// The supervisor of the boot image: `zbm` runs the start-up program on
    /// the console, and the shell is the one the runtime chose
    /// ([`zbm_platform::shell`]), with `-i`, or `/bin/bash -i`. Bash is the
    /// fallback of any other shell.
    pub fn new(zbm: PathBuf) -> Supervisor {
        let interactive = |path: PathBuf| Program {
            path,
            args: vec!["-i".to_owned()],
        };
        let bash = PathBuf::from(zbm_platform::shell::BASH);
        let shell = zbm_platform::shell::chosen().unwrap_or_else(|| bash.clone());
        let fallback = (shell != bash).then(|| interactive(bash));
        Supervisor {
            startup: Program {
                path: zbm,
                args: vec!["zfsbootmenu-init".to_owned(), "--console".to_owned()],
            },
            shell: interactive(shell),
            fallback,
            retry: Duration::from_secs(10),
            log: PathBuf::from("/dev/kmsg"),
            base: PathBuf::from(crate::debug::BASE),
            console: None,
            redirect_console: true,
            console_restart: Backoff::CONSOLE,
        }
    }

    /// Writes an error to the kernel log at level 3, with the `ZFSBootMenu:`
    /// prefix; failures are ignored.
    fn error(&self, text: &str) {
        self.log_at(3, text);
    }

    /// Writes a notice to the kernel log at level 5, as [`Supervisor::error`]
    /// does an error.
    fn notice(&self, text: &str) {
        self.log_at(5, text);
    }

    /// Writes `text` to the kernel log at `level`.
    fn log_at(&self, level: u8, text: &str) {
        if let Ok(mut kmsg) = std::fs::OpenOptions::new().append(true).open(&self.log) {
            let _ = writeln!(kmsg, "<{level}>ZFSBootMenu: {text}");
        }
    }

    /// Starts `program`, on the console's terminal when it is `attached`,
    /// in a session of its own whose controlling terminal that is; the
    /// recovery shell gets the default `SIGINT` action, which the
    /// supervisor ignores.
    fn spawn(
        &self,
        program: &Program,
        extra: &[&str],
        shell: bool,
        attached: Option<&Attached>,
    ) -> Option<u32> {
        let on_console = attached.map(|attached| attached.command(program, extra));
        let setup = zbm_sys::ChildSetup {
            default_signals: if shell {
                vec![Signal::Interrupt]
            } else {
                Vec::new()
            },
            session: if on_console.is_some() {
                zbm_sys::Session::NewOnTerminal
            } else {
                zbm_sys::Session::Inherit
            },
        };
        let mut command = match on_console {
            Some(Ok(mut command)) => {
                // The menu draws on the console's cells; the shell does not.
                if !shell && let Some(attached) = attached {
                    attached.pass_cells(&mut command);
                }
                command
            }
            Some(Err(error)) => {
                self.error(&format!("unable to use the console's terminal: {error}"));
                return None;
            }
            None => {
                let mut command = Command::new(&program.path);
                command.args(&program.args).args(extra);
                command
            }
        };
        zbm_sys::prepare_child(&mut command, &setup);
        match command.spawn() {
            // The child is collected with `waitpid(-1)`, not through `Child`.
            Ok(child) => Some(child.id()),
            Err(error) => {
                self.error(&format!(
                    "unable to start {}: {error}",
                    program.path.display()
                ));
                None
            }
        }
    }

    /// Starts the console on the display, if one is wanted; without it,
    /// the programs run on the supervisor's terminal.
    fn start_console(&self, events: &mut dyn FnMut(Event)) -> Option<Attached> {
        let program = self.console.as_ref()?;
        match Attached::start(program) {
            Ok(attached) => {
                events(Event::Console(format!("started {}", attached.pid)));
                self.redirect(&attached);
                Some(attached)
            }
            Err(error) => {
                self.error(&format!(
                    "unable to start the console on the display: {error}; \
                     using the kernel console"
                ));
                events(Event::Console(format!("unavailable: {error}")));
                None
            }
        }
    }

    /// Has what hooks and tools write to `/dev/console` show on the console
    /// `attached`, not on the kernel console it hides.
    fn redirect(&self, attached: &Attached) {
        if self.redirect_console
            && let Err(error) = attached.redirect_console()
        {
            self.error(&format!(
                "/dev/console stays on the kernel console: {error}"
            ));
        }
    }

    /// Runs the start-up program and restarts it after a recovery shell each
    /// time it ends, `rounds` times (without end with `None`), calling
    /// `events` for each step, with the console on the display if one is
    /// wanted.
    pub fn run(&self, rounds: Option<usize>, events: &mut dyn FnMut(Event)) {
        let _ = zbm_sys::ignore_signal(Signal::Interrupt);
        let attached = self.start_console(events);
        self.run_from(attached, Begin::Startup { resume: false }, rounds, events);
    }

    /// Runs from `begin` on the console `attached`, if there is one, for
    /// `rounds` rounds (`supervisor/running.rs`).
    fn run_from(
        &self,
        attached: Option<Attached>,
        begin: Begin,
        rounds: Option<usize>,
        events: &mut dyn FnMut(Event),
    ) {
        Running::new(self, events, attached, begin, rounds).run();
    }

    /// After a crash, has `zbm debug save --after-crash` save the logs on
    /// the partition `zbm.save_logs` names, when the command line asks for
    /// it. It runs as a process of its own, not waited for: probing devices
    /// and mounting the partition may hang, which process 1 must not.
    fn save_logs(&self) {
        if crate::saved::destination(&RealFiles, &self.base).is_none() {
            return;
        }
        let mut command = Command::new(&self.startup.path);
        command
            .args(["debug", "save", "--after-crash"])
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        if let Ok(kmsg) = std::fs::OpenOptions::new().append(true).open(&self.log) {
            command.stderr(kmsg);
        }
        // Without the supervisor's blocked `SIGCHLD`.
        zbm_sys::prepare_child(&mut command, &zbm_sys::ChildSetup::default());
        if let Err(error) = command.spawn() {
            self.error(&format!("unable to save the logs: {error}"));
        }
    }

    /// The crash report of the start-up program `pid`, which ended with the
    /// wait `status`: the one its panic hook wrote, or, when a signal killed
    /// it, one written here, which names the signal.
    fn crash_report(&self, pid: u32, status: i32) -> Option<PathBuf> {
        // The newest of `pid`'s, in the order of their numbers: as text,
        // `crash-TIME-PID.txt` sorts after the later `crash-TIME-PID.1.txt`.
        let written = crate::debug::crash_reports(&RealFiles, &self.base)
            .into_iter()
            .rfind(|name| {
                crate::debug::report_order(name).is_some_and(|(_, of, _)| of == u64::from(pid))
            });
        if let Some(name) = written {
            return Some(self.base.join(name));
        }
        let signal = status & 0x7f;
        // SIGHUP is no crash: the program lost its terminal, as when the
        // console on the display ends.
        if signal == 0 || signal == 0x7f || signal == 1 {
            return None;
        }
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let report = Report {
            program: "zfsbootmenu-init".to_owned(),
            binary: self.startup.path.display().to_string(),
            pid,
            time,
            reason: Reason::Signal {
                number: signal,
                core: status & 0x80 != 0,
            },
            build_id: std::fs::read(&self.startup.path)
                .ok()
                .and_then(|binary| zbm_trace::crash::build_id(&binary)),
            spans: Vec::new(),
            frames: Vec::new(),
        };
        let path = self.base.join(report.file_name());
        std::fs::write(&path, report.render()).ok().map(|()| path)
    }
}

/// The modes of the terminal the programs run on, to give back to the
/// recovery shell when the start-up program is killed in raw mode.
fn modes(attached: Option<&Attached>) -> Option<Modes> {
    match attached {
        Some(console) => Modes::save(console.terminal()),
        None => Modes::save(std::io::stdin().as_fd()),
    }
}

/// What the recovery shell is told before it starts, which names the crash
/// `report`, if there is one.
fn shell_message(report: Option<&Path>) -> String {
    // A program killed while it drew leaves the cursor hidden, and its
    // screen up.
    let mut message = String::from(
        "\x1b[?1049l\x1b[?25h\nZFSBootMenu ended unexpectedly; type 'exit' to start it again\n",
    );
    if let Some(report) = report {
        let _ = writeln!(
            message,
            "A crash report is in {}; 'zbm debug crash' shows it.",
            report.display()
        );
    }
    message.push('\n');
    message
}

#[cfg(test)]
mod report_tests;
#[cfg(test)]
mod tests;
