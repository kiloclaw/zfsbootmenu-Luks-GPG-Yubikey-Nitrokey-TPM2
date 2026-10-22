//! Device paths, mapping names, pool, dataset and snapshot names, kernel
//! versions, import policies and stages (inputs I1, I3, I5 and I7 of
//! docs/dev/threat-model.md), which the command line (A1), a key device (A2)
//! or a foreign pool (A3) supply. Parsing must never panic, and a value that
//! parses must print as the text it was parsed from.

#![no_main]

use std::fmt::Display;
use std::str::FromStr;

use libfuzzer_sys::fuzz_target;
use zbm_types::{
    DatasetName, DevicePath, HookStage, ImportPolicy, KernelVersion, MappingName, PoolName,
    SnapshotName, Stage,
};

/// Parses `text` as a `T`, and checks that a value prints as its text.
fn round_trip<T: FromStr + Display>(text: &str) {
    if let Ok(value) = text.parse::<T>() {
        assert_eq!(value.to_string(), text);
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    round_trip::<DevicePath>(text);
    round_trip::<MappingName>(text);
    round_trip::<PoolName>(text);
    round_trip::<DatasetName>(text);
    round_trip::<SnapshotName>(text);
    round_trip::<KernelVersion>(text);
    round_trip::<ImportPolicy>(text);
    round_trip::<Stage>(text);
    round_trip::<HookStage>(text);
    let (a, b) = text.split_at(text.len() / 2);
    let _ = zbm_types::version::filevercmp(a.as_bytes(), b.as_bytes());
});
