//! Start-up on a terminal (ROADMAP.md, R5 step 4): the start-up
//! view, drawn as the stages run, and the dialogs of what start-up asks.
//!
//! Start-up runs its stages on a worker, one after another, and sends
//! their events and questions to the view ([`link`]), which
//! [`StartupView::serve`] takes on the thread that owns the terminal. A
//! question opens its dialog ([`StartupView::ask`]), which reads keys until
//! it gives an answer, sent back on the question's reply: start-up waits
//! for it, as it waits at a console prompt. The view never draws the main
//! view; the menu does, once start-up is over.

pub mod link;

use std::collections::VecDeque;
use std::io;
use std::os::fd::{AsFd as _, BorrowedFd};

use ratatui::Terminal;
use zbm_platform::event::Mailbox;
use zbm_types::Answer;

use super::frontend::Frontend;
pub use super::frontend::{Input, Surface, TICK};
use super::model::Model;
use super::theme::Theme;
use super::update::{Message, update};
use super::world::{Action, Event, Snapshot};
pub use link::{MailLink, ToView, ViewLink};

/// The start-up view, as start-up's front end uses it: on a terminal
/// ([`StartupView`]), or scripted in tests.
pub trait StartupScreen {
    /// Serves start-up's stages, which send to `mailbox` from their thread,
    /// until they close the view or `running` says they ended
    /// ([`StartupView::serve`]).
    ///
    /// # Errors
    ///
    /// When the terminal cannot be read or drawn on: the view is gone.
    fn serve(
        self: Box<Self>,
        mailbox: Mailbox<ToView>,
        running: &dyn Fn() -> bool,
    ) -> io::Result<()>;
}

/// The start-up view on a front end: the start-up screen of the menu's
/// model.
pub struct StartupView<B: Surface, I: Input> {
    model: Option<Model>,
    frontend: Frontend<B, I>,
}

/// Whether `action` answers a dialog of start-up.
fn is_answer(action: &Action) -> bool {
    matches!(
        action,
        Action::Unlock { .. } | Action::SkipUnlock(_) | Action::Answer(_)
    )
}

impl<B: Surface, I: Input> StartupView<B, I>
where
    B::Error: Send + Sync + 'static,
{
    /// The view of `snapshot` on `frontend`, drawn at once.
    ///
    /// # Errors
    ///
    /// When the surface cannot be drawn on.
    pub fn new(
        frontend: Frontend<B, I>,
        snapshot: Snapshot,
        theme: Theme,
    ) -> io::Result<StartupView<B, I>> {
        let mut view = StartupView {
            model: Some(Model::new(snapshot, theme)),
            frontend,
        };
        view.draw()?;
        Ok(view)
    }

    /// The menu the view draws.
    #[allow(
        clippy::unreachable,
        reason = "step takes the model by value and puts it back before returning"
    )]
    pub fn model(&self) -> &Model {
        self.model.as_ref().unwrap_or_else(|| unreachable!())
    }

    /// The terminal, for the tests that read what it shows.
    pub fn terminal(&self) -> &Terminal<B> {
        self.frontend.terminal()
    }

    /// `message`; the actions it asked for.
    fn step(&mut self, message: Message) -> Vec<Action> {
        let Some(model) = self.model.take() else {
            return Vec::new();
        };
        let (model, actions) = update(model, message);
        self.model = Some(model);
        actions
    }

    /// Draws the start-up view, unless start-up is over: the menu draws
    /// the main view.
    fn draw(&mut self) -> io::Result<()> {
        match self.model.as_ref() {
            Some(model) if !model.started => self.frontend.draw(model),
            _ => Ok(()),
        }
    }

    /// Shows `event`: a stage started or ended, a volume opened.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be drawn on.
    pub fn send(&mut self, event: Event) -> io::Result<()> {
        let _ = self.step(Message::Event(Box::new(event)));
        self.draw()
    }

    /// Shows the dialog `event` opens and reads keys until it gives an
    /// answer: [`Action::Unlock`], [`Action::SkipUnlock`] or
    /// [`Action::Answer`]. `check` is asked as keys come and time passes,
    /// and ends the dialog with its answer when it has one, such as a
    /// device that appeared. A dialog closed without an answer opens again,
    /// as start-up still waits for one.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be read or drawn on.
    pub fn ask(
        &mut self,
        event: &Event,
        check: &mut dyn FnMut() -> Option<Action>,
    ) -> io::Result<Action> {
        self.ask_waking(event, check, None)
    }

    /// [`StartupView::ask`], reading keys no longer than until `wake` is
    /// readable, so that `check` sees what came at once.
    fn ask_waking(
        &mut self,
        event: &Event,
        check: &mut dyn FnMut() -> Option<Action>,
        wake: Option<BorrowedFd<'_>>,
    ) -> io::Result<Action> {
        let _ = self.step(Message::Event(Box::new(event.clone())));
        let mut last_tick = self.frontend.now();
        loop {
            self.draw()?;
            // A read can end before the tick with no key, as noise on a
            // serial line does: the dialog's time is the time that passed.
            let passed = self.frontend.now().saturating_sub(last_tick);
            let mut answers = match self.frontend.key(TICK.saturating_sub(passed), wake)? {
                Some(key) => self.step(Message::Key(key)),
                None => Vec::new(),
            };
            let passed = self.frontend.now().saturating_sub(last_tick);
            if !answers.iter().any(is_answer) && passed >= TICK {
                last_tick = self.frontend.now();
                let ms = u64::try_from(passed.as_millis()).unwrap_or(u64::MAX);
                answers.extend(self.step(Message::Tick(ms)));
            }
            // The dialog answered closes at once on the screen, before
            // what the answer starts.
            if let Some(answer) = answers.into_iter().find(is_answer) {
                self.draw()?;
                return Ok(answer);
            }
            if let Some(answer) = check() {
                if let Some(model) = self.model.as_mut() {
                    model.dialog = None;
                }
                self.draw()?;
                return Ok(answer);
            }
            if self.model().dialog.is_none() {
                let _ = self.step(Message::Event(Box::new(event.clone())));
            }
        }
    }

    /// Leaves the terminal as it was before the view: cleared, with the
    /// cursor at the top and shown.
    fn give_back(&mut self) -> io::Result<()> {
        self.frontend.give_back()
    }

    /// Hands the terminal over to `run`, such as hooks or the recovery
    /// shell, and draws the view again afterwards.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be handed over or drawn on again.
    pub fn suspend(&mut self, run: &mut dyn FnMut()) -> io::Result<()> {
        self.give_back()?;
        run();
        self.frontend.take_back()?;
        self.draw()
    }

    /// Ends the view: start-up is over.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be given back.
    pub fn close(mut self) -> io::Result<()> {
        self.give_back()
    }

    /// Serves start-up's stages, which send their events and questions to
    /// `mailbox` from their thread, until they close the view, or `running`
    /// says they ended. A question opens its dialog, whose answer goes back
    /// on its reply; one withdrawn closes it, unanswered. The view gives the
    /// terminal back before it returns, and the mailbox, dropped, tells the
    /// stages that nothing will answer any more.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be read or drawn on.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "the mailbox is dropped when the view ends, which tells the stages"
    )]
    pub fn serve(mut self, mailbox: Mailbox<ToView>, running: &dyn Fn() -> bool) -> io::Result<()> {
        let mut queue: VecDeque<ToView> = VecDeque::new();
        loop {
            if queue.is_empty() {
                let _ = zbm_sys::wait_readable(mailbox.as_fd(), Some(TICK));
                queue.extend(mailbox.take());
            }
            let Some(message) = queue.pop_front() else {
                if !running() {
                    return self.give_back();
                }
                self.draw()?;
                continue;
            };
            match message {
                ToView::Show(event) => self.send(*event)?,
                ToView::Ask { event, reply } => {
                    let mut withdrawn = || is_withdrawn(&mailbox, &mut queue);
                    let answer = self.ask_waking(&event, &mut withdrawn, Some(mailbox.as_fd()))?;
                    // A withdrawn question's answer goes to nobody.
                    reply.send(answer);
                }
                ToView::Withdraw | ToView::Resume => {}
                ToView::Lend { ready } => {
                    let mut ready = Some(ready);
                    self.suspend(&mut || {
                        if let Some(ready) = ready.take() {
                            ready.send(());
                        }
                        resumed(&mailbox, &mut queue, running);
                    })?;
                }
                ToView::Close { done } => {
                    self.give_back()?;
                    done.send(());
                    return Ok(());
                }
            }
        }
    }
}

/// Whether the question asked last was withdrawn: what came is queued, but
/// for the withdrawal, which the stages send while they wait for it, before
/// they ask anything else.
fn is_withdrawn(mailbox: &Mailbox<ToView>, queue: &mut VecDeque<ToView>) -> Option<Action> {
    queue.extend(mailbox.take());
    let at = queue
        .iter()
        .position(|message| matches!(message, ToView::Withdraw | ToView::Ask { .. }))?;
    if !matches!(queue.get(at), Some(ToView::Withdraw)) {
        return None;
    }
    queue.remove(at);
    Some(Action::Answer(Answer::Escape))
}

/// Waits until the stages take the terminal back, or end; what else came
/// meanwhile stays queued.
fn resumed(mailbox: &Mailbox<ToView>, queue: &mut VecDeque<ToView>, running: &dyn Fn() -> bool) {
    loop {
        queue.extend(mailbox.take());
        if let Some(at) = queue
            .iter()
            .position(|message| matches!(message, ToView::Resume))
        {
            queue.remove(at);
            return;
        }
        if !running() {
            return;
        }
        let _ = zbm_sys::wait_readable(mailbox.as_fd(), Some(TICK));
    }
}

impl<B: Surface, I: Input> StartupScreen for StartupView<B, I>
where
    B::Error: Send + Sync + 'static,
{
    fn serve(
        self: Box<Self>,
        mailbox: Mailbox<ToView>,
        running: &dyn Fn() -> bool,
    ) -> io::Result<()> {
        StartupView::serve(*self, mailbox, running)
    }
}

#[cfg(test)]
mod tests;
