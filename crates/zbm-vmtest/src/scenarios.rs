//! The test scenarios, by group: the name of a test is its group, a dash
//! and its scenario (`layouts-main`, `complex-hotplug`), except
//! `procedures`.
//!
//! A scenario returns an error when it cannot go on; the test then records
//! a failed check with the reason, and still writes its result.

use crate::error::{Error, Result};
use crate::record::{Outcome, Test};
use crate::settings::Settings;

pub mod complex;
pub mod docs;
pub mod firmware;
pub mod keys;
pub mod layouts;
pub mod procedures;
pub mod runtime;
pub mod secrets;

/// Every test, in the order `zbm-vmtest run` runs them: each setup before
/// the scenarios that need it.
pub const ALL: &[&str] = &[
    "layouts-main",
    "layouts-parity",
    "layouts-parity-oldzfs",
    "complex-setup",
    "complex-healthy",
    "complex-crypttab",
    "complex-degraded",
    "complex-hotplug",
    "complex-hotplug-auto",
    "complex-badkey",
    "complex-skip-recover",
    "complex-oldzfs",
    "keys-setup",
    "keys-plugin",
    "keys-native",
    "keys-lockout",
    "keys-notpm",
    "firmware-setup",
    "firmware-boot",
    "firmware-usbkey",
    "firmware-usblate",
    "firmware-nostick",
    "procedures",
    "runtime-setup",
    "runtime-legacy",
    "runtime-rust",
    "runtime-legacy-hostid",
    "runtime-rust-hostid",
    "runtime-legacy-strict",
    "runtime-rust-strict",
    "runtime-legacy-force",
    "runtime-rust-force",
    "runtime-legacy-prefer",
    "runtime-rust-prefer",
    "runtime-legacy-late",
    "runtime-rust-late",
    "runtime-legacy-waitfor",
    "runtime-rust-waitfor",
    "runtime-legacy-hooks",
    "runtime-rust-hooks",
    "runtime-legacy-signals",
    "runtime-rust-signals",
    "runtime-legacy-keysource",
    "runtime-rust-pilot",
    "runtime-rust-crash",
    "runtime-rust-autosize",
    "runtime-rust-colours",
    "runtime-rust-drm",
    "runtime-rust-console",
    "runtime-rust-saved",
    "secrets-setup",
    "secrets-passphrase",
    "secrets-keyfile",
];

/// Tests that are not in [`ALL`]: tools rather than tests.
pub const TOOLS: &[&str] = &["runtime-record-zfs", "runtime-record-fixtures"];

/// Runs the scenario of `test`, named as in [`ALL`].
fn dispatch(test: &Test, name: &str) -> Result<()> {
    let (group, scenario) = name.split_once('-').unwrap_or((name, ""));
    match group {
        "layouts" => layouts::run(test, scenario),
        "complex" => complex::run(test, scenario),
        "keys" => keys::run(test, scenario),
        "firmware" => firmware::run(test, scenario),
        "procedures" => procedures::run(test),
        "runtime" => runtime::run(test, scenario),
        "secrets" => secrets::run(test, scenario),
        _ => Err(Error::Setup(format!("unknown test {name}"))),
    }
}

/// Runs the test `name` and writes its result.
///
/// # Errors
///
/// [`Error::Io`] when the test's directory cannot be made.
pub fn run(settings: &Settings, name: &str) -> Result<Outcome> {
    let test = Test::new(settings, name)?;
    if let Err(error) = dispatch(&test, name) {
        test.check(false, &format!("the test ran to its end: {error}"));
    }
    Ok(test.finish())
}

/// Whether `name` names a test or a tool.
pub fn exists(name: &str) -> bool {
    ALL.contains(&name) || TOOLS.contains(&name)
}

/// The machines' disks `d<index>` of a group, as Python's `disk()` names
/// them: `(path, serial)`.
pub fn disks(
    settings: &Settings,
    group: &str,
    indexes: impl IntoIterator<Item = usize>,
    fresh: bool,
) -> Result<Vec<crate::qemu::Disk>> {
    indexes
        .into_iter()
        .map(|index| {
            let path = crate::disks::disk(settings, group, index, "384M", fresh)?;
            Ok(crate::qemu::Disk::new(path, format!("d{index}")))
        })
        .collect()
}

/// The payload disk, as a machine's disk.
///
/// # Errors
///
/// When the boot environment or the disk cannot be built.
pub fn payload(settings: &Settings) -> Result<crate::qemu::Disk> {
    let path = crate::disks::payload_disk(settings)?;
    Ok(crate::qemu::Disk::new(path, crate::qemu::PAYLOAD))
}
