//! The machine the menu boots: [`zbm_app::App`] reads it and performs the
//! actions. With a [`ServiceHandle`], the actions that leave the console
//! alone run on the service's worker while the menu goes on; those that
//! take it run here, once the service is idle.

use std::os::fd::{AsFd as _, BorrowedFd};

use zbm_app::service::{Report, ServiceHandle};
use zbm_app::{App, AppError, Recovery};
use zbm_platform::System;

use super::Machine;
use crate::menu::world::{Action, Event, Snapshot, Timeout};

/// The real machine, through the application layer.
pub struct RealMachine<'a, 'r> {
    app: App<'a>,
    /// The front end's whole system, whose console the actions that need it
    /// take.
    console: System<'a>,
    /// Opens the recovery shell.
    recovery: &'r dyn Recovery,
    /// Whether the menu should end: another instance took over.
    stop: &'r dyn Fn() -> bool,
    snapshot: Snapshot,
    /// The first snapshot went to the menu.
    sent: bool,
    /// The service that performs the actions that leave the console alone.
    service: Option<ServiceHandle>,
}

/// Whether the menu counts down before booting the environment start-up
/// chose.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Countdown {
    /// It counts down for the time the options give, as at the end of
    /// start-up.
    Run,
    /// It waits for the user, as when it is opened again, over SSH or from
    /// the recovery shell.
    Skip,
}

impl<'a, 'r> RealMachine<'a, 'r> {
    /// The machine `app` reads, with the kernels of every environment whose
    /// key is loaded. With [`Countdown::Run`] and an environment chosen by
    /// start-up, the menu counts down to booting it; otherwise it waits for
    /// the user. `stop` says when another instance took over.
    ///
    /// # Errors
    ///
    /// [`AppError::Snapshot`] when the machine cannot be read.
    pub fn new(
        app: App<'a>,
        console: System<'a>,
        recovery: &'r dyn Recovery,
        stop: &'r dyn Fn() -> bool,
        countdown: Countdown,
    ) -> Result<RealMachine<'a, 'r>, AppError> {
        let snapshot = read(&app, console, countdown)?;
        Ok(RealMachine {
            app,
            console,
            recovery,
            stop,
            snapshot,
            sent: false,
            service: None,
        })
    }

    /// With `service` performing the actions that leave the console alone.
    #[must_use]
    pub fn with_service(self, service: ServiceHandle) -> RealMachine<'a, 'r> {
        RealMachine {
            service: Some(service),
            ..self
        }
    }

    /// The events the service sent: a snapshot read after an action waits
    /// for the user, as the countdown does not run again.
    fn reports(&mut self) -> Vec<Event> {
        let Some(service) = &mut self.service else {
            return Vec::new();
        };
        let mut events = Vec::new();
        for report in service.take() {
            if let Report::Event(event) = report {
                let mut event = *event;
                if let Event::Changed(snapshot) = &mut event {
                    snapshot.timeout = Timeout::Wait;
                    self.snapshot = snapshot.clone();
                }
                events.push(event);
            }
        }
        if !events.is_empty() && (self.stop)() {
            events.push(Event::Leaving(String::new()));
        }
        events
    }
}

/// The machine as the menu shows it; the countdown only when it is to run
/// and start-up chose an environment, as in the legacy start-up program.
fn read(app: &App<'_>, console: System<'_>, countdown: Countdown) -> Result<Snapshot, AppError> {
    let mut snapshot = app.snapshot()?;
    app.read_kernels(&mut snapshot, console);
    if countdown == Countdown::Skip || snapshot.bootfs.is_none() {
        snapshot.timeout = Timeout::Wait;
    }
    Ok(snapshot)
}

impl Machine for RealMachine<'_, '_> {
    fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// The first snapshot, which opens the main view; the end of the menu
    /// once another instance took over.
    fn tick(&mut self) -> Vec<Event> {
        if (self.stop)() {
            return vec![Event::Leaving(String::new())];
        }
        if self.sent {
            return self.reports();
        }
        self.sent = true;
        vec![Event::Changed(self.snapshot.clone())]
    }

    fn poll(&mut self) -> Vec<Event> {
        self.reports()
    }

    /// Performs `action`, then reads the machine again, as the action may
    /// have changed it. The recovery shell ends the menu, as in the legacy
    /// menu: the program that ran it opens the shell, and imports the pools
    /// the shell made importable before it runs the menu again.
    fn apply(&mut self, action: Action) -> Vec<Event> {
        if action == Action::Shell {
            return vec![Event::Leaving(String::new())];
        }
        let quiet = !action.needs_console_on(&self.snapshot);
        // A service that ended performs nothing more: the action runs here.
        if quiet && let Some(service) = &mut self.service {
            if service.send(action.clone()).is_ok() {
                return Vec::new();
            }
            self.service = None;
        }
        let mut events = if quiet {
            self.app.act_quietly(&action, self.console)
        } else {
            self.app
                .act_on_console(&action, self.console, self.recovery)
        };
        if (self.stop)() {
            events.push(Event::Leaving(String::new()));
        } else if let Ok(snapshot) = read(&self.app, self.console, Countdown::Skip) {
            self.snapshot = snapshot.clone();
            events.insert(0, Event::Changed(snapshot));
        }
        events
    }

    /// Whether `action` takes the console: those that always do, and a
    /// change that may ask about hibernation or for a key, on a pool
    /// imported read-only or an environment that is locked.
    fn needs_console(&self, action: &Action) -> bool {
        action.needs_console_on(&self.snapshot) && *action != Action::Shell
    }

    fn is_busy(&self) -> bool {
        self.service.as_ref().is_some_and(ServiceHandle::is_busy)
    }

    fn cancel(&mut self) {
        if let Some(service) = &self.service {
            service.cancel();
        }
    }

    fn waker(&self) -> Option<BorrowedFd<'_>> {
        self.service.as_ref().map(|service| service.as_fd())
    }
}
