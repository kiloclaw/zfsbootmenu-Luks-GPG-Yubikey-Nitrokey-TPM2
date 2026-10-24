//! A menu and the machine it runs on together: what the front ends run, and
//! what the tests drive, key by key.

use std::collections::VecDeque;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::demo::FakeMachine;
use super::machine::Machine;
use super::model::{Model, Screen};
use super::theme::Theme;
use super::update::{Message, update};
use super::world::Action;

/// How many ticks start-up may take at most in [`Session::start`].
const START_TICKS: usize = 50;

/// A menu on a machine: the fake one unless said otherwise.
#[derive(Clone, Debug)]
pub struct Session<M = FakeMachine> {
    model: Option<Model>,
    machine: M,
    /// The actions the menu asked for, in order.
    pub actions: Vec<Action>,
    /// Actions that take the console, waiting for the front end to hand it
    /// over ([`Session::console_action`]).
    console: VecDeque<Action>,
}

impl<M: Machine> Session<M> {
    /// The menu on `machine`, as it reads at first.
    pub fn on(machine: M, theme: Theme) -> Session<M> {
        let model = Model::new(machine.snapshot().clone(), theme);
        Session {
            model: Some(model),
            machine,
            actions: Vec::new(),
            console: VecDeque::new(),
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

    fn step(&mut self, message: Message) {
        let Some(model) = self.model.take() else {
            return;
        };
        let (model, actions) = update(model, message);
        self.model = Some(model);
        for action in actions {
            self.actions.push(action.clone());
            if self.machine.needs_console(&action) {
                self.console.push_back(action);
            } else {
                self.apply(action);
            }
        }
    }

    fn apply(&mut self, action: Action) {
        for event in self.machine.apply(action) {
            self.step(Message::Event(Box::new(event)));
        }
    }

    /// The next action that takes the console. The front end hands the
    /// console over, calls [`Session::perform`] with it, and takes the
    /// console back.
    pub fn console_action(&mut self) -> Option<Action> {
        self.console.pop_front()
    }

    /// Performs an action of [`Session::console_action`].
    pub fn perform(&mut self, action: Action) {
        self.apply(action);
    }

    /// A key press.
    pub fn press(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.step(Message::Key(KeyEvent::new(code, modifiers)));
        let model = self.model();
        if model.screen == Screen::Startup && model.dialog.is_none() {
            self.machine.reask();
        }
    }

    /// A plain key press.
    pub fn key(&mut self, code: KeyCode) {
        self.press(code, KeyModifiers::NONE);
    }

    /// Types `text`, one key press per character.
    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.key(KeyCode::Char(c));
        }
    }

    /// Time passes: start-up goes a step further, and the countdown runs.
    pub fn tick(&mut self, ms: u64) {
        for event in self.machine.tick() {
            self.step(Message::Event(Box::new(event)));
        }
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
