//! A runner whose commands one token cancels: what an action runs, so that
//! cancelling the action kills the program it waits for (ADR 0001, PM8).

use std::io;

use crate::event::CancelToken;
use crate::exec::{Command, Output, Runner};

/// A [`Runner`] that runs commands through another runner, each killed when
/// `token` is cancelled, unless it has a token of its own. Once the token
/// is cancelled, it starts nothing more: a program that changes a pool
/// must not start after the user cancelled the action that runs it.
#[derive(Clone)]
pub struct CancellingRunner<'a> {
    /// The runner that runs the commands.
    inner: &'a dyn Runner,
    /// What cancels them.
    token: CancelToken,
}

impl<'a> CancellingRunner<'a> {
    /// A runner of `inner`'s, its commands cancelled by `token`.
    pub fn new(inner: &'a dyn Runner, token: CancelToken) -> CancellingRunner<'a> {
        CancellingRunner { inner, token }
    }

    /// The token that cancels its commands.
    pub fn token(&self) -> &CancelToken {
        &self.token
    }
}

impl Runner for CancellingRunner<'_> {
    /// # Errors
    ///
    /// [`io::ErrorKind::Interrupted`] when the token was cancelled before
    /// the command started; otherwise as `inner`.
    fn run(&self, command: &Command) -> io::Result<Output> {
        if self.token.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                crate::event::Cancelled,
            ));
        }
        if command.get_cancel().is_some() {
            return self.inner.run(command);
        }
        self.inner.run(&command.clone().cancel(&self.token))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::exec::Status;

    /// Records whether each command came with a token.
    #[derive(Default)]
    struct Recording(Mutex<Vec<bool>>);

    impl Runner for Recording {
        fn run(&self, command: &Command) -> io::Result<Output> {
            if let Ok(mut seen) = self.0.lock() {
                seen.push(command.get_cancel().is_some());
            }
            Ok(Output::new(Status::exited(0), Vec::new()))
        }
    }

    #[test]
    fn commands_get_the_token_and_none_starts_once_it_is_cancelled() {
        let recording = Recording::default();
        let token = CancelToken::new().unwrap();
        let runner = CancellingRunner::new(&recording, token.clone());
        let own = CancelToken::new().unwrap();

        runner.run(&Command::new("zfs")).unwrap();
        runner.run(&Command::new("zfs").cancel(&own)).unwrap();
        assert_eq!(runner.token(), &token);
        token.cancel();
        let error = runner.run(&Command::new("zfs")).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(*recording.0.lock().unwrap(), [true, true]);
    }
}
