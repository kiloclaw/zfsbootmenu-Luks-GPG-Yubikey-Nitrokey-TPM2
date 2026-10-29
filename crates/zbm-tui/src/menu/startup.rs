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
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::layout::{Position, Rect, Size};
use zbm_display::keys::KeyPress;
use zbm_platform::clock::{Clock, SystemClock};
use zbm_platform::event::Mailbox;
use zbm_types::Answer;

use super::model::Model;
use super::secret::{Marker, is_asking_secret};
use super::theme::Theme;
use super::update::{Message, update};
use super::view::view;
use super::world::{Action, Event, Snapshot};
use crate::quiet::QuietBackend;
pub use link::{MailLink, ToView, ViewLink};

/// How long the view waits for a key before time passes for its dialog.
pub const TICK: Duration = Duration::from_millis(200);

/// The keys of the terminal the view draws on, its mode and its size.
pub trait Keys {
    /// The next key pressed within `timeout`; `None` when none was.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be read.
    fn key(&mut self, timeout: Duration) -> io::Result<Option<KeyPress>>;

    /// As [`Keys::key`], and no longer once `wake` is readable, as when a
    /// message came: then `None`. Keys that come at once ignore it.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be read.
    fn key_or_wake(
        &mut self,
        timeout: Duration,
        wake: Option<BorrowedFd<'_>>,
    ) -> io::Result<Option<KeyPress>> {
        let _ = wake;
        self.key(timeout)
    }

    /// Gives the terminal back to programs, as it was before the view.
    ///
    /// # Errors
    ///
    /// When its mode cannot be set.
    fn release(&mut self) -> io::Result<()>;

    /// Takes the terminal again, after [`Keys::release`].
    ///
    /// # Errors
    ///
    /// When its mode cannot be set.
    fn take(&mut self) -> io::Result<()>;

    /// The terminal's size now, in columns and rows, if it can be measured.
    /// It changes when a program that had the terminal changed the console
    /// font, as `30-console-autosize` does, or when its window was resized.
    fn size(&mut self) -> Option<(u16, u16)>;
}

/// A backend that is told the size of the terminal it draws on, as the
/// view measures it ([`Keys::size`]).
pub trait Resize: Backend {
    /// Records the terminal's new size.
    fn set_size(&mut self, size: Size);
}

impl<B: Backend> Resize for QuietBackend<B> {
    fn set_size(&mut self, size: Size) {
        QuietBackend::set_size(self, size);
    }
}

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

/// The start-up view on a terminal.
pub struct StartupView<B: Backend, K: Keys> {
    model: Option<Model>,
    terminal: Terminal<B>,
    keys: K,
    /// Where the view records its secret prompts for the console on the
    /// display, if anywhere.
    marker: Option<Marker>,
    /// The clock the dialogs' time passes on.
    clock: Box<dyn Clock>,
}

/// Whether `action` answers a dialog of start-up.
fn is_answer(action: &Action) -> bool {
    matches!(
        action,
        Action::Unlock { .. } | Action::SkipUnlock(_) | Action::Answer(_)
    )
}

/// A result of the backend `B`, with its error as an I/O error.
fn drawn<T, E>(result: Result<T, E>) -> io::Result<T>
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    result.map_err(io::Error::other)
}

impl<B: Resize, K: Keys> StartupView<B, K>
where
    B::Error: Send + Sync + 'static,
{
    /// The view of `snapshot`, drawn on `terminal`, which `keys` reads.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be drawn on.
    pub fn new(
        terminal: Terminal<B>,
        keys: K,
        snapshot: Snapshot,
        theme: Theme,
    ) -> io::Result<StartupView<B, K>> {
        let mut view = StartupView {
            model: Some(Model::new(snapshot, theme)),
            terminal,
            keys,
            marker: None,
            clock: Box::new(SystemClock::new()),
        };
        drawn(view.terminal.clear())?;
        view.draw()?;
        Ok(view)
    }

    /// The view, which records its secret prompts with `marker` for the
    /// console on the display.
    #[must_use]
    pub fn with_marker(mut self, marker: Marker) -> StartupView<B, K> {
        self.marker = Some(marker);
        self
    }

    /// The view, which measures the time of its dialogs on `clock`.
    #[must_use]
    pub fn with_clock(mut self, clock: impl Clock + 'static) -> StartupView<B, K> {
        self.clock = Box::new(clock);
        self
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
        &self.terminal
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

    /// Draws the start-up view in the terminal's size now, unless start-up
    /// is over: the menu draws the main view.
    fn draw(&mut self) -> io::Result<()> {
        if self.model.as_ref().is_none_or(|model| model.started) {
            return Ok(());
        }
        self.fit()?;
        let Some(model) = self.model.as_ref() else {
            return Ok(());
        };
        drawn(self.terminal.draw(|frame| view(model, frame)))?;
        if let Some(marker) = &mut self.marker {
            marker.show(is_asking_secret(model));
        }
        Ok(())
    }

    /// Follows the terminal's size, when it changed since the last frame.
    fn fit(&mut self) -> io::Result<()> {
        let Some((columns, rows)) = self.keys.size().filter(|&(c, r)| c > 0 && r > 0) else {
            return Ok(());
        };
        let area = Rect::new(0, 0, columns, rows);
        if self.terminal.get_frame().area() == area {
            return Ok(());
        }
        self.terminal
            .backend_mut()
            .set_size(Size::new(columns, rows));
        drawn(self.terminal.resize(area))
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
        let mut last_tick = self.clock.now();
        loop {
            self.draw()?;
            // A read can end before the tick with no key, as noise on a
            // serial line does: the dialog's time is the time that passed.
            let passed = self.clock.now().saturating_sub(last_tick);
            let mut answers = match self.keys.key_or_wake(TICK.saturating_sub(passed), wake)? {
                Some(key) => self.step(Message::Key(key)),
                None => Vec::new(),
            };
            let passed = self.clock.now().saturating_sub(last_tick);
            if !answers.iter().any(is_answer) && passed >= TICK {
                last_tick = self.clock.now();
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
        if let Some(marker) = &mut self.marker {
            marker.show(false);
        }
        drawn(self.terminal.clear())?;
        drawn(self.terminal.set_cursor_position(Position::ORIGIN))?;
        drawn(self.terminal.show_cursor())?;
        self.keys.release()
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
        self.keys.take()?;
        drawn(self.terminal.clear())?;
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

impl<B: Resize, K: Keys> StartupScreen for StartupView<B, K>
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
