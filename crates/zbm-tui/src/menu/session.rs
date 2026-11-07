//! A menu and the machine it runs on together: what the front ends run, and
//! what the tests drive, key by key.

use std::collections::VecDeque;

use zbm_display::keys::{Key, KeyPress};

use super::demo::FakeMachine;
use super::keymap::Map;
use super::machine::Machine;
use super::model::{Dialog, Model};
use super::theme::Theme;
use super::update::{Message, update};
use super::world::{Action, Event};

/// How many ticks start-up may take at most in [`Session::start`].
const START_TICKS: usize = 50;

/// A menu on a machine: the fake one unless said otherwise.
#[derive(Debug)]
pub struct Session<M = FakeMachine> {
    model: Option<Model>,
    machine: M,
    /// The actions the menu asked for, in order, without their secrets:
    /// the session lasts until `kexec`, longer than a secret may; and
    /// without what only reads the machine ([`Action::is_read`]), as the
    /// scan of the pools the menu asks for by itself.
    pub actions: Vec<Action>,
    /// Actions that take the console, waiting for the front end to hand it
    /// over ([`Session::console_action`]).
    console: VecDeque<Action>,
    /// Whether the last action sent is a plan the machine took while it
    /// performed nothing else: closing its review page cancels it then,
    /// and only then, as a cancel reaches every action not done.
    plan_alone: bool,
}

/// `action` as the session records it: an unlock without the secret typed.
fn recorded(action: &Action) -> Action {
    match action {
        Action::Unlock { volume, method, .. } => Action::Unlock {
            volume: volume.clone(),
            method: *method,
            secret: None,
        },
        other => other.clone(),
    }
}

impl<M: Machine> Session<M> {
    /// The menu on `machine` in `theme`, as it reads at first, reading keys
    /// with the key map `map`.
    pub fn on(machine: M, theme: Theme, map: Map) -> Session<M> {
        let mut model = Model::new(machine.snapshot().clone(), theme);
        model.map = map;
        // The workspace groups the environments by pool.
        model.refresh();
        Session {
            model: Some(model),
            machine,
            actions: Vec::new(),
            console: VecDeque::new(),
            plan_alone: false,
        }
    }

    /// The menu on `machine`, going on from `model`, as the session's menu
    /// goes on from the start-up view and from its last round.
    pub fn with_model(machine: M, model: Model) -> Session<M> {
        Session {
            model: Some(model),
            machine,
            actions: Vec::new(),
            console: VecDeque::new(),
            plan_alone: false,
        }
    }

    /// The menu.
    #[allow(
        clippy::unreachable,
        reason = "update takes the model by value; step puts it back before returning"
    )]
    pub fn model(&self) -> &Model {
        self.model.as_ref().unwrap_or_else(|| unreachable!())
    }

    /// The menu, taken out of the session; the model is not `Clone`, as an
    /// unlock dialog holds what was typed.
    #[allow(
        clippy::unreachable,
        reason = "update takes the model by value; step puts it back before returning"
    )]
    pub fn into_model(mut self) -> Model {
        self.model.take().unwrap_or_else(|| unreachable!())
    }

    fn step(&mut self, message: Message) {
        let Some(model) = self.model.take() else {
            return;
        };
        let (model, actions) = update(model, message);
        self.model = Some(model);
        for action in actions {
            if !action.is_read() {
                self.actions.push(recorded(&action));
            }
            if self.machine.needs_console(&action) {
                self.console.push_back(action);
            } else {
                self.apply(action);
            }
        }
    }

    fn apply(&mut self, action: Action) {
        let alone = matches!(action, Action::Plan(_)) && !self.machine.is_busy();
        let events = self.machine.apply(action);
        self.plan_alone = alone;
        self.events(events);
    }

    /// Steps `events`, then tells the menu whether the machine works.
    fn events(&mut self, events: Vec<Event>) {
        for event in events {
            self.step(Message::Event(Box::new(event)));
        }
        let busy = self.machine.is_busy();
        if busy != self.model().working.is_some() {
            self.step(Message::Busy(busy));
        }
    }

    /// The next action that takes the console, once the machine performs
    /// nothing on its own. The front end hands the console over, calls
    /// [`Session::perform`] with it, and takes the console back.
    pub fn console_action(&mut self) -> Option<Action> {
        if self.machine.is_busy() {
            return None;
        }
        self.console.pop_front()
    }

    /// What the machine sent since: the events of what it performs.
    pub fn poll(&mut self) {
        let events = self.machine.poll();
        self.events(events);
    }

    /// Readable when [`Session::poll`] has something.
    pub fn waker(&self) -> Option<std::os::fd::BorrowedFd<'_>> {
        self.machine.waker()
    }

    /// Performs an action of [`Session::console_action`].
    pub fn perform(&mut self, action: Action) {
        self.apply(action);
    }

    /// A key press. While the machine performs an action, Escape cancels
    /// it, unless a dialog is open; a review page that closes while the
    /// machine plans its change, and performs nothing else, cancels the
    /// plan.
    pub fn press(&mut self, key: KeyPress) {
        if key.key == Key::Escape && self.machine.is_busy() && self.model().dialog.is_none() {
            self.machine.cancel();
            return;
        }
        let planning = matches!(
            &self.model().dialog,
            Some(Dialog::Review(reviewing)) if reviewing.plan.is_none()
        );
        self.step(Message::Key(key));
        if planning && self.plan_alone && self.model().dialog.is_none() && self.machine.is_busy() {
            self.machine.cancel();
        }
        let model = self.model();
        if model.is_starting_up() && model.dialog.is_none() {
            self.machine.reask();
        }
    }

    /// A plain key press.
    pub fn key(&mut self, key: Key) {
        self.press(KeyPress::new(key));
    }

    /// Types `text`, one key press per character.
    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.key(Key::Char(c));
        }
    }

    /// Time passes: start-up goes a step further, and the countdown runs.
    pub fn tick(&mut self, ms: u64) {
        let events = self.machine.tick();
        self.events(events);
        self.step(Message::Tick(ms));
    }

    /// Ticks until start-up is over or a dialog waits for the user.
    pub fn start(&mut self) {
        for _ in 0..START_TICKS {
            let model = self.model();
            if model.started || model.dialog.is_some() {
                return;
            }
            self.tick(0);
        }
    }

    /// Why the menu ended, once it did.
    pub fn leaving(&self) -> Option<&str> {
        self.model().leaving.as_deref()
    }

    /// The key presses so far.
    pub fn keys(&self) -> u32 {
        self.model().keys
    }
}
