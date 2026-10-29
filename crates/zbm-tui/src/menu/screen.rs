//! The menu's screen in the session (ROADMAP.md, PM9): the front end and the
//! model the menu goes on with, from the start-up view that showed the
//! stages, and from one round of the recovery loop to the next. Between
//! rounds the terminal is the recovery shell's and the hooks'.

use std::io;

use super::frontend::{Frontend, Input, Surface};
use super::machine::RealMachine;
use super::model::Model;
use super::session::Session;
use super::theme::Theme;

/// The screen the session's menu runs on.
pub trait MenuScreen {
    /// Runs the menu on `machine` until it ends: the screen takes the
    /// terminal, and gives it back at the end. Why the menu ended.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be taken, read or drawn on.
    fn run(&mut self, machine: RealMachine<'_, '_>) -> io::Result<String>;
}

/// The screen on a front end, with the model the menu goes on with: the
/// start-up view's, or, without one, a new one in `theme`.
pub struct SessionScreen<B: Surface, I: Input> {
    frontend: Frontend<B, I>,
    model: Option<Model>,
    theme: Theme,
}

impl<B: Surface, I: Input> SessionScreen<B, I>
where
    B::Error: Send + Sync + 'static,
{
    /// The screen on `frontend`, going on from `model` when there is one.
    pub fn new(
        frontend: Frontend<B, I>,
        model: Option<Model>,
        theme: Theme,
    ) -> SessionScreen<B, I> {
        SessionScreen {
            frontend,
            model,
            theme,
        }
    }

    /// The model the next round goes on from, for the tests.
    pub fn model(&self) -> Option<&Model> {
        self.model.as_ref()
    }
}

impl<B: Surface, I: Input> MenuScreen for SessionScreen<B, I>
where
    B::Error: Send + Sync + 'static,
{
    fn run(&mut self, machine: RealMachine<'_, '_>) -> io::Result<String> {
        self.frontend.take_back()?;
        let mut session = match self.model.take() {
            Some(mut model) => {
                model.begin_round();
                Session::with_model(machine, model)
            }
            None => Session::on(machine, self.theme),
        };
        let why = self.frontend.run(&mut session);
        self.model = Some(session.into_model());
        why
    }
}

#[cfg(test)]
mod tests;
