//! [`Mailbox`]: how workers send messages to the reactor.

use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SendError, SyncSender, TryRecvError, sync_channel};

use rustix::event::{EventfdFlags, eventfd};

/// How many messages a mailbox holds before a sender waits (ADR 0001): a
/// flood of events slows the worker that sends them instead of growing
/// memory.
pub const MAILBOX_BOUND: usize = 256;

/// The reactor's end of a channel of messages of type `T`: an `eventfd`,
/// readable while messages wait, which wakes the reactor's `poll`, paired
/// with a channel bounded at [`MAILBOX_BOUND`].
pub struct Mailbox<T> {
    receiver: Receiver<T>,
    sender: SyncSender<T>,
    wake: Arc<OwnedFd>,
}

/// A worker's end of a [`Mailbox`].
pub struct MailSender<T> {
    sender: SyncSender<T>,
    wake: Arc<OwnedFd>,
}

impl<T> Clone for MailSender<T> {
    fn clone(&self) -> MailSender<T> {
        MailSender {
            sender: self.sender.clone(),
            wake: Arc::clone(&self.wake),
        }
    }
}

impl<T> Mailbox<T> {
    /// An empty mailbox.
    ///
    /// # Errors
    ///
    /// When the `eventfd` cannot be created.
    pub fn new() -> io::Result<Mailbox<T>> {
        let wake = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)?;
        let (sender, receiver) = sync_channel(MAILBOX_BOUND);
        Ok(Mailbox {
            receiver,
            sender,
            wake: Arc::new(wake),
        })
    }

    /// A sender, for a worker.
    pub fn sender(&self) -> MailSender<T> {
        MailSender {
            sender: self.sender.clone(),
            wake: Arc::clone(&self.wake),
        }
    }

    /// The messages that came, in the order they were sent; afterwards the
    /// mailbox is not readable until another comes.
    pub fn take(&self) -> Vec<T> {
        // The counter first: a message sent after it is read sets it again.
        let mut counter = [0_u8; 8];
        let _ = rustix::io::read(&*self.wake, &mut counter);
        let mut messages = Vec::new();
        loop {
            match self.receiver.try_recv() {
                Ok(message) => messages.push(message),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return messages,
            }
        }
    }
}

impl<T> AsFd for Mailbox<T> {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.wake.as_fd()
    }
}

impl<T> MailSender<T> {
    /// Sends `message`, waiting while the mailbox is full.
    ///
    /// # Errors
    ///
    /// The message, when the mailbox is gone.
    pub fn send(&self, message: T) -> Result<(), SendError<T>> {
        self.sender.send(message)?;
        let _ = rustix::io::write(&*self.wake, &1_u64.to_ne_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::event::reactor::readable;
    use crate::event::spawn_traced;

    #[test]
    fn messages_wake_the_reactor_and_come_in_order() {
        let mailbox = Mailbox::new().unwrap();
        assert!(!readable(&mailbox, Some(Duration::ZERO)).unwrap());
        let sender = mailbox.sender();
        let worker = spawn_traced("sender", move || {
            for number in 0..3 {
                sender.clone().send(number).unwrap();
            }
        })
        .unwrap();
        worker.join().unwrap();
        assert!(readable(&mailbox, Some(Duration::ZERO)).unwrap());
        assert_eq!(mailbox.take(), [0, 1, 2]);
        assert!(!readable(&mailbox, Some(Duration::ZERO)).unwrap());
        assert!(mailbox.take().is_empty());
    }

    #[test]
    fn a_sender_waits_while_the_mailbox_is_full() {
        let mailbox = Mailbox::new().unwrap();
        let sender = mailbox.sender();
        let worker = spawn_traced("flood", move || {
            for number in 0..=MAILBOX_BOUND {
                sender.send(number).unwrap();
            }
        })
        .unwrap();
        let mut received = Vec::new();
        while received.len() <= MAILBOX_BOUND {
            if readable(&mailbox, Some(Duration::from_secs(5))).unwrap() {
                received.extend(mailbox.take());
            }
        }
        worker.join().unwrap();
        assert_eq!(received, (0..=MAILBOX_BOUND).collect::<Vec<_>>());
    }

    #[test]
    fn sending_to_a_mailbox_that_is_gone_fails() {
        let mailbox = Mailbox::<u8>::new().unwrap();
        let sender = mailbox.sender();
        drop(mailbox);
        assert_eq!(sender.send(1).unwrap_err().0, 1);
    }
}
