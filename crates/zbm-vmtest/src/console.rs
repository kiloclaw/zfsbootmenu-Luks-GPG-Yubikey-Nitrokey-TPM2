//! A serial console: a thread reads it into a log file, a terminal emulator
//! and the text that [`Console::expect`] matches.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::expect::{self, Found, Matcher};
use crate::text::{Decoder, clean};

/// The least time between two searches of the text while waiting, so that
/// a console that sends a byte at a time does not cost a search per byte.
const SEARCH_INTERVAL: Duration = Duration::from_millis(10);

/// How much of the console an error message shows.
const TAIL: usize = 300;

/// The size of the terminal emulator's screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    /// Lines.
    pub rows: u16,
    /// Columns.
    pub cols: u16,
}

impl Default for Size {
    fn default() -> Size {
        Size { rows: 24, cols: 80 }
    }
}

/// What the reader thread shares with the test.
struct Shared {
    /// The text not consumed by a match yet.
    text: String,
    /// Whether the console was closed.
    eof: bool,
    /// The bytes read so far.
    bytes: u64,
    screen: vt100::Parser,
    decoder: Decoder,
}

type State = Arc<(Mutex<Shared>, Condvar)>;

fn lock(state: &State) -> MutexGuard<'_, Shared> {
    state.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A console being read.
pub struct Console {
    writer: Box<dyn Write + Send>,
    state: State,
    reader: Option<JoinHandle<()>>,
    patterns: HashMap<String, Matcher>,
    before: String,
    found: Found,
}

impl Console {
    /// Starts reading `reader` into `log` and the screen; `writer` is the
    /// console's input.
    pub fn attach(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        log: impl Write + Send + 'static,
        size: Size,
    ) -> Console {
        let rows = NonZeroU16::new(size.rows).unwrap_or(NonZeroU16::MIN);
        let cols = NonZeroU16::new(size.cols).unwrap_or(NonZeroU16::MIN);
        let state: State = Arc::new((
            Mutex::new(Shared {
                text: String::new(),
                eof: false,
                bytes: 0,
                screen: vt100::Parser::new(rows, cols, 0),
                decoder: Decoder::default(),
            }),
            Condvar::new(),
        ));
        let thread_state = Arc::clone(&state);
        let reader = thread::spawn(move || read_console(reader, log, &thread_state));
        Console {
            writer: Box::new(writer),
            state,
            reader: Some(reader),
            patterns: HashMap::new(),
            before: String::new(),
            found: Found::default(),
        }
    }

    /// Sends `text` as typed.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the console is closed.
    pub fn send(&mut self, text: &str) -> Result<()> {
        self.writer
            .write_all(text.as_bytes())
            .and_then(|()| self.writer.flush())
            .map_err(Error::io("writing to the console"))
    }

    /// Sends `text` and a newline.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the console is closed.
    pub fn sendline(&mut self, text: &str) -> Result<()> {
        self.send(&format!("{text}\n"))
    }

    /// Waits up to `timeout` for one of `patterns` (regular expressions);
    /// returns the index of the one that matched. The text up to the end of
    /// the match is consumed: [`Console::before`] holds what came before it,
    /// [`Console::group`] its groups. A pattern that starts with
    /// [`expect::SCREEN`] is matched against the screen, once no pattern
    /// matches the text; its match consumes all the text.
    ///
    /// # Errors
    ///
    /// [`Error::Timeout`] when no pattern matched in time, [`Error::Eof`]
    /// when the console closed first, [`Error::Pattern`] for an invalid
    /// pattern.
    pub fn expect(&mut self, patterns: &[&str], timeout: Duration) -> Result<usize> {
        let regexes = self.compile(patterns)?;
        let deadline = Instant::now() + timeout;
        let mut searched = 0;
        let mut guard = lock(&self.state);
        loop {
            let from = expect::search_start(&guard.text, searched);
            if let Some(found) = expect::earliest_text(&regexes, &guard.text, from) {
                self.before = guard.text[..found.start].to_owned();
                guard.text.drain(..found.end);
                let index = found.index;
                self.found = found;
                return Ok(index);
            }
            // What the screen shows takes all the text so far.
            let screen = guard.screen.screen().contents();
            if let Some(found) = expect::on_screen(&regexes, &screen) {
                self.before = std::mem::take(&mut guard.text);
                let index = found.index;
                self.found = found;
                return Ok(index);
            }
            searched = guard.text.len();
            let now = Instant::now();
            if guard.eof || now >= deadline {
                let tail = tail(&guard.text);
                let patterns = patterns.iter().map(|&pattern| pattern.to_owned()).collect();
                return Err(if guard.eof {
                    Error::Eof { patterns, tail }
                } else {
                    Error::Timeout {
                        patterns,
                        seconds: timeout.as_secs(),
                        tail,
                    }
                });
            }
            drop(guard);
            thread::sleep(SEARCH_INTERVAL.min(deadline - now));
            guard = wait_for_more(&self.state, searched, deadline);
        }
    }

    /// Waits up to `timeout` for the console to close. What it showed
    /// until then is [`Console::before`].
    ///
    /// # Errors
    ///
    /// [`Error::Timeout`] when it is still open.
    pub fn expect_eof(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let mut guard = lock(&self.state);
        while !guard.eof {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return Err(Error::Timeout {
                    patterns: vec!["the end of the console".into()],
                    seconds: timeout.as_secs(),
                    tail: tail(&guard.text),
                });
            };
            guard = self
                .state
                .1
                .wait_timeout(guard, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        self.before = std::mem::take(&mut guard.text);
        Ok(())
    }

    fn compile(&mut self, patterns: &[&str]) -> Result<Vec<Matcher>> {
        patterns
            .iter()
            .map(|&pattern| {
                if let Some(matcher) = self.patterns.get(pattern) {
                    return Ok(matcher.clone());
                }
                let matcher = Matcher::new(pattern)?;
                self.patterns.insert(pattern.to_owned(), matcher.clone());
                Ok(matcher)
            })
            .collect()
    }

    /// The text before the last match, without carriage returns.
    pub fn before(&self) -> String {
        clean(&self.before)
    }

    /// Group `n` of the last match (0: all of it); empty when it did not
    /// take part.
    pub fn group(&self, n: usize) -> &str {
        self.found
            .groups
            .get(n)
            .and_then(Option::as_deref)
            .unwrap_or_default()
    }

    /// The text the screen shows, line by line.
    pub fn screen(&self) -> String {
        lock(&self.state).screen.screen().contents()
    }

    /// The number of bytes the console sent so far.
    pub fn bytes_read(&self) -> u64 {
        lock(&self.state).bytes
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        // The reader ends when the other end closes the console.
        if let Some(reader) = self.reader.take()
            && lock(&self.state).eof
        {
            reader.join().ok();
        }
    }
}

/// Waits until the console has more text than `len`, closes, or the
/// deadline passes.
fn wait_for_more(state: &State, len: usize, deadline: Instant) -> MutexGuard<'_, Shared> {
    let mut guard = lock(state);
    while guard.text.len() == len && !guard.eof {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        guard = state
            .1
            .wait_timeout(guard, left)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
    guard
}

/// The reader thread: until the console closes.
fn read_console(mut reader: impl Read, mut log: impl Write, state: &State) {
    let mut buffer = [0_u8; 4096];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let bytes = &buffer[..read];
        log.write_all(bytes).and_then(|()| log.flush()).ok();
        let mut guard = lock(state);
        let shared = &mut *guard;
        shared.screen.process(bytes);
        shared.decoder.feed(bytes, &mut shared.text);
        shared.bytes += read as u64;
        drop(guard);
        state.1.notify_all();
    }
    lock(state).eof = true;
    state.1.notify_all();
}

/// The last characters of `text`, for messages.
fn tail(text: &str) -> String {
    let start = text
        .char_indices()
        .rev()
        .nth(TAIL)
        .map_or(0, |(index, _)| index);
    text[start..].to_owned()
}

#[cfg(test)]
mod tests;
