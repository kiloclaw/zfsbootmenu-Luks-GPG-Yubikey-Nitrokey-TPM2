//! Secrets: passphrases, PINs and keys.
//!
//! A [`Secret`] holds bytes that must not outlive their use or leak into a log:
//! they are wiped when the secret is dropped, they grow without leaving copies
//! behind in freed memory, and formatting a secret prints `Secret(..)`. See
//! `crates/DESIGN.md` for the rules secrets follow on their way to programs.

use std::fmt;
use std::io::{self, Read};

/// How many bytes a secret reserves at first when it is read; most secrets are
/// passphrases and keys of a few hundred bytes at most.
const INITIAL_CAPACITY: usize = 256;

/// Bytes that are wiped from memory when they are no longer needed.
#[derive(Default)]
pub struct Secret {
    bytes: Vec<u8>,
}

impl Secret {
    /// An empty secret.
    pub fn new() -> Secret {
        Secret::default()
    }

    /// Takes ownership of `bytes`. Only the final allocation is wiped later, so
    /// the bytes must not have been copied around (for example by growing the
    /// vector) before.
    pub fn from_vec(bytes: Vec<u8>) -> Secret {
        Secret { bytes }
    }

    /// A secret with a copy of `bytes`; for tests and for values that are not
    /// secret on their own, such as a constant.
    pub fn copy_from(bytes: &[u8]) -> Secret {
        let mut secret = Secret::with_capacity(bytes.len());
        secret.bytes.extend_from_slice(bytes);
        secret
    }

    fn with_capacity(capacity: usize) -> Secret {
        Secret {
            bytes: Vec::with_capacity(capacity),
        }
    }

    /// The secret bytes.
    pub fn expose(&self) -> &[u8] {
        &self.bytes
    }

    /// The number of bytes.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the secret is empty.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Makes room for at least `additional` more bytes. The bytes move to a new
    /// allocation by hand, so the old one can be wiped first; `Vec` would free
    /// it with the secret still in it.
    fn reserve(&mut self, additional: usize) {
        let needed = self.bytes.len().saturating_add(additional);
        if needed <= self.bytes.capacity() {
            return;
        }
        let capacity = needed.max(self.bytes.capacity().saturating_mul(2));
        let mut bytes = Vec::with_capacity(capacity);
        bytes.extend_from_slice(&self.bytes);
        zbm_sys::wipe_vec(&mut self.bytes);
        self.bytes = bytes;
    }

    /// Appends `bytes`.
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        self.reserve(bytes.len());
        self.bytes.extend_from_slice(bytes);
    }

    /// Appends one byte.
    pub fn push(&mut self, byte: u8) {
        self.reserve(1);
        self.bytes.push(byte);
    }

    /// Removes the last byte, as a backspace at a passphrase prompt does.
    pub fn pop(&mut self) -> Option<u8> {
        let byte = self.bytes.pop();
        if let Some(spare) = self.bytes.spare_capacity_mut().first_mut() {
            spare.write(0);
        }
        byte
    }

    /// Removes a final newline (or carriage return and newline), as the shell's
    /// `read` and `$( ... )` do with the lines they read.
    pub fn trim_newline(&mut self) {
        for end in *b"\n\r" {
            if self.bytes.last() == Some(&end) {
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
        let mut secret = Secret::with_capacity(INITIAL_CAPACITY);
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

impl Clone for Secret {
    fn clone(&self) -> Secret {
        Secret::copy_from(&self.bytes)
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        zbm_sys::wipe_vec(&mut self.bytes);
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Secret) -> bool {
        self.bytes == other.bytes
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
    fn growing_wipes_the_old_allocation() {
        let mut secret = Secret::with_capacity(4);
        secret.push_bytes(b"abcd");
        let old = secret.bytes.as_ptr();
        secret.push_bytes(b"efgh");
        assert_ne!(secret.bytes.as_ptr(), old);
        assert_eq!(secret.expose(), b"abcdefgh");
        assert!(secret.bytes.capacity() >= 8);
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
