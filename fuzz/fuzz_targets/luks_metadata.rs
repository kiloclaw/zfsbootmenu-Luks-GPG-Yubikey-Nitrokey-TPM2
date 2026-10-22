//! LUKS headers as `cryptsetup luksDump` prints them, and token JSON (input
//! I8 of docs/dev/threat-model.md): a tampered disk (A1) or a hostile USB
//! device (A2) chooses them. The first line is a key to look up; the rest is
//! the metadata. Parsing must never panic or hang.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zbm_luks::tokens::{json_arr, json_bool, json_str, parse_dump_text, parse_json_metadata};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = parse_json_metadata(&text);
    let _ = parse_dump_text(&text);
    let (key, json) = text.split_once('\n').unwrap_or(("type", &text));
    let _ = json_str(json, key);
    let _ = json_arr(json, key);
    let _ = json_bool(json, key);
});
