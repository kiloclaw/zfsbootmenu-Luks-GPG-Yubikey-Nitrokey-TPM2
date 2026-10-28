//! The supervisor's loop (ADR 0001): one `poll` over the pidfds of the
//! children it runs, the hang-up of the console's terminal, and, as
//! process 1, a `signalfd` for `SIGCHLD`, which tells of the orphans that
//! come to it; the time-out is the nearest deadline. Each time it wakes it
//! collects every child that ended and moves its services on:
//!
//! - the start-up program, which the recovery shell follows when it ends,
//!   and which starts again with `--resume` once the shell ends;
//! - the recovery shell, whose fallback runs when it does not open, and
//!   which is tried again after `retry` when it cannot start;
//! - the console on the display, which starts again by its [`Backoff`],
//!   between programs only, so that a program never changes terminal while
//!   it runs.
//!
//! Nothing here sleeps or blocks but the `poll`: every wait is a deadline.

use std::io::Write as _;
use std::os::fd::AsFd as _;
use std::path::Path;
use std::time::{Duration, Instant};

use zbm_console::host::Modes;
use zbm_sys::console::{Watch, wait_ready};
use zbm_sys::{Ended, Process, Signal, SignalSet, Signals};

use super::console::Attached;
use super::restart::Backoff;
use super::{CONSOLE_END, Event, Program, QUICK_FAILURE, Supervisor};

/// The longest the loop waits for nothing, as process 1: what a lost
/// wake-up costs at most.
const IDLE: Duration = Duration::from_secs(5);

/// The longest it waits for nothing otherwise, without a `signalfd`.
const POLL: Duration = Duration::from_millis(250);

/// Where a run begins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Begin {
    /// With the start-up program, with `--resume` or without.
    Startup {
        /// Whether it resumes.
        resume: bool,
    },
    /// With the recovery shell.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the tests begin with the shell")
    )]
    Shell,
}

/// What the start-up program and the recovery shell are doing.
enum Main {
    /// The start-up program is to start.
    Startup { resume: bool },
    /// It runs; `modes` are its terminal's, to give back to the shell.
    Running {
        pid: u32,
        process: Option<Process>,
        modes: Option<Modes>,
    },
    /// The recovery shell, or its fallback, is to start once `at` passed.
    Shell { fallback: bool, at: Instant },
    /// It runs, since `started`.
    InShell {
        pid: u32,
        process: Option<Process>,
        fallback: bool,
        started: Instant,
    },
    /// The rounds asked for are over.
    Done,
}

/// The console on the display.
enum Display {
    /// None: none is wanted, or it was given up.
    Off,
    /// It runs, since `since`.
    On {
        attached: Attached,
        process: Option<Process>,
        since: Instant,
    },
    /// Its terminal hung up: it is ending, and waited for until `until`.
    Ending {
        attached: Attached,
        process: Option<Process>,
        since: Instant,
        until: Instant,
    },
    /// It starts again once `at` passed, between programs.
    Restart { at: Instant },
}

/// A run of the supervisor.
pub struct Running<'s, 'e> {
    supervisor: &'s Supervisor,
    events: &'e mut dyn FnMut(Event),
    /// `SIGCHLD`, as process 1.
    signals: Option<Signals>,
    display: Display,
    /// The console's restarts in a row so far.
    attempt: u32,
    main: Main,
    /// How many rounds to run, without end with `None`.
    rounds: Option<usize>,
    /// How many ran.
    done: usize,
    /// What to tell before the next recovery shell.
    message: Option<String>,
}

impl<'s, 'e> Running<'s, 'e> {
    /// A run of `supervisor`, on the console `attached` if there is one,
    /// from `begin`, for `rounds` rounds of the start-up program and the
    /// shell.
    pub fn new(
        supervisor: &'s Supervisor,
        events: &'e mut dyn FnMut(Event),
        attached: Option<Attached>,
        begin: Begin,
        rounds: Option<usize>,
    ) -> Running<'s, 'e> {
        // Only process 1 gets the orphans; elsewhere, a signal that another
        // thread does not block would never reach the `signalfd`.
        let signals = (std::process::id() == 1)
            .then(|| Signals::block(SignalSet::of(&[Signal::Child])).ok())
            .flatten();
        let display = attached.map_or(Display::Off, |attached| Display::On {
            process: Process::open(attached.pid).ok(),
            attached,
            since: Instant::now(),
        });
        let mut running = Running {
            supervisor,
            events,
            signals,
            display,
            attempt: 0,
            main: Main::Done,
            rounds,
            done: 0,
            message: None,
        };
        match begin {
            Begin::Startup { resume } => running.main = Main::Startup { resume },
            Begin::Shell => running.begin_shell(None),
        }
        running
    }

    /// Runs until the rounds are over; without end with `None`. A turn that
    /// panics is logged and the loop goes on: process 1 must not end.
    pub fn run(&mut self) {
        while !matches!(self.main, Main::Done) {
            let turn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.turn()));
            if turn.is_err() {
                self.supervisor
                    .error("the supervisor failed in this turn; going on");
                // The turn may have left it between two states.
                if matches!(self.main, Main::Done) && !self.has_run_all_rounds() {
                    self.main = Main::Startup { resume: true };
                }
                std::thread::sleep(self.supervisor.retry);
            }
        }
    }

    /// One turn: what is due, then a wait for the next thing to happen,
    /// then the children that ended.
    fn turn(&mut self) {
        let now = Instant::now();
        self.settle(now);
        self.advance(now);
        if matches!(self.main, Main::Done) {
            return;
        }
        self.wait();
        self.reap();
    }

    /// Sends `event`.
    fn event(&mut self, event: Event) {
        (self.events)(event);
    }

    /// The console that the programs run on, if it runs.
    fn attached(&self) -> Option<&Attached> {
        match &self.display {
            Display::On { attached, .. } => Some(attached),
            _ => None,
        }
    }

    /// Whether the start-up program or the shell runs.
    fn is_main_running(&self) -> bool {
        matches!(self.main, Main::Running { .. } | Main::InShell { .. })
    }

    /// Whether a program may start: the console runs, or none is wanted.
    fn is_display_settled(&self) -> bool {
        matches!(self.display, Display::Off | Display::On { .. })
    }

    /// Whether the rounds asked for ran.
    fn has_run_all_rounds(&self) -> bool {
        self.rounds.is_some_and(|rounds| self.done >= rounds)
    }
}

/// The console.
impl Running<'_, '_> {
    /// What became of the console: a terminal that hung up starts its end,
    /// which is given up when the console was not collected in time; a
    /// restart that is due, between programs, starts it again.
    fn settle(&mut self, now: Instant) {
        match std::mem::replace(&mut self.display, Display::Off) {
            Display::On {
                attached,
                process,
                since,
            } if attached.is_hung_up() => {
                self.display = Display::Ending {
                    attached,
                    process,
                    since,
                    until: now + CONSOLE_END,
                };
            }
            Display::Ending {
                attached,
                since,
                until,
                ..
            } if now >= until => {
                attached.detach(&self.supervisor.base);
                self.supervisor.error(
                    "the terminal of the console on the display hung up; \
                     going on on the kernel console",
                );
                self.event(Event::Console("hung up".to_owned()));
                self.schedule_restart(since, now);
            }
            Display::Restart { at } if now >= at && !self.is_main_running() => self.restart(now),
            display => self.display = display,
        }
    }

    /// The process ID of the console, if it runs or is ending.
    fn console_pid(&self) -> Option<u32> {
        match &self.display {
            Display::On { attached, .. } | Display::Ending { attached, .. } => Some(attached.pid),
            _ => None,
        }
    }

    /// The console ended with `status`: the virtual terminal goes back to
    /// the kernel console, and the console starts again by its back-off.
    fn console_ended(&mut self, status: i32, now: Instant) {
        let (Display::On {
            attached, since, ..
        }
        | Display::Ending {
            attached, since, ..
        }) = std::mem::replace(&mut self.display, Display::Off)
        else {
            return;
        };
        attached.detach(&self.supervisor.base);
        self.supervisor.error(&format!(
            "the console on the display ended (wait status {status}); \
             going on on the kernel console"
        ));
        self.event(Event::Console(format!("ended {status}")));
        self.schedule_restart(since, now);
    }

    /// After the console's run from `since`, its next restart, or none
    /// once the tries are used up.
    fn schedule_restart(&mut self, since: Instant, now: Instant) {
        if self.supervisor.console.is_none() {
            return;
        }
        let backoff: Backoff = self.supervisor.console_restart;
        self.attempt = backoff.after_run(self.attempt, now.saturating_duration_since(since));
        if let Some(delay) = backoff.delay(self.attempt) {
            self.attempt += 1;
            self.display = Display::Restart { at: now + delay };
        } else if backoff.tries > 0 {
            self.supervisor.error(&format!(
                "the console on the display ended {} times in a row; \
                 the programs stay on the kernel console",
                self.attempt + 1
            ));
            self.event(Event::Console("given up".to_owned()));
        }
    }

    /// Starts the console again.
    fn restart(&mut self, now: Instant) {
        let supervisor = self.supervisor;
        let Some(program) = &supervisor.console else {
            return;
        };
        match Attached::start(program) {
            Ok(attached) => {
                self.event(Event::Console(format!("restarted {}", attached.pid)));
                supervisor.redirect(&attached);
                self.display = Display::On {
                    process: Process::open(attached.pid).ok(),
                    attached,
                    since: now,
                };
            }
            Err(error) => {
                supervisor.error(&format!(
                    "unable to start the console on the display again: {error}"
                ));
                self.schedule_restart(now, now);
            }
        }
    }
}

/// The start-up program and the recovery shell.
impl Running<'_, '_> {
    /// Starts what is due, once the console is settled.
    fn advance(&mut self, now: Instant) {
        if !self.is_display_settled() {
            return;
        }
        match self.main {
            Main::Startup { resume } => self.start_startup(resume),
            Main::Shell { fallback, at } if now >= at => self.start_shell(fallback, now),
            _ => {}
        }
    }

    /// Starts the start-up program, with `--resume` or without.
    fn start_startup(&mut self, resume: bool) {
        let supervisor = self.supervisor;
        let extra: &[&str] = if resume { &["--resume"] } else { &[] };
        let modes = super::modes(self.attached());
        if let Some(pid) = supervisor.spawn(&supervisor.startup, extra, false, self.attached()) {
            self.event(Event::Started(pid));
            self.main = Main::Running {
                pid,
                process: Process::open(pid).ok(),
                modes,
            };
        } else {
            self.event(Event::Failed(supervisor.startup.path.display().to_string()));
            self.begin_shell(None);
        }
    }

    /// The start-up program `pid` ended with `status`: the recovery shell
    /// follows, told of its crash report, on a terminal with the `modes`
    /// the program found.
    fn startup_ended(&mut self, pid: u32, status: i32, modes: Option<&Modes>) {
        let supervisor = self.supervisor;
        take_terminal();
        self.event(Event::Ended(status));
        supervisor.error(&format!(
            "zfsbootmenu-init ended unexpectedly (wait status {status}); \
             starting a recovery shell"
        ));
        let report = supervisor.crash_report(pid, status);
        if let Some(path) = &report {
            supervisor.error(&format!("crash report: {}", path.display()));
        }
        supervisor.save_logs();
        if let Some(modes) = modes {
            let _ = modes.restore();
        }
        self.begin_shell(report.as_deref());
    }

    /// The recovery shell is to start now, after a message that names the
    /// crash `report`, if there is one.
    fn begin_shell(&mut self, report: Option<&Path>) {
        self.message = Some(super::shell_message(report));
        self.main = Main::Shell {
            fallback: false,
            at: Instant::now(),
        };
    }

    /// Starts the recovery shell, or its `fallback`.
    fn start_shell(&mut self, fallback: bool, now: Instant) {
        let supervisor = self.supervisor;
        let shell: &Program = match (&supervisor.fallback, fallback) {
            (Some(program), true) => program,
            _ => &supervisor.shell,
        };
        if let Some(message) = self.message.take() {
            match self.attached() {
                // The terminal turns each line feed into a new line.
                Some(console) => {
                    let _ = console.write(&message);
                }
                None => {
                    let _ = std::io::stderr().write_all(message.as_bytes());
                }
            }
        }
        if let Some(pid) = supervisor.spawn(shell, &[], true, self.attached()) {
            self.main = Main::InShell {
                pid,
                process: Process::open(pid).ok(),
                fallback,
                started: now,
            };
        } else {
            self.event(Event::Failed(shell.path.display().to_string()));
            self.shell_failed(shell, fallback, now);
        }
    }

    /// The shell, or its `fallback`, started at `started`, ended with
    /// `status`: the round is over, unless it did not open, when its
    /// fallback runs, or it is tried again.
    fn shell_ended(&mut self, status: i32, fallback: bool, started: Instant, now: Instant) {
        take_terminal();
        self.event(Event::Shell);
        let opened = status == 0 || now.saturating_duration_since(started) >= QUICK_FAILURE;
        if opened || fallback {
            self.round_over();
        } else {
            let supervisor = self.supervisor;
            self.shell_failed(&supervisor.shell, fallback, now);
        }
    }

    /// `shell` did not open: its fallback, if it has one and it was not
    /// the fallback that failed, runs now; else the shell is tried again
    /// after `retry`, so that an error never becomes a busy loop.
    fn shell_failed(&mut self, shell: &Program, fallback: bool, now: Instant) {
        let supervisor = self.supervisor;
        match &supervisor.fallback {
            Some(other) if !fallback => {
                supervisor.error(&format!(
                    "{} did not open; starting {}",
                    shell.path.display(),
                    other.path.display()
                ));
                self.main = Main::Shell {
                    fallback: true,
                    at: now,
                };
            }
            _ => {
                self.main = Main::Shell {
                    fallback: false,
                    at: now + supervisor.retry,
                };
            }
        }
    }

    /// A round is over: the next starts with `--resume`, unless the rounds
    /// asked for ran.
    fn round_over(&mut self) {
        self.done += 1;
        self.main = if self.has_run_all_rounds() {
            Main::Done
        } else {
            Main::Startup { resume: true }
        };
    }
}

/// Waiting and collecting.
impl Running<'_, '_> {
    /// The nearest deadline that the loop acts on, if any.
    fn deadline(&self) -> Option<Instant> {
        let display = match self.display {
            Display::Ending { until, .. } => Some(until),
            Display::Restart { at } if !self.is_main_running() => Some(at),
            _ => None,
        };
        let main = match self.main {
            Main::Shell { at, .. } if self.is_display_settled() => Some(at),
            _ => None,
        };
        display.into_iter().chain(main).min()
    }

    /// The descriptors that wake the loop.
    fn sources(&self) -> Vec<Watch<'_>> {
        let mut watches: Vec<Watch<'_>> = Vec::new();
        if let Some(signals) = &self.signals {
            watches.push(Watch::read(signals.as_fd()));
        }
        if let Main::Running {
            process: Some(process),
            ..
        }
        | Main::InShell {
            process: Some(process),
            ..
        } = &self.main
        {
            watches.push(Watch::read(process.as_fd()));
        }
        match &self.display {
            Display::On {
                attached, process, ..
            } => {
                if let Some(process) = process {
                    watches.push(Watch::read(process.as_fd()));
                }
                // Its hang-up alone: what is typed is for the programs.
                watches.push(Watch {
                    fd: attached.terminal().as_fd(),
                    read: false,
                    write: false,
                });
            }
            Display::Ending {
                process: Some(process),
                ..
            } => watches.push(Watch::read(process.as_fd())),
            _ => {}
        }
        watches
    }

    /// Waits until a source is ready or the nearest deadline passed.
    fn wait(&mut self) {
        let idle = if self.signals.is_some() { IDLE } else { POLL };
        let timeout = self.deadline().map_or(idle, |at| {
            at.saturating_duration_since(Instant::now()).min(idle)
        });
        if wait_ready(&self.sources(), Some(timeout)).is_err() {
            std::thread::sleep(POLL);
        }
        if let Some(signals) = &self.signals {
            let _ = signals.take();
        }
    }

    /// Collects every child that ended, and moves on what each was.
    fn reap(&mut self) {
        while let Ok(Some(Ended { pid, status })) = zbm_sys::reap() {
            let now = Instant::now();
            if self.console_pid() == Some(pid) {
                self.console_ended(status, now);
                continue;
            }
            match std::mem::replace(&mut self.main, Main::Done) {
                Main::Running {
                    pid: running,
                    modes,
                    ..
                } if running == pid => self.startup_ended(pid, status, modes.as_ref()),
                Main::InShell {
                    pid: running,
                    fallback,
                    started,
                    ..
                } if running == pid => self.shell_ended(status, fallback, started, now),
                // An orphan, or a process the supervisor does not wait
                // for, such as the one that saves the logs.
                main => self.main = main,
            }
        }
    }
}

/// Takes the terminal back once a program on it ended: a shell with job
/// control, such as brush, may leave its own process group in the
/// foreground, and the next program could then read no key.
fn take_terminal() {
    // Not a terminal (the tests): nothing to take back.
    let _ = zbm_sys::take_terminal(std::io::stdin());
}
