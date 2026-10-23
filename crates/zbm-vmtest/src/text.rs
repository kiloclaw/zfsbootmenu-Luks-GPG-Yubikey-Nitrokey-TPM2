//! Console bytes as text: UTF-8 decoded across reads, with the terminal's
//! escape sequences removed.
//!
//! Patterns match this text, so that colours and cursor movements between
//! the words of a prompt do not hide it. What the screen shows is the
//! terminal emulator's business ([`crate::console::Console::screen`]).

/// Where the decoder is in an escape sequence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Escape {
    /// Not in one.
    #[default]
    None,
    /// After ESC.
    Start,
    /// In a control sequence (`ESC [`), until its final byte.
    Csi,
    /// In an operating system command (`ESC ]`), until BEL or `ESC \`.
    Osc,
    /// After ESC in an operating system command.
    OscEnd,
    /// After `ESC (` or `ESC )` and the like, before the character set.
    Charset,
}

/// Decodes console bytes as they arrive.
#[derive(Debug, Default)]
pub struct Decoder {
    /// The start of a UTF-8 sequence that the next read completes.
    pending: Vec<u8>,
    escape: Escape,
}

impl Decoder {
    /// Appends the text of `bytes` to `out`: invalid UTF-8 becomes U+FFFD,
    /// and escape sequences are dropped.
    pub fn feed(&mut self, bytes: &[u8], out: &mut String) {
        self.pending.extend_from_slice(bytes);
        let mut rest = std::mem::take(&mut self.pending);
        let mut start = 0;
        loop {
            match std::str::from_utf8(&rest[start..]) {
                Ok(text) => {
                    self.push_str(text, out);
                    break;
                }
                Err(error) => {
                    let valid = start + error.valid_up_to();
                    let text = std::str::from_utf8(&rest[start..valid]).unwrap_or_default();
                    self.push_str(text, out);
                    if let Some(length) = error.error_len() {
                        self.push(char::REPLACEMENT_CHARACTER, out);
                        start = valid + length;
                    } else {
                        rest.drain(..valid);
                        self.pending = rest;
                        break;
                    }
                }
            }
        }
    }

    fn push_str(&mut self, text: &str, out: &mut String) {
        for c in text.chars() {
            self.push(c, out);
        }
    }

    fn push(&mut self, c: char, out: &mut String) {
        self.escape = match (self.escape, c) {
            (Escape::Osc, '\u{1b}') => Escape::OscEnd,
            (Escape::Osc | Escape::OscEnd, '\u{7}') | (Escape::OscEnd, '\\') => Escape::None,
            (Escape::Osc | Escape::OscEnd, _) => Escape::Osc,
            (_, '\u{1b}') => Escape::Start,
            (Escape::None, c) => {
                out.push(c);
                Escape::None
            }
            (Escape::Start, '[') => Escape::Csi,
            (Escape::Start, ']') => Escape::Osc,
            (Escape::Start, '(' | ')' | '*' | '+' | '#' | '%') => Escape::Charset,
            (Escape::Csi, '\u{40}'..='\u{7e}') | (Escape::Start | Escape::Charset, _) => {
                Escape::None
            }
            (Escape::Csi, _) => Escape::Csi,
        };
    }
}

/// `text` without escape sequences and carriage returns.
pub fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    Decoder::default().feed(text.as_bytes(), &mut out);
    out.retain(|c| c != '\r');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn decode(chunks: &[&[u8]]) -> String {
        let mut decoder = Decoder::default();
        let mut out = String::new();
        for chunk in chunks {
            decoder.feed(chunk, &mut out);
        }
        out
    }

    #[test]
    fn escape_sequences_are_dropped() {
        let text = "\x1b[1;32mBooting\x1b[0m \x1b]0;title\x07tank\x1b(B/ROOT\x1b[?25l\x1b7 in";

        assert_eq!(clean(text), "Booting tank/ROOT in");
    }

    #[test]
    fn an_osc_ends_with_bel_or_st() {
        assert_eq!(clean("a\x1b]2;x\x1b\\b\x1b]2;y\x07c"), "abc");
    }

    #[test]
    fn sequences_and_characters_split_across_reads_are_kept_together() {
        let text = decode(&[b"pool \x1b[3", b"1mta", b"nk \xc3", b"\xa9t\xc3\xa9"]);

        assert_eq!(text, "pool tank été");
    }

    #[test]
    fn invalid_utf8_becomes_a_replacement_character() {
        assert_eq!(decode(&[b"a\xffb"]), "a\u{fffd}b");
    }

    #[test]
    fn clean_drops_carriage_returns() {
        assert_eq!(clean("RC=0\r\r\nzbm# "), "RC=0\nzbm# ");
    }
}
