//! Reading a child's output streams, up to a limit.

use std::io::{self, Read};

use crate::secret::Secret;

/// Reads a pipe to its end, or to `limit` bytes: whether it had more is
/// [`Captured::over`]. The pipe is closed when it is dropped, so a program
/// that writes on gets `SIGPIPE` or `EPIPE` instead of waiting for a reader.
pub(super) fn read_pipe(pipe: impl Read, limit: usize) -> io::Result<Captured<Vec<u8>>> {
    let mut bytes = Vec::new();
    pipe.take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)?;
    let over = bytes.len() > limit;
    bytes.truncate(limit);
    Ok(Captured { data: bytes, over })
}

/// Reads a pipe into a secret, as [`read_pipe`] does.
pub(super) fn read_secret(mut pipe: impl Read, limit: usize) -> io::Result<Captured<Secret>> {
    let take = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let secret = Secret::read_from(&mut (&mut pipe).take(take))?;
    let over = secret.len() > limit;
    Ok(Captured { data: secret, over })
}

/// What was read of a pipe, and whether it held more than was taken.
pub(super) struct Captured<T> {
    /// What was read.
    pub(super) data: T,
    /// Whether the pipe held more.
    pub(super) over: bool,
}

/// Captured standard output.
pub(super) enum Stdout {
    /// Plain bytes.
    Plain(Captured<Vec<u8>>),
    /// A secret.
    Secret(Captured<Secret>),
}
