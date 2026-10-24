//! Secrets: passphrases, PINs and keys.
//!
//! A [`Secret`] holds bytes that must not outlive their use or leak into a log:
//! they live in pages of their own ([`zbm_sys::memory::Pages`]), which are
//! never swapped out, stay out of core dumps and child processes, and, where
//! the kernel allows it, out of the kernel's map of memory; they are wiped when
//! the secret is dropped, they grow without leaving copies behind in freed
//! memory, and formatting a secret prints `Secret(..)`. See `crates/DESIGN.md`
//! for the rules secrets follow on their way to programs.

use std::fmt;
use std::io::{self, Read};

use zbm_sys::memory::{Pages, Protection};

/// Bytes that are wiped from memory when they are no longer needed.
#[derive(Default)]
pub struct Secret {
    /// The pages, made when the first byte arrives.
    pages: Option<Pages>,
    /// How many bytes of the pages are the secret.
    len: usize,
}

/// New pages for at least `bytes` bytes.
fn pages(bytes: usize) -> Pages {
    Pages::new_or_abort(bytes)
}

impl Secret {
    /// An empty secret.
    pub fn new() -> Secret {
        Secret::default()
    }

    /// Takes `bytes` into the secret's pages, and wipes them. Only the final
    /// allocation of the vector is wiped, so the bytes must not have been
    /// copied around (for example by growing the vector) before.
    pub fn from_vec(mut bytes: Vec<u8>) -> Secret {
        let secret = Secret::copy_from(&bytes);
        zbm_sys::wipe_vec(&mut bytes);
        secret
    }

    /// A secret with a copy of `bytes`; for tests and for values that are not
    /// secret on their own, such as a constant.
    pub fn copy_from(bytes: &[u8]) -> Secret {
        let mut secret = Secret::new();
        secret.push_bytes(bytes);
        secret
    }

    /// The secret bytes.
    pub fn expose(&self) -> &[u8] {
        self.pages
            .as_ref()
            .and_then(|pages| pages.bytes().get(..self.len))
            .unwrap_or_default()
    }

    /// The number of bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the secret is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// How the secret's memory is kept: `None` while it is empty and has no
    /// pages yet.
    pub fn protection(&self) -> Option<Protection> {
        self.pages.as_ref().map(Pages::protection)
    }

    /// The pages, with room for at least `additional` more bytes. Growing
    /// copies the bytes to new pages; the old ones are wiped as they go.
    fn room(&mut self, additional: usize) -> &mut Pages {
        let needed = self.len.saturating_add(additional);
        let capacity = self.pages.as_ref().map_or(0, Pages::len);
        if needed > capacity {
            let mut grown = pages(needed.max(capacity.saturating_mul(2)));
            if let (Some(to), Some(from)) = (grown.bytes_mut().get_mut(..self.len), &self.pages) {
                to.copy_from_slice(from.bytes().get(..self.len).unwrap_or_default());
            }
            self.pages = Some(grown);
        }
        self.pages.get_or_insert_with(|| pages(needed))
    }

    /// Appends `bytes`.
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let start = self.len;
        let pages = self.room(bytes.len());
        if let Some(to) = pages.bytes_mut().get_mut(start..start + bytes.len()) {
            to.copy_from_slice(bytes);
            self.len = start + bytes.len();
        }
    }

    /// Appends one byte.
    pub fn push(&mut self, byte: u8) {
        self.push_bytes(&[byte]);
    }

    /// Removes the last byte, as a backspace at a passphrase prompt does; the
    /// byte is wiped from the pages.
    pub fn pop(&mut self) -> Option<u8> {
        let last = self.len.checked_sub(1)?;
        let slot = self.pages.as_mut()?.bytes_mut().get_mut(last)?;
        let byte = *slot;
        zbm_sys::wipe(std::slice::from_mut(slot));
        self.len = last;
        Some(byte)
    }

    /// Removes a final newline (or carriage return and newline), as the shell's
    /// `read` and `$( ... )` do with the lines they read.
    pub fn trim_newline(&mut self) {
        for end in *b"\n\r" {
            if self.expose().last() == Some(&end) {
                self.pop();
            }
        }
    }

    /// Reads `reader` to its end into a new secret.
    ///
    /// # Errors
    ///
    /// The error of the reader; what was read is wiped.
    pub fn read_from(reader: &mut impl Read) -> io::Result<Secret> {
        let mut secret = Secret::new();
        let mut chunk = [0_u8; 512];
        let result = loop {
            match reader.read(&mut chunk) {
                Ok(0) => break Ok(()),
                Ok(count) => secret.push_bytes(chunk.get(..count).unwrap_or_default()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => break Err(error),
            }
        };
        zbm_sys::wipe(&mut chunk);
        result.map(|()| secret)
    }
}

/// How many secrets with bytes the process holds now, which it should not
/// just before `kexec`.
pub fn live_secrets() -> usize {
    zbm_sys::memory::live_regions()
}

impl Clone for Secret {
    fn clone(&self) -> Secret {
        Secret::copy_from(self.expose())
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Secret) -> bool {
        self.expose() == other.expose()
    }
}

impl Eq for Secret {}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Not even the length: it tells how long a passphrase is.
        formatter.write_str("Secret(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_never_shows_the_bytes() {
        let secret = Secret::copy_from(b"hunter2");
        assert_eq!(format!("{secret:?}"), "Secret(..)");
        assert_eq!(format!("{:?}", Some(&secret)), "Some(Secret(..))");
        assert_eq!(secret.expose(), b"hunter2");
        assert_eq!(secret.len(), 7);
        assert!(!secret.is_empty());
        assert!(Secret::new().is_empty());
    }

    #[test]
    fn grows_and_shrinks_like_a_buffer() {
        let mut secret = Secret::new();
        for byte in b"pass" {
            secret.push(*byte);
        }
        secret.push_bytes(b"word!");
        assert_eq!(secret.pop(), Some(b'!'));
        assert_eq!(secret.expose(), b"password");
        let copy = secret.clone();
        assert_eq!(copy, secret);
        assert_ne!(copy, Secret::copy_from(b"other"));

        let mut line = Secret::copy_from(b"pin\r\n");
        line.trim_newline();
        assert_eq!(line.expose(), b"pin");
        line.trim_newline();
        assert_eq!(line.expose(), b"pin");
    }

    #[test]
    fn growing_moves_the_bytes_to_new_pages() {
        let page = zbm_sys::memory::page_size();
        let mut secret = Secret::new();
        assert_eq!(secret.protection(), None);
        secret.push_bytes(b"abcd");
        let first = secret.expose().as_ptr();
        assert!(secret.protection().is_some());
        secret.push_bytes(&vec![b'x'; page]);
        assert_ne!(secret.expose().as_ptr(), first);
        assert_eq!(secret.len(), page + 4);
        assert_eq!(&secret.expose()[..5], b"abcdx");
        let empty = Secret::from_vec(Vec::new());
        assert!(empty.is_empty());
        assert_eq!(empty.expose(), b"");
    }

    #[test]
    fn popped_bytes_are_wiped() {
        let mut secret = Secret::copy_from(b"ab");
        assert_eq!(secret.pop(), Some(b'b'));
        let pages = secret.pages.as_ref().unwrap();
        assert_eq!(&pages.bytes()[..2], b"a\0");
        assert_eq!(secret.pop(), Some(b'a'));
        assert_eq!(secret.pop(), None);
        assert_eq!(Secret::new().pop(), None);
    }

    /// A reader that always fails.
    struct Failing;

    impl Read for Failing {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("broken"))
        }
    }

    #[test]
    fn reads_everything_from_a_reader() {
        let long: Vec<u8> = (0..2000_u32).map(|i| (i % 251) as u8).collect();
        let secret = Secret::read_from(&mut long.as_slice()).unwrap();
        assert_eq!(secret.expose(), long.as_slice());

        assert_eq!(
            Secret::read_from(&mut Failing).unwrap_err().to_string(),
            "broken"
        );
    }
}
