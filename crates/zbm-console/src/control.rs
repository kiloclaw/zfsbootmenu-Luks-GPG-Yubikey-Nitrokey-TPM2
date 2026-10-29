//! The console's control socket (ROADMAP.md, PM7; ADR 0004): what other
//! processes ask it, each request with a reply once it is done
//! ([`zbm_app::console`] has the messages). A thread accepts the peers,
//! each answered on a thread of its own; [`Control`] is what they share
//! with the console's loop, which it wakes ([`Wake`]) when a request comes
//! and which says when the request is done: the display handed back or
//! taken again, the keyboard layout or the font's size changed.

use std::io;
use std::os::fd::{AsFd as _, BorrowedFd};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use zbm_app::console::{DONE_WAIT, Refusal, Reply, Request, Status};
use zbm_app::wire::{self, Unread};
use zbm_platform::event::{CancelToken, MailSender, Mailbox, spawn_traced};
use zbm_sys::socket::{Connection, Listener};

use crate::screen::zoom::Zoom;

/// How many peers are answered at once; more are told the console is
/// busy.
const MAX_PEERS: usize = 16;

/// What wakes the console when a request comes: readable then; taking it
/// leaves it unreadable.
pub trait Wake {
    /// The descriptor to wait on.
    fn fd(&self) -> BorrowedFd<'_>;

    /// Takes what made it readable.
    fn take(&mut self);
}

impl<W: Wake + ?Sized> Wake for &mut W {
    fn fd(&self) -> BorrowedFd<'_> {
        (**self).fd()
    }

    fn take(&mut self) {
        (**self).take();
    }
}

/// Where the console is.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Mode {
    /// Not yet on the display.
    #[default]
    Starting,
    /// Drawing on the display.
    Shown,
    /// Relaying on the kernel console, having handed the display back.
    Released,
    /// Relaying on the kernel console for good: the display cannot be had.
    Relaying,
}

/// What the loop and the peers share.
#[derive(Debug, Default)]
struct Shared {
    mode: Mode,
    layout: String,
    font: String,
    /// The display is asked for back.
    release: bool,
    /// The display is asked for again.
    reattach: bool,
    /// The keyboard layout asked for, and the reply once it was set.
    layout_asked: Option<String>,
    layout_reply: Option<Reply>,
    /// The font's size asked for, and the font once it was shown.
    font_asked: Option<Zoom>,
    font_reply: Option<String>,
    /// How many peers hold a secret prompt.
    secrets: usize,
    /// The console is ending: nothing more will be done.
    ended: bool,
}

/// What the console's loop and its control socket share.
pub struct Control {
    shared: Mutex<Shared>,
    changed: Condvar,
    wake: MailSender<()>,
    /// Held while a request that the loop does is answered, so that the
    /// replies of two peers do not mix.
    exchange: Mutex<()>,
    /// How many peers are answered now.
    peers: AtomicUsize,
    /// How long a request waits for the loop: [`DONE_WAIT`].
    done_wait: Duration,
}

/// The console's end of the wake-up: readable when a request came.
pub struct ControlWake {
    mailbox: Mailbox<()>,
}

impl std::fmt::Debug for Control {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Control")
            .field("shared", &self.shared)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ControlWake {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ControlWake")
            .finish_non_exhaustive()
    }
}

impl Wake for ControlWake {
    fn fd(&self) -> BorrowedFd<'_> {
        self.mailbox.as_fd()
    }

    fn take(&mut self) {
        let _ = self.mailbox.take();
    }
}

impl Control {
    /// The shared state, and the wake-up of the console's loop.
    ///
    /// # Errors
    ///
    /// When the wake-up cannot be made.
    pub fn new() -> io::Result<(Arc<Control>, ControlWake)> {
        Control::waiting(DONE_WAIT)
    }

    /// The control, whose requests wait `wait` for the loop instead of
    /// [`DONE_WAIT`].
    ///
    /// # Errors
    ///
    /// When the wake-up cannot be made.
    pub fn waiting(wait: Duration) -> io::Result<(Arc<Control>, ControlWake)> {
        let mailbox = Mailbox::new()?;
        let control = Control {
            shared: Mutex::default(),
            changed: Condvar::new(),
            wake: mailbox.sender(),
            exchange: Mutex::new(()),
            peers: AtomicUsize::new(0),
            done_wait: wait,
        };
        Ok((Arc::new(control), ControlWake { mailbox }))
    }

    /// The shared state, whatever a thread that panicked left in it.
    fn shared(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the shared state with `change`, and tells the peers waiting.
    fn change(&self, change: impl FnOnce(&mut Shared)) {
        change(&mut self.shared());
        self.changed.notify_all();
    }

    /// Wakes the console's loop.
    fn wake(&self) {
        // A full mailbox wakes it already.
        let _ = self.wake.send(());
    }

    // What the console's loop does.

    /// Whether the display is asked for back.
    pub fn is_release_asked(&self) -> bool {
        self.shared().release
    }

    /// Whether the display is asked for again, after it was handed back.
    pub fn is_reattach_asked(&self) -> bool {
        self.shared().reattach
    }

    /// The console draws on the display, typing with `layout` and drawing
    /// with `font`.
    pub fn shown(&self, layout: &str, font: &str) {
        self.change(|shared| {
            shared.mode = Mode::Shown;
            shared.reattach = false;
            layout.clone_into(&mut shared.layout);
            font.clone_into(&mut shared.font);
        });
    }

    /// The display is handed back.
    pub fn released(&self) {
        self.change(|shared| {
            shared.mode = Mode::Released;
            shared.release = false;
        });
    }

    /// The console relays on the kernel console for good.
    pub fn relaying(&self) {
        self.change(|shared| {
            shared.mode = Mode::Relaying;
            shared.release = false;
            shared.reattach = false;
        });
    }

    /// The keyboard layout asked for, which the loop takes.
    pub fn take_layout(&self) -> Option<String> {
        self.shared().layout_asked.take()
    }

    /// The keyboards type with `layout` now.
    pub fn layout_set(&self, layout: &str) {
        self.change(|shared| layout.clone_into(&mut shared.layout));
    }

    /// Answers the request for the layout `name`: the keyboards type with
    /// `layout` now, which is `name` when the image had it.
    pub fn layout_answered(&self, name: &str, layout: &str) {
        self.change(|shared| {
            layout.clone_into(&mut shared.layout);
            shared.layout_reply = Some(if name == layout {
                Reply::LayoutSet(name.to_owned())
            } else {
                Reply::NoSuchLayout(name.to_owned())
            });
        });
    }

    /// The font's size asked for, which the loop takes.
    pub fn take_font(&self) -> Option<Zoom> {
        self.shared().font_asked.take()
    }

    /// The console draws with `font` now, which answers a request for a
    /// size.
    pub fn font_shown(&self, font: &str) {
        self.change(|shared| {
            font.clone_into(&mut shared.font);
            shared.font_reply = Some(font.to_owned());
        });
    }

    /// Whether a process holds a secret prompt.
    pub fn is_secret(&self) -> bool {
        self.shared().secrets > 0
    }

    /// The console ends: the peers waiting are told it is busy.
    pub fn end(&self) {
        self.change(|shared| shared.ended = true);
    }

    // What the peers ask.

    /// How the console is.
    fn status(&self) -> Status {
        let shared = self.shared();
        Status {
            shown: shared.mode == Mode::Shown,
            layout: shared.layout.clone(),
            font: shared.font.clone(),
        }
    }

    /// Waits at most [`Control::done_wait`] until `is_done`; the state
    /// then, `None` when it is not done, or the console ends.
    fn until(&self, is_done: impl Fn(&Shared) -> bool) -> Option<MutexGuard<'_, Shared>> {
        let (shared, _) = self
            .changed
            .wait_timeout_while(self.shared(), self.done_wait, |shared| {
                !is_done(shared) && !shared.ended
            })
            .unwrap_or_else(PoisonError::into_inner);
        is_done(&shared).then_some(shared)
    }

    /// Answers `request`, once the loop did what it asks.
    pub fn answer(&self, request: &Request) -> Reply {
        match request {
            Request::Status => Reply::Status(self.status()),
            Request::Secret => Reply::Refused(Refusal::Malformed),
            _ => {
                let _one = self.exchange.lock().unwrap_or_else(PoisonError::into_inner);
                self.done(request).unwrap_or(Reply::Refused(Refusal::Busy))
            }
        }
    }

    /// Asks the loop for `request`, and waits until it did it; `None` when
    /// it did not in time.
    fn done(&self, request: &Request) -> Option<Reply> {
        match request {
            Request::Release => self.released_when_done(),
            Request::Reattach => self.reattached_when_done(),
            Request::Layout(name) => self.layout_when_set(name),
            Request::Font(size) => match Zoom::parse(size) {
                Some(zoom) => self.font_when_shown(zoom),
                None => Some(Reply::Refused(Refusal::Malformed)),
            },
            Request::Status | Request::Secret => Some(self.answer(request)),
        }
    }

    /// Asks for the display back, and waits until it is handed back.
    fn released_when_done(&self) -> Option<Reply> {
        if matches!(self.shared().mode, Mode::Shown | Mode::Starting) {
            self.change(|shared| shared.release = true);
            self.wake();
            drop(self.until(|shared| matches!(shared.mode, Mode::Released | Mode::Relaying))?);
        }
        Some(Reply::Released)
    }

    /// Asks for the display again, and waits until it is taken, or cannot
    /// be.
    fn reattached_when_done(&self) -> Option<Reply> {
        if self.shared().mode != Mode::Released {
            return Some(Reply::Reattached);
        }
        self.change(|shared| shared.reattach = true);
        let shared = self.until(|shared| shared.mode != Mode::Released)?;
        Some(if shared.mode == Mode::Shown {
            Reply::Reattached
        } else {
            Reply::Refused(Refusal::NotShown)
        })
    }

    /// Asks for the layout `name`, and waits until the keyboards have it,
    /// or kept theirs.
    fn layout_when_set(&self, name: &str) -> Option<Reply> {
        let asked = self.is_asked(|shared| {
            shared.layout_asked = Some(name.to_owned());
            shared.layout_reply = None;
        });
        if !asked {
            return Some(Reply::Refused(Refusal::NotShown));
        }
        let mut shared =
            self.until(|shared| shared.layout_reply.is_some() || shared.mode != Mode::Shown)?;
        let reply = shared.layout_reply.take();
        Some(reply.unwrap_or(Reply::Refused(Refusal::NotShown)))
    }

    /// Asks for the font's size `zoom`, and waits until a font is shown.
    fn font_when_shown(&self, zoom: Zoom) -> Option<Reply> {
        let asked = self.is_asked(|shared| {
            shared.font_asked = Some(zoom);
            shared.font_reply = None;
        });
        if !asked {
            return Some(Reply::Refused(Refusal::NotShown));
        }
        let mut shared =
            self.until(|shared| shared.font_reply.is_some() || shared.mode != Mode::Shown)?;
        let reply = shared.font_reply.take();
        Some(reply.map_or(Reply::Refused(Refusal::NotShown), Reply::FontSet))
    }

    /// Asks the loop for something with `ask`, when the console is on the
    /// display, and wakes it; whether it is.
    fn is_asked(&self, ask: impl FnOnce(&mut Shared)) -> bool {
        let mut shared = self.shared();
        if shared.mode != Mode::Shown {
            return false;
        }
        ask(&mut shared);
        drop(shared);
        self.wake();
        true
    }

    /// Notes the secret prompt of `peer` until it closes the connection.
    fn hold(&self, peer: &Connection) {
        self.change(|shared| shared.secrets += 1);
        self.wake();
        if peer.send(&Reply::SecretHeld.encode()).is_ok() {
            // Whatever it sends is ignored, until it is gone.
            while matches!(peer.receive(None), Ok(Some(bytes)) if !bytes.is_empty()) {}
        }
        self.change(|shared| shared.secrets = shared.secrets.saturating_sub(1));
        self.wake();
    }

    /// Answers `peer`, which must be of `user`.
    fn answer_peer(&self, peer: &Connection, user: u32) {
        let request = match wire::request_of(peer, user) {
            Ok(bytes) => Request::decode(&bytes),
            Err(Unread::Stranger) => Err(Refusal::NotRoot),
            Err(Unread::Nothing) => Err(Refusal::Malformed),
        };
        let reply = match request {
            Ok(Request::Secret) => return self.hold(peer),
            Ok(request) => self.answer(&request),
            Err(refusal) => Reply::Refused(refusal),
        };
        // A peer that went before the reply needs none.
        let _ = peer.send(&reply.encode());
    }
}

/// What the console's own tests ask, without a socket.
#[cfg(test)]
impl Control {
    /// Asks the loop to hand the display back.
    pub(crate) fn ask_release(&self) {
        self.change(|shared| shared.release = true);
        self.wake();
    }

    /// Asks the relay for the display again.
    pub(crate) fn ask_reattach(&self) {
        self.change(|shared| shared.reattach = true);
    }

    /// Asks the loop for the layout `name`.
    pub(crate) fn ask_layout(&self, name: &str) {
        self.change(|shared| shared.layout_asked = Some(name.to_owned()));
        self.wake();
    }

    /// Asks the loop for a font's size.
    pub(crate) fn ask_font(&self, zoom: Zoom) {
        self.change(|shared| shared.font_asked = Some(zoom));
        self.wake();
    }

    /// Sets how many peers hold a secret prompt.
    pub(crate) fn set_secrets(&self, secrets: usize) {
        self.change(|shared| shared.secrets = secrets);
    }

    /// The replies the loop gave to a layout and a font's size.
    pub(crate) fn replies(&self) -> (Option<Reply>, Option<String>) {
        let shared = self.shared();
        (shared.layout_reply.clone(), shared.font_reply.clone())
    }
}

/// Answers the peers that connect to `listener`, each on a thread of its
/// own, until `until` is cancelled; only those of this process's user,
/// root, are answered.
pub fn serve(control: &Arc<Control>, listener: &Listener, until: &CancelToken) {
    let user = zbm_sys::credentials().uid;
    wire::accept_until(listener, until, "console", |peer| {
        if control.peers.fetch_add(1, Ordering::SeqCst) >= MAX_PEERS {
            control.peers.fetch_sub(1, Ordering::SeqCst);
            let _ = peer.send(&Reply::Refused(Refusal::Busy).encode());
            return;
        }
        let answering = Arc::clone(control);
        let started = spawn_traced("console-peer", move || {
            answering.answer_peer(&peer, user);
            answering.peers.fetch_sub(1, Ordering::SeqCst);
        });
        // Without a thread, the peer is dropped, which closes it.
        if started.is_err() {
            control.peers.fetch_sub(1, Ordering::SeqCst);
        }
    });
}

#[cfg(test)]
mod tests;
