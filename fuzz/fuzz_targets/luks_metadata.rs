//! LUKS headers as `cryptsetup luksDump` prints them, and token JSON (input
//! I8 of docs/dev/threat-model.md): a tampered disk (A1) or a hostile USB
//! device (A2) chooses them. The first line is a key to look up; the rest is
//! the metadata. Parsing must never panic or hang.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zbm_crypt::token::{Token, from_dump_text, from_metadata};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = from_metadata(&text);
    let _ = from_dump_text(&text);
    let (key, json) = text.split_once('\n').unwrap_or(("type", &text));
    if let Ok(token) = Token::from_export("0", json) {
        let _ = token.text(key);
        let _ = token.list(key);
        let _ = token.flag(key);
        let _ = token.to_json();
    }
});
