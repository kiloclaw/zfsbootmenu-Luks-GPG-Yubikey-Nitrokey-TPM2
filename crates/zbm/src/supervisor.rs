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
//! The kernel panics if process 1 ends, so this module cannot panic: it uses no
//! `unwrap`, `expect`, `panic!` or indexing, and handles every error by
//! logging it and carrying on.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use zbm_sys::{Ended, Signal};
use zbm_trace::crash::{Reason, Report};

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
        }
    }

    /// Writes an error to the log, as `zerror` does; failures are ignored.
    fn error(&self, text: &str) {
        if let Ok(mut kmsg) = std::fs::OpenOptions::new().append(true).open(&self.log) {
            let _ = writeln!(kmsg, "<3>ZFSBootMenu: {text}");
        }
    }

    /// Starts `program`; the recovery shell gets the default `SIGINT` action,
    /// which the supervisor ignores.
    fn spawn(&self, program: &Program, extra: &[&str], shell: bool) -> Option<u32> {
        let mut command = std::process::Command::new(&program.path);
        command.args(&program.args).args(extra);
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

    /// Waits until `pid` ends, reaping every other child that ends meanwhile,
    /// and returns its wait status (`None` if it is gone already).
    fn wait_for(&self, pid: u32) -> Option<i32> {
        loop {
            match zbm_sys::wait_any() {
                Ok(Some(Ended { pid: ended, status })) if ended == pid => return Some(status),
                Ok(Some(_)) => {}
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
    /// `events` for each step.
    pub fn run(&self, rounds: Option<usize>, events: &mut dyn FnMut(Event)) {
        let _ = zbm_sys::ignore_signal(Signal::Interrupt);
        let mut round = 0;
        let mut resume: &[&str] = &[];
        let mut report: Option<PathBuf>;
        while rounds.is_none_or(|rounds| round < rounds) {
            round += 1;
            if let Some(pid) = self.spawn(&self.startup, resume, false) {
                events(Event::Started(pid));
                let status = self.wait_for(pid).unwrap_or(-1);
                events(Event::Ended(status));
                self.error(&format!(
                    "zfsbootmenu-init ended unexpectedly (wait status {status}); \
                     starting a recovery shell"
                ));
                report = self.crash_report(pid, status);
                if let Some(path) = &report {
                    self.error(&format!("crash report: {}", path.display()));
                }
            } else {
                events(Event::Failed(self.startup.path.display().to_string()));
                report = None;
            }
            resume = &["--resume"];
            self.recovery_shell(report.as_deref(), events);
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
        if signal == 0 || signal == 0x7f {
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
    fn recovery_shell(&self, report: Option<&std::path::Path>, events: &mut dyn FnMut(Event)) {
        let mut message =
            String::from("\nZFSBootMenu ended unexpectedly; type 'exit' to start it again\n");
        if let Some(report) = report {
            message.push_str(&format!(
                "A crash report is in {}; 'zbm debug crash' shows it.\n",
                report.display()
            ));
        }
        message.push('\n');
        let _ = std::io::stderr().write_all(message.as_bytes());
        loop {
            if let Some(pid) = self.spawn(&self.shell, &[], true) {
                self.wait_for(pid);
                events(Event::Shell);
                return;
            }
            events(Event::Failed(self.shell.path.display().to_string()));
            std::thread::sleep(self.retry);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "tests may panic; the rules are for process 1"
)]
mod tests {
    use super::*;
    use zbm_testkit::TempDir;

    /// `waitpid(-1)` collects any child, so tests that start processes take
    /// turns.
    static PROCESSES: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn restarts_the_program_after_a_shell() {
        let _guard = PROCESSES.lock().unwrap();
        let dir = TempDir::new("zbm-supervisor");
        let calls = dir.join("calls");
        let startup = dir.write_executable(
            "startup",
            format!(
                "#!/bin/sh\necho \"startup $*\" >> {}\nexit 3\n",
                calls.display()
            ),
        );
        let shell = dir.write_executable(
            "shell",
            format!(
                "#!/bin/sh\ngrep SigIgn /proc/self/status >> {}\n",
                calls.display()
            ),
        );
        let log = dir.write("kmsg", "");
        let supervisor = Supervisor {
            startup: Program {
                path: startup,
                args: vec!["zfsbootmenu-init".to_owned()],
            },
            shell: Program {
                path: shell,
                args: Vec::new(),
            },
            retry: Duration::from_millis(10),
            log: log.clone(),
            base: dir.path().to_owned(),
        };
        let mut events = Vec::new();
        supervisor.run(Some(2), &mut |event| events.push(event));
        let _ = zbm_sys::restore_default(Signal::Interrupt);

        let text = std::fs::read_to_string(&calls).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4, "{text}");
        assert_eq!(lines.first(), Some(&"startup zfsbootmenu-init"));
        assert_eq!(lines.get(2), Some(&"startup zfsbootmenu-init --resume"));
        // The shell gets SIGINT back.
        let mask = lines
            .get(1)
            .and_then(|line| line.strip_prefix("SigIgn:"))
            .map(|mask| u64::from_str_radix(mask.trim(), 16).unwrap_or(u64::MAX));
        assert_eq!(mask.map(|mask| mask & 2), Some(0), "{text}");
        let ended: Vec<&Event> = events
            .iter()
            .filter(|event| matches!(event, Event::Ended(_)))
            .collect();
        assert_eq!(ended, [&Event::Ended(3 << 8), &Event::Ended(3 << 8)]);
        let kmsg = std::fs::read_to_string(&log).unwrap();
        assert!(kmsg.contains("<3>ZFSBootMenu: zfsbootmenu-init ended unexpectedly"));
    }

    #[test]
    fn a_program_killed_by_a_signal_leaves_a_crash_report() {
        let _guard = PROCESSES.lock().unwrap();
        let dir = TempDir::new("zbm-supervisor");
        let startup = dir.write_executable("startup", "#!/bin/sh\nkill -SEGV $$\n");
        let log = dir.write("kmsg", "");
        let supervisor = Supervisor {
            startup: Program {
                path: startup,
                args: Vec::new(),
            },
            shell: Program {
                path: PathBuf::from("true"),
                args: Vec::new(),
            },
            retry: Duration::from_millis(10),
            log: log.clone(),
            base: dir.path().to_owned(),
        };

        let mut events = Vec::new();
        supervisor.run(Some(1), &mut |event| events.push(event));
        let _ = zbm_sys::restore_default(Signal::Interrupt);

        let Some(Event::Started(pid)) = events.first() else {
            panic!("{events:?}");
        };
        let name = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.unwrap().file_name().into_string().ok())
            .find(|name| name.starts_with("crash-"))
            .unwrap();
        assert!(name.ends_with(&format!("-{pid}.txt")), "{name}");
        let report = Report::parse(&std::fs::read_to_string(dir.join(&name)).unwrap()).unwrap();
        assert_eq!(
            report.reason,
            Reason::Signal {
                number: 11,
                core: false
            }
        );
        let kmsg = std::fs::read_to_string(&log).unwrap();
        assert!(kmsg.contains(&format!("crash report: {}", dir.join(&name).display())));
    }

    #[test]
    fn a_crash_report_the_program_wrote_is_named() {
        let _guard = PROCESSES.lock().unwrap();
        let dir = TempDir::new("zbm-supervisor");
        let startup = dir.write_executable(
            "startup",
            format!(
                "#!/bin/sh\necho report > {}/crash-1-$$.txt\nexit 101\n",
                dir.path().display()
            ),
        );
        let log = dir.write("kmsg", "");
        let supervisor = Supervisor {
            startup: Program {
                path: startup,
                args: Vec::new(),
            },
            shell: Program {
                path: PathBuf::from("true"),
                args: Vec::new(),
            },
            retry: Duration::from_millis(10),
            log: log.clone(),
            base: dir.path().to_owned(),
        };

        supervisor.run(Some(1), &mut |_| {});
        let _ = zbm_sys::restore_default(Signal::Interrupt);

        let kmsg = std::fs::read_to_string(&log).unwrap();
        assert!(kmsg.contains("crash report: "), "{kmsg}");
        let reports = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.unwrap().file_name().into_string().ok())
            .filter(|name| name.starts_with("crash-"))
            .count();
        assert_eq!(reports, 1);
    }

    #[test]
    fn programs_that_cannot_start_are_logged() {
        let _guard = PROCESSES.lock().unwrap();
        let dir = TempDir::new("zbm-supervisor");
        let log = dir.write("kmsg", "");
        let supervisor = Supervisor {
            startup: Program {
                path: dir.join("missing"),
                args: Vec::new(),
            },
            shell: Program {
                path: PathBuf::from("true"),
                args: Vec::new(),
            },
            retry: Duration::from_millis(10),
            log: log.clone(),
            base: dir.path().to_owned(),
        };
        let mut events = Vec::new();
        supervisor.run(Some(1), &mut |event| events.push(event));
        let _ = zbm_sys::restore_default(Signal::Interrupt);
        assert!(matches!(events.first(), Some(Event::Failed(_))));
        assert_eq!(events.last(), Some(&Event::Shell));
        let kmsg = std::fs::read_to_string(&log).unwrap();
        assert!(kmsg.contains("unable to start"));
        assert_eq!(
            Supervisor::new(PathBuf::from("/libexec/zbm")).startup.args,
            ["zfsbootmenu-init", "--console"]
        );
    }
}
