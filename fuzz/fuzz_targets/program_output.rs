//! The output of `zpool` and `zfs`, of a failed import, and the state file of
//! LUKS mappings (inputs I9 and I11 of docs/dev/threat-model.md): pools and
//! devices that someone else controls (A2, A3) shape what the programs print,
//! and hooks can write the state file. Parsing must never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zbm_boot::import::scrape_hostid;
use zbm_luks::mappings::parse_mappings;
use zbm_zfs::parse_table;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = parse_table(&text);
    let _ = scrape_hostid(&text);
    let _ = parse_mappings(&text);
});
