//! The parts of the reactor of a process ([ADR 0001]): one thread owns the
//! process's state and waits in one `poll` ([`Reactor`]) for its sources of
//! input, among them the [`Mailbox`] through which its workers, started
//! with [`spawn_traced`], send back what they did. Long work can be
//! cancelled ([`CancelToken`]), and parallel work is bounded in threads
//! ([`WorkPool`]) and in memory ([`MemoryBudget`]).
//!
//! [ADR 0001]: ../../../docs/dev/adr/0001-process-model.md

mod budget;
mod cancel;
mod mailbox;
mod pool;
pub(crate) mod reactor;
mod spawn;

pub use budget::{MemoryBudget, Permit};
pub use cancel::{CancelToken, Cancelled};
pub use mailbox::{MAILBOX_BOUND, MailSender, Mailbox};
pub use pool::WorkPool;
pub use reactor::{Reactor, Wake};
pub use spawn::{spawn_scoped_traced, spawn_traced};
