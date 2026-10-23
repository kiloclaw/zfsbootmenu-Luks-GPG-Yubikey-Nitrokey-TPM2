//! The procedures of `docs/luks/vdev-configurations.rst` and the pool
//! sections of `docs/luks/administration.rst`, run as written with ZFS
//! 2.4.4.
//!
//! The commands come from the documentation itself (see [`super::docs`]);
//! the test only supplies devices for the placeholder names (symbolic links
//! in `/dev/disk/by-id`), answers `cryptsetup`'s passphrase prompts, and
//! fills in values the documentation cannot know, such as the UUIDs it
//! abbreviates as `luks-5d0c3a1e-...`. The LUKS volumes use `cryptsetup`'s
//! default PBKDF, as in the documentation.
//!
//! Disks: `ata-DISK1..7` partitioned (esp, zfs), `ata-DISK1..7` whole (for
//! dRAID), `nvme-FAST1..3` with partitions 2-5 (special, log, cache,
//! dedup), `ata-NEWDISK` and `nvme-SSD1..2` partitioned.
//!
//! The test runs the procedures, then reboots and checks the documented
//! verification step.

use crate::error::Result;
use crate::images::{ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::vm::Vm;

use super::payload;

mod admin;
mod devices;
mod layouts;
mod session;
mod upgrade;
mod verify;

use session::Run;

/// Runs the procedures, then boots the complete layout of Example 11.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let disks = devices::create(settings)?;
    let image = zbm_image(settings, &ImageSpec::new("auto"))?;
    let mut machine_disks = disks.clone();
    machine_disks.push(payload(settings)?);
    let machine = Machine::boot(&image, "console=ttyS0 zbm.luks=off zbm.retry_delay=3")
        .with_disks(machine_disks);
    let mut vm = Vm::start(test, "procedures", &machine)?;
    vm.recovery_shell()?;
    vm.send_functions()?;
    vm.must(&devices::links(&devices::table()).join("; "))?;
    let mut run = Run::new(vm, &settings.repo.join("docs/luks"));
    layouts::preparation(&mut run)?;
    layouts::data_layouts(&mut run)?;
    admin::auxiliary(&mut run)?;
    admin::administration(&mut run)?;
    admin::expansion(&mut run)?;
    admin::single_to_mirror(&mut run)?;
    layouts::nvme_mirror(&mut run)?;
    upgrade::upgrade_pool(&mut run)?;
    verify::bootable(run)?;
    verify::verify(test, &image, disks)
}

#[cfg(test)]
mod tests;
