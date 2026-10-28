//! Writing a child's input and reading its output streams, up to a limit.

use std::io::{self, Read, Write};
use std::os::fd::AsFd;

use super::supervise::{Supervised, Supervisor};
use super::{Command, Input, Stream};
use crate::secret::Secret;

/// A pipe a child writes to.
pub(super) trait Pipe: Read + AsFd + Send {}

impl<T: Read + AsFd + Send> Pipe for T {}

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

/// What a child wrote to its standard output and standard error.
pub(super) type Streams = (
    Option<io::Result<Stdout>>,
    Option<io::Result<Captured<Vec<u8>>>>,
);

/// Gives `child` the input of `command` and reads its output.
///
/// Standard input is written, and standard error read, by other threads, so
/// that a command that writes a lot before reading all of its input, or fills
/// one pipe while this thread reads the other, cannot block.
/// With a `supervisor`, standard output is read only when there is
/// something to read, and ends when it kills the child.
pub(super) fn communicate(
    command: &Command,
    child: &mut std::process::Child,
    merged: Option<io::PipeReader>,
    supervisor: Option<&Supervisor>,
) -> Streams {
    let stdin = child.stdin.take();
    let stdout: Option<Box<dyn Pipe>> = match merged {
        Some(reader) => Some(Box::new(reader)),
        None => child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Pipe>),
    };
    let stderr = child.stderr.take();
    let secret_stdout = command.stdout == Stream::CaptureSecret;
    std::thread::scope(|scope| {
        if let Some(mut pipe) = stdin {
            let bytes = match &command.stdin {
                Input::Bytes(bytes) => bytes.as_slice(),
                Input::Secret(secret) => secret.expose(),
                Input::Inherit | Input::Null => &[],
            };
            // A command may exit without reading all of its input; that is not
            // an error here. Closing the pipe ends the input.
            crate::event::spawn_scoped_traced(scope, move || {
                let _ = pipe.write_all(bytes);
            });
        }
        let stderr_limit = command.get_output_limit(Stream::Capture);
        let stderr = stderr.map(|pipe| {
            crate::event::spawn_scoped_traced(scope, move || read_pipe(pipe, stderr_limit))
        });
        let stdout = stdout.map(|pipe| {
            let pipe: Box<dyn Read> = match supervisor {
                Some(supervisor) => Box::new(Supervised {
                    reader: pipe,
                    supervisor,
                }),
                None => pipe,
            };
            if secret_stdout {
                read_secret(pipe, command.get_output_limit(Stream::CaptureSecret))
                    .map(Stdout::Secret)
            } else {
                read_pipe(pipe, command.get_output_limit(Stream::Capture)).map(Stdout::Plain)
            }
        });
        let stderr = stderr.map(|reader| {
            reader
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("reading standard error failed")))
        });
        (stdout, stderr)
    })
}
