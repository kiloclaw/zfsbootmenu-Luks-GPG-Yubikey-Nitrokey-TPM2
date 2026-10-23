//! A menu and a fake machine together: what the demo runs, and what the
//! tests drive, key by key.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{DemoOptions, FakeMachine};
use crate::menu::model::{Model, Screen};
use crate::menu::theme::Theme;
use crate::menu::update::{Message, update};
use crate::menu::world::Action;

/// How many ticks start-up may take at most in [`Session::start`].
const START_TICKS: usize = 50;

/// A menu on a fake machine.
#[derive(Clone, Debug)]
pub struct Session {
    model: Option<Model>,
    machine: FakeMachine,
    /// The actions the menu asked for, in order.
    pub actions: Vec<Action>,
}

impl Session {
    /// The menu on the demo machine `options` describe, at power-on.
    pub fn new(options: &DemoOptions, theme: Theme) -> Session {
        let machine = FakeMachine::new(options);
        let model = Model::new(machine.snapshot().clone(), theme);
        Session {
            model: Some(model),
            machine,
            actions: Vec::new(),
        }
    }

    /// The menu.
    #[allow(
        clippy::unreachable,
        reason = "update takes the model by value; step puts it back before returning"
    )]
    pub fn model(&self) -> &Model {
        self.model
            .as_ref()
            .unwrap_or_else(|| unreachable!("the model is always put back"))
    }

    fn step(&mut self, message: Message) {
        let Some(model) = self.model.take() else {
            return;
        };
        let (model, actions) = update(model, message);
        self.model = Some(model);
        for action in actions {
            self.actions.push(action.clone());
            for event in self.machine.apply(action) {
                self.step(Message::Event(event));
            }
        }
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
            self.step(Message::Event(event));
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
