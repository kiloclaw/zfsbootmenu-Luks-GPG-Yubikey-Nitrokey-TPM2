//! Complex pools on LUKS, created and used with ZFS 2.4.4.
//!
//! Disks (16 plus the payload disk):
//!
//! | Disks | Use |
//! | --- | --- |
//! | d0-d3 | RAIDZ2 data vdev of tank (LUKS, passphrase pw) |
//! | d4-d5 | special mirror of tank (LUKS) |
//! | d6-d7 | dedup mirror of tank (LUKS) |
//! | d8-d9 | log mirror of tank (LUKS) |
//! | d10 | cache of tank (LUKS) |
//! | d11 | hot spare of tank (LUKS) |
//! | d12 | LUKS half of the mirror "mixed" (LUKS, passphrase other) |
//! | d13 | plain half of the mirror "mixed" (no encryption) |
//! | d14 | disk added to raidz2-0 by RAIDZ expansion (LUKS) |
//! | d15 | partition zfsA (LUKS) and partition plainB (plain), mirror "parts" |
//!
//! `tank/ROOT/be` (the boot environment) uses ZFS native encryption on top
//! of LUKS. `tank/data` activates the newer pool features: `fast_dedup`,
//! `block_cloning` and `block_cloning_endian`, `longname`,
//! `physical_rewrite` (`zfs rewrite -P`), `dynamic_gang_header` (forced gang
//! blocks), `zstd_compress`, `blake3`, `raidz_expansion` and
//! `allocation_classes`.
//!
//! Scenarios:
//!
//! - `setup`: create everything in the recovery shell (`zbm.luks=off`);
//! - `healthy`: automatic unlocking; two passphrases, ZFS key in
//!   ZFSBootMenu and in the boot environment; every pool ONLINE;
//! - `crypttab`: the same through an embedded crypttab mixing `UUID=`,
//!   `ID=` and `PARTLABEL=` specifications;
//! - `degraded`: a RAIDZ2 member and the plain mirror half are missing;
//! - `hotplug`: `zbm.luks=` lists every volume; d0 appears while
//!   ZFSBootMenu waits for it;
//! - `hotplug-auto`: only the special, dedup, log, cache and spare disks are
//!   present; the rest appear during the import retry loop;
//! - `badkey`: wrong ZFS key: recovery shell with volumes and pools
//!   available;
//! - `skip-recover`: every LUKS prompt skipped; the volumes are unlocked
//!   with zluks in the recovery shell and the boot continues;
//! - `oldzfs`: an image with an older ZFS (`ZBMTEST_OLD_KERNEL` and
//!   `ZBMTEST_OLD_INITRD`) refuses the ZFS 2.4.4 pools; skipped when not
//!   configured.

use std::path::PathBuf;

use crate::error::Result;
use crate::images::{ImageSpec, zbm_image};
use crate::qemu::Disk;
use crate::record::Test;
use crate::settings::Settings;

mod boots;
mod healthy;
mod recover;
mod setup;
#[cfg(test)]
mod tests;

/// The LUKS volumes of the group: d0-d12, d14 and the partition zfsA of
/// d15.
const LUKS_VOLUMES: usize = 15;

/// The disk group.
const GROUP: &str = "complex";

/// The number of disks.
const N: usize = 16;

/// The options of the pools.
const OPTS: &str = "-o ashift=12 -o autotrim=on -o compatibility=openzfs-2.4-linux \
                    -O compression=lz4 -O acltype=posixacl -O xattr=sa -O relatime=on -m none";

/// The pools.
const POOLS: [&str; 3] = ["tank", "mixed", "parts"];

/// The newer features `tank` activates.
const NEW_FEATURES: [&str; 11] = [
    "raidz_expansion",
    "fast_dedup",
    "block_cloning",
    "block_cloning_endian",
    "longname",
    "physical_rewrite",
    "dynamic_gang_header",
    "zstd_compress",
    "blake3",
    "allocation_classes",
    "encryption",
];

/// The kernel command line of the boots after setup.
const BASE: &str = "console=ttyS0 zbm.timeout=3 zbm.retry_delay=4";

/// Runs a scenario of the group.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub fn run(test: &Test, scenario: &str) -> Result<()> {
    if scenario == "setup" {
        return setup::setup(test);
    }
    let settings = test.settings();
    if !crypttab(settings).exists() {
        test.check(false, "run complex-setup first");
        return Ok(());
    }
    let auto = zbm_image(settings, &ImageSpec::new("auto"))?;
    match scenario {
        "healthy" => boots::healthy(test, &auto),
        "crypttab" => boots::crypttab(test),
        "degraded" => boots::degraded(test, &auto),
        "hotplug" => boots::hotplug(test, &auto),
        "hotplug-auto" => boots::hotplug_auto(test, &auto),
        "badkey" => boots::badkey(test, &auto),
        "skip-recover" => recover::skip_recover(test, &auto),
        "oldzfs" => recover::oldzfs(test),
        _ => {
            test.check(false, &format!("unknown scenario {scenario}"));
            Ok(())
        }
    }
}

/// The crypttab that setup writes for the crypttab scenario.
fn crypttab(settings: &Settings) -> PathBuf {
    settings.disks(GROUP).join("crypttab")
}

/// The disk `d<index>` of the group, created when missing.
fn disk(settings: &Settings, index: usize) -> Result<PathBuf> {
    crate::disks::disk(settings, GROUP, index, "384M", false)
}

/// The disks of a machine, without those in `skip` (absent disks).
fn disks(settings: &Settings, skip: &[usize]) -> Result<Vec<Disk>> {
    super::disks(
        settings,
        GROUP,
        (0..N).filter(|index| !skip.contains(index)),
        false,
    )
}
