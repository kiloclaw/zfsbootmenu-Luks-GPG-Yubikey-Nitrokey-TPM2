//! The supervisor: process 1 of the Rust runtime.
//!
//! It starts the start-up program on the console and waits for every child
//! with `waitpid(-1)`, which also reaps orphans. If the start-up program ends
//! for any reason, it logs that, opens a recovery shell, and then starts the
//! program again with `--resume`, which skips the countdown and the automatic
//! boot. It never uses `Runner` or `Child::wait`, whose own waiting its
//! `waitpid(-1)` would get in the way of, and it contains no ZFS logic. See
//! "The supervisor" in `crates/DESIGN.md`.
//!
//! With `zbm.console=drm` it first starts the console on the display, and
//! runs the programs on its terminal (`supervisor/console.rs`).
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

mod console;

use std::io::Write;
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use zbm_console::host::Modes;
use zbm_platform::RealFiles;

use zbm_sys::{Ended, Signal};
use zbm_trace::crash::{Reason, Report};

use self::console::Attached;

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
    /// How long to wait before trying again when the shell cannot start.
    pub retry: Duration,
    /// Where messages go, normally `/dev/kmsg`.
    pub log: PathBuf,
    /// The state directory, where crash reports are found and written.
    pub base: PathBuf,
    /// The console on the display, if one is wanted (`zbm console`).
    pub console: Option<Program>,
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

impl Supervisor {
    /// The supervisor of the boot image: `zbm` runs the start-up program on
    /// the console, and the shell is `/bin/bash -i`.
    pub fn new(zbm: PathBuf) -> Supervisor {
        Supervisor {
            startup: Program {
                path: zbm,
                args: vec!["zfsbootmenu-init".to_owned(), "--console".to_owned()],
            },
            shell: Program {
                path: PathBuf::from("/bin/bash"),
                args: vec!["-i".to_owned()],
            },
            retry: Duration::from_secs(10),
            log: PathBuf::from("/dev/kmsg"),
            base: PathBuf::from(crate::debug::BASE),
            console: None,
        }
    }

    /// Writes an error to the kernel log at level 3, with the `ZFSBootMenu:`
    /// prefix; failures are ignored.
    fn error(&self, text: &str) {
        if let Ok(mut kmsg) = std::fs::OpenOptions::new().append(true).open(&self.log) {
            let _ = writeln!(kmsg, "<3>ZFSBootMenu: {text}");
        }
    }

    /// Starts `program`, on the console's terminal when it is `attached`;
    /// the recovery shell gets the default `SIGINT` action, which the
    /// supervisor ignores.
    fn spawn(
        &self,
        program: &Program,
        extra: &[&str],
        shell: bool,
        attached: Option<&Attached>,
    ) -> Option<u32> {
        let on_console = attached.map(|attached| attached.command(program, extra));
        let mut command = match on_console {
            Some(Ok(command)) => command,
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
        if shell {
            zbm_sys::restore_in_child(&mut command, &[Signal::Interrupt]);
        }
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

    /// Gives the terminal back to the kernel console when the console that
    /// ended with `status` is the `attached` one.
    fn console_ended(
        &self,
        ended: u32,
        status: i32,
        attached: &mut Option<Attached>,
        events: &mut dyn FnMut(Event),
    ) {
        if attached
            .as_ref()
            .is_some_and(|console| console.pid == ended)
            && let Some(console) = attached.take()
        {
            console.detach(&self.base);
            self.error(&format!(
                "the console on the display ended (wait status {status}); \
                 going on on the kernel console"
            ));
            events(Event::Console(format!("ended {status}")));
        }
    }

    /// Waits until `pid` ends, reaping every other child that ends meanwhile,
    /// and returns its wait status (`None` if it is gone already). When the
    /// console is among them, the terminal goes back to the kernel console.
    fn wait_for(
        &self,
        pid: u32,
        attached: &mut Option<Attached>,
        events: &mut dyn FnMut(Event),
    ) -> Option<i32> {
        loop {
            match zbm_sys::wait_any() {
                Ok(Some(Ended { pid: ended, status })) if ended == pid => return Some(status),
                Ok(Some(Ended { pid: ended, status })) => {
                    self.console_ended(ended, status, attached, events);
                }
                Ok(None) => return None,
                Err(error) => {
                    self.error(&format!("waiting for processes failed: {error}"));
                    std::thread::sleep(self.retry);
                }
            }
        }
    }

    /// Runs the start-up program and restarts it after a recovery shell each
    /// time it ends, `rounds` times (without end with `None`), calling
    /// `events` for each step. A round that panics is logged and the next
    /// one starts: process 1 must not end.
    pub fn run(&self, rounds: Option<usize>, events: &mut dyn FnMut(Event)) {
        let _ = zbm_sys::ignore_signal(Signal::Interrupt);
        let mut attached = self.start_console(events);
        let mut round = 0;
        while rounds.is_none_or(|rounds| round < rounds) {
            let resume = round > 0;
            round += 1;
            let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.round(resume, &mut attached, events);
            }));
            if ran.is_err() {
                self.error("the supervisor failed in this round; starting again");
                std::thread::sleep(self.retry);
            }
        }
    }

    /// One round: the start-up program (with `--resume` after the first),
    /// then, when it ends, the recovery shell.
    fn round(&self, resume: bool, attached: &mut Option<Attached>, events: &mut dyn FnMut(Event)) {
        let extra: &[&str] = if resume { &["--resume"] } else { &[] };
        let modes = self.modes(attached.as_ref());
        let report = if let Some(pid) = self.spawn(&self.startup, extra, false, attached.as_ref()) {
            events(Event::Started(pid));
            let status = self.wait_for(pid, attached, events).unwrap_or(-1);
            events(Event::Ended(status));
            self.error(&format!(
                "zfsbootmenu-init ended unexpectedly (wait status {status}); \
                 starting a recovery shell"
            ));
            let report = self.crash_report(pid, status);
            if let Some(path) = &report {
                self.error(&format!("crash report: {}", path.display()));
            }
            self.save_logs();
            report
        } else {
            events(Event::Failed(self.startup.path.display().to_string()));
            None
        };
        if let Some(modes) = &modes {
            let _ = modes.restore();
        }
        self.recovery_shell(report.as_deref(), attached, events);
    }

    /// The modes of the terminal the programs run on, to give back to the
    /// recovery shell when the start-up program is killed in raw mode.
    fn modes(&self, attached: Option<&Attached>) -> Option<Modes> {
        match attached {
            Some(console) => Modes::save(console.terminal()),
            None => Modes::save(std::io::stdin().as_fd()),
        }
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
        if let Err(error) = command.spawn() {
            self.error(&format!("unable to save the logs: {error}"));
        }
    }

    /// The crash report of the start-up program `pid`, which ended with the
    /// wait `status`: the one its panic hook wrote, or, when a signal killed
    /// it, one written here, which names the signal.
    fn crash_report(&self, pid: u32, status: i32) -> Option<PathBuf> {
        // `crash-TIME-PID.txt`, or `crash-TIME-PID.N.txt` after the first.
        let tag = format!("-{pid}.");
        let written = std::fs::read_dir(&self.base)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("crash-") && name.contains(&tag))
            })
            .max();
        if written.is_some() {
            return written;
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

    /// Runs the recovery shell until it ends; while it cannot start, tries
    /// again every `retry`, so an error never becomes a busy loop. The
    /// message before it names the crash `report`, if there is one.
    fn recovery_shell(
        &self,
        report: Option<&std::path::Path>,
        attached: &mut Option<Attached>,
        events: &mut dyn FnMut(Event),
    ) {
        let mut message =
            String::from("\nZFSBootMenu ended unexpectedly; type 'exit' to start it again\n");
        if let Some(report) = report {
            message.push_str(&format!(
                "A crash report is in {}; 'zbm debug crash' shows it.\n",
                report.display()
            ));
        }
        message.push('\n');
        // A program killed while it drew leaves the cursor hidden, and its
        // screen up.
        message.insert_str(0, "\x1b[?1049l\x1b[?25h");
        match attached {
            Some(console) => {
                // The terminal turns each line feed into a new line.
                let _ = console.write(&message);
            }
            None => {
                let _ = std::io::stderr().write_all(message.as_bytes());
            }
        }
        loop {
            if let Some(pid) = self.spawn(&self.shell, &[], true, attached.as_ref()) {
                self.wait_for(pid, attached, events);
                events(Event::Shell);
                return;
            }
            events(Event::Failed(self.shell.path.display().to_string()));
            std::thread::sleep(self.retry);
        }
    }
}

#[cfg(test)]
mod tests;
