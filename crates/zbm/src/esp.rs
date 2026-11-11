//! `mount_esp [DEVICE]`: mounts DEVICE, or without one the EFI system
//! partition, read/write at `/mnt/NAME` for the recovery shell, and leaves
//! it mounted ([`zbm_app::esp`], ADR 0018, point 10). It prints the mount
//! point, or where the device is mounted already, on standard output and
//! exits 0. When it cannot, it says why on standard error and in the kernel
//! log, where the legacy script logged it, and exits 1. Two or more
//! arguments print the usage and exit 2.

use std::path::Path;

use zbm_app::esp::{self, EspError};
use zbm_config::conf::{self, Conf};
use zbm_platform::{NoConsole, RealSystem, System};
use zbm_startup::program::Startup;
use zbm_types::{DevicePath, Printable, code};

use crate::table::MOUNT_ESP;
use crate::{Io, USAGE};

/// `mount_esp [DEVICE]` on this machine.
pub(crate) fn run(args: &[String], io: &mut Io) -> i32 {
    let real = RealSystem::new();
    run_in(&real.with_console(&NoConsole), args, io)
}

/// `mount_esp` with `args` on `system`. Its messages go to the kernel log
/// until it returns, as the menu's do.
fn run_in(system: &System<'_>, args: &[String], io: &mut Io) -> i32 {
    let name = match args {
        [] => "",
        [name] => name.as_str(),
        _ => {
            let _ = io.stderr.write_all(MOUNT_ESP.usage.as_bytes());
            return USAGE;
        }
    };
    let conf = system
        .files
        .read_to_string(Path::new(conf::PATH))
        .map(|text| Conf::parse(&text))
        .unwrap_or_default();
    // Start-up's kernel log, which lives as long as `_startup`.
    let _startup = Startup::new(*system, &conf);
    match device(name).and_then(|device| esp::mount_esp(system, device.as_ref())) {
        Ok(point) => {
            let _ = writeln!(io.stdout, "{}", Printable(&point.to_string_lossy()));
            0
        }
        Err(error) => {
            // The refusal and each of its causes, such as why the mount
            // point could not be made.
            let said = Printable(&code::chain(&error)).to_string();
            zbm_trace::error!("mount_esp: {said}"; %said);
            let _ = writeln!(io.stderr, "mount_esp: {said}");
            1
        }
    }
}

/// The device `name` names: none for an empty name, which the legacy
/// script took as no argument.
///
/// # Errors
///
/// A name that is no device's path, which cannot be a block device.
fn device(name: &str) -> Result<Option<DevicePath>, EspError> {
    if name.is_empty() {
        return Ok(None);
    }
    name.parse()
        .map(Some)
        .map_err(|_| EspError::NotBlockDevice(name.to_owned()))
}

#[cfg(test)]
mod tests;
