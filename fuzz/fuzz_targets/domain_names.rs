//! Device paths, mapping names, pool, dataset, snapshot and filesystem
//! names, kernel versions, kernel command lines, import policies and stages
//! (inputs I1, I3, I4, I5 and I7 of docs/dev/threat-model.md), which the
//! command line (A1), a key device (A2) or a foreign pool (A3) supply.
//! Parsing must never panic, and a value that parses must print as the text
//! it was parsed from, or in a canonical form that parses back to it.

#![no_main]

use std::fmt::Display;
use std::str::FromStr;

use libfuzzer_sys::fuzz_target;
use zbm_types::{
    BootfsPreference, DatasetName, DevicePath, FilesystemName, HookStage, HostId, ImportPolicy,
    KernelArg, KernelCommandLine, KernelVersion, LogLevel, MappingName, PoolName, PoolRequirement,
    RetryDelay, SnapshotName, Stage, Timeout,
};

/// Parses `text` as a `T`, and checks that a value prints as its text.
fn round_trip<T: FromStr + Display>(text: &str) {
    if let Ok(value) = text.parse::<T>() {
        assert_eq!(value.to_string(), text);
    }
}

/// Parses `text` as a `T` that prints in one canonical form (`+3` as `3`),
/// and checks that the form it prints parses back to the same value.
fn canonical<T: FromStr + Display + PartialEq + std::fmt::Debug>(text: &str) {
    if let Ok(value) = text.parse::<T>() {
        let printed = value.to_string();
        assert_eq!(printed.parse::<T>().ok(), Some(value), "{text:?}");
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
    round_trip::<FilesystemName>(text);
    round_trip::<KernelVersion>(text);
    round_trip::<ImportPolicy>(text);
    round_trip::<Stage>(text);
    round_trip::<HookStage>(text);
    canonical::<Timeout>(text);
    canonical::<RetryDelay>(text);
    canonical::<LogLevel>(text);
    canonical::<HostId>(text);
    canonical::<KernelArg>(text);
    canonical::<KernelCommandLine>(text);
    let _ = PoolRequirement::from_conf(text);
    let _ = BootfsPreference::from_conf(text).to_string();
    let (a, b) = text.split_at_checked(text.len() / 2).unwrap_or((text, ""));
    let _ = zbm_types::version::version_cmp(a, b);
});
