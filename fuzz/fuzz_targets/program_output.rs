//! The output of `zpool` and `zfs` (JSON and text), of a failed import, and
//! the state file of LUKS mappings (inputs I9 and I11 of
//! docs/dev/threat-model.md): pools and devices that someone else controls
//! (A2, A3) shape what the programs print, and hooks can write the state
//! file. Parsing must never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zbm_luks::mappings::parse_mappings;
use zbm_pool::hostid::scrape_hostid;
use zbm_zfs::{json, parse_table, text};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = parse_table(&text);
    let _ = json::pools(&text);
    let _ = json::datasets(&text);
    let _ = json::snapshots(&text);
    let _ = json::properties_of(&text);
    let _ = json::version(&text);
    let _ = text::pools(&text);
    let _ = text::datasets(&text, &["mountpoint", "canmount"]);
    let _ = text::snapshots(&text);
    let _ = text::properties(&text);
    let _ = text::version(&text);
    let _ = scrape_hostid(&text);
    let _ = parse_mappings(&text);
});
