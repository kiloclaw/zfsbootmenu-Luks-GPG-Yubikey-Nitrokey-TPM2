//! Key input (input I10 of docs/dev/threat-model.md): the events of the
//! input devices, which a fake keyboard (A2) chooses, and the bytes of a
//! terminal, which an SSH session or a serial line sends, the replies of
//! the terminal included. Neither decoder may panic or hang.
//!
//! The first byte picks how the rest is read:
//! - even: the bytes of a terminal. Decoded in pieces, as the menu's reader
//!   decodes reads, they give the keys they give at once: a sequence cut
//!   between two reads waits for its rest, and CR LF cut between them is
//!   one Enter. Decoded with nothing more to come, every byte is used.
//! - odd: `input_event` records, fed to the display's key decoder; every
//!   eighth record also resynchronises it with the keys it holds.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zbm_display::input::parse_events;
use zbm_display::keys::{Decoder, KeyPress};
use zbm_tui::menu::term_keys::{decode, ends_with_carriage_return};

/// `EV_KEY`.
const EV_KEY: u16 = 1;

/// The keys of `bytes`, read in pieces of `size` bytes, as the menu's
/// reader (`TerminalKeys`) decodes them: what one read does not decode
/// waits for the next, and a line feed after the Enter of a carriage
/// return that ended the read before is dropped, as CR LF is one Enter.
fn in_pieces(bytes: &[u8], size: usize) -> Vec<KeyPress> {
    let mut keys = Vec::new();
    let mut held: Vec<u8> = Vec::new();
    let mut after_cr = false;
    let mut take = |held: &mut Vec<u8>, last: bool, keys: &mut Vec<KeyPress>| {
        if after_cr && !held.is_empty() {
            after_cr = false;
            if held.first() == Some(&b'\n') {
                held.remove(0);
            }
        }
        let decoded = keys.len();
        let used = decode(held, last, keys);
        assert!(used <= held.len());
        if used > 0 {
            after_cr = ends_with_carriage_return(&held[..used], &keys[decoded..]);
        }
        held.drain(..used);
    };
    for piece in bytes.chunks(size.max(1)) {
        held.extend_from_slice(piece);
        take(&mut held, false, &mut keys);
    }
    take(&mut held, true, &mut keys);
    assert!(held.is_empty(), "{held:?} left");
    keys
}

fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else {
        return;
    };
    if first % 2 == 0 {
        // Below the longest sequence waited for, so that a cut never meets
        // the limit at another place than the whole would.
        let rest = rest.get(..3000).unwrap_or(rest);
        let mut whole = Vec::new();
        assert_eq!(decode(rest, true, &mut whole), rest.len());
        let pieces = in_pieces(rest, usize::from(first / 2));
        assert_eq!(pieces, whole);
    } else {
        let mut decoder = Decoder::new();
        let mut down: Vec<u16> = Vec::new();
        for (index, event) in parse_events(rest).iter().enumerate() {
            if event.kind == EV_KEY {
                let _ = decoder.feed(event.code, event.value);
                if event.value == 0 {
                    down.retain(|&code| code != event.code);
                } else if !down.contains(&event.code) {
                    down.push(event.code);
                }
            }
            if index % 8 == 7 {
                decoder.resync(&down);
            }
        }
        // A keymap and compose rules from the same bytes: the image's
        // layouts come from its build host (A1), and must not crash the
        // console when broken.
        let text = String::from_utf8_lossy(rest);
        let _ = zbm_display::layout::Layout::from_keymap("fuzz", text.as_bytes());
        let _ = zbm_display::layout::compile::dead_keys(rest);
        let _ = zbm_display::keys::compose_key(&text);
    }
});
