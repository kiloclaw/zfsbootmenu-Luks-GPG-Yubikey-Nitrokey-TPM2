//! The output of `zpool` and `zfs` (JSON and text), of a failed import, of
//! the status of pools and the listing of those not imported, of a pool's
//! configuration and uberblock that `zdb` prints, of the snapshots, holds
//! and destroys a review reads, and the state file of LUKS mappings
//! (inputs I9 and I11 of docs/dev/threat-model.md): pools and devices that
//! someone else controls (A2, A3) shape what the programs print, vdev
//! names, host names, clone names and hold tags included, and hooks can
//! write the state file. Parsing must never panic.

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
    let _ = json::status(&text);
    let _ = text::pools(&text);
    let _ = text::datasets(&text, &["mountpoint", "canmount"]);
    let _ = text::snapshots(&text);
    let _ = text::own_snapshots(&text);
    let _ = text::properties(&text);
    let _ = text::version(&text);
    let _ = text::status(&text);
    let _ = text::importable(&text);
    let _ = text::pool_config(&text);
    let _ = text::checkpoint(&text);
    let _ = text::snapshot_details(&text);
    let _ = text::holds(&text);
    let _ = text::reclaim(&text);
    let _ = scrape_hostid(&text);
    let _ = parse_mappings(&text);
});
