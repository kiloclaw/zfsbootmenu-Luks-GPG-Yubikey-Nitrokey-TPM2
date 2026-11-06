//! A device's kernel messages ([`Action::LoadDeviceLog`]): the kernel log
//! read as `dmesg` reads it, then the records about the device and those it
//! stands on ([`Devices::stack`]), which name one of them as a word or
//! which the kernel tagged with one of their numbers.
//!
//! The kernel's text and the names of devices come from the devices, which
//! whoever made them named: they stay [`Untrusted`].
//!
//! [`Action::LoadDeviceLog`]: crate::model::Action::LoadDeviceLog

use zbm_platform::dmesg;
use zbm_platform::{Files, Runner};
use zbm_tools::devices::Devices;
use zbm_types::Untrusted;

use crate::model::DeviceLog;

/// The most lines a device's messages keep: the newest.
pub(crate) const LINES: usize = 500;

/// The kernel's messages about `device`, as a pool names it, read through
/// `files`, or by `dmesg`, which `runner` runs, when `/dev/kmsg` cannot be
/// read.
pub(crate) fn device_log(
    files: &dyn Files,
    runner: &dyn Runner,
    device: &Untrusted<String>,
) -> DeviceLog {
    let stack = Devices::new(runner, files).stack(device.get());
    let names: Vec<&str> = stack.iter().map(|found| found.name.as_str()).collect();
    let numbers: Vec<&str> = stack
        .iter()
        .filter_map(|found| found.numbers.as_deref())
        .collect();
    let mut lines = if stack.is_empty() {
        Vec::new()
    } else {
        dmesg::device_lines(files, runner, &names, &numbers)
    };
    lines.drain(..lines.len().saturating_sub(LINES));
    DeviceLog {
        device: device.clone(),
        names: names
            .into_iter()
            .map(|name| Untrusted::new(name.to_owned()))
            .collect(),
        lines: lines.into_iter().map(Untrusted::new).collect(),
    }
}

#[cfg(test)]
mod tests;
