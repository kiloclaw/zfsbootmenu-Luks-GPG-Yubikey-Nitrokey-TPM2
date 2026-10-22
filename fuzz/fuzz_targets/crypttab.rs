//! The value of `zbm.luks` and a crypttab (inputs I1 and I7 of
//! docs/dev/threat-model.md): the command line (A1) and a key device (A2)
//! supply them. The first line is the value of `zbm.luks`, unless it is `-`;
//! the rest is the crypttab. Loading must never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zbm_luks::config::load_config;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
    let zbm_luks = (first != "-").then_some(first);
    let _ = load_config(zbm_luks, Some(rest), "/etc/crypttab");
    let _ = load_config(None, Some(&text), "/etc/crypttab");
});
