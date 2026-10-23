//! The checks of a boot of the complex pools, and the boot environment's
//! command line.

use crate::boot::{Boot, check_be, check_teardown};
use crate::disks::luks_uuid;
use crate::error::Result;
use crate::expect::regex;
use crate::record::Test;
use crate::settings::Settings;

use super::{NEW_FEATURES, POOLS, disk};

/// What a boot is expected to show: by default, every pool ONLINE, in
/// ZFSBootMenu and in the boot environment, and nothing left imported.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Expect<'a> {
    /// Pools that are not ONLINE in ZFSBootMenu, and their states.
    pub states: &'a [(&'a str, &'a str)],
    /// The same in the boot environment; `None`: as in ZFSBootMenu.
    pub be_states: Option<&'a [(&'a str, &'a str)]>,
    /// Pools ZFSBootMenu leaves imported, read-only, at kexec.
    pub readonly: &'a [&'a str],
}

/// The state `states` gives `pool`, ONLINE by default.
fn state_of<'a>(states: &[(&str, &'a str)], pool: &str) -> &'a str {
    states
        .iter()
        .find(|(name, _)| *name == pool)
        .map_or("ONLINE", |(_, state)| *state)
}

/// Checks the pools, their vdevs, the teardown and the boot environment.
///
/// # Errors
///
/// [`crate::Error::Pattern`] when a pattern is invalid.
pub(super) fn expect_healthy(test: &Test, boot: &Boot, expect: Expect<'_>) -> Result<()> {
    let pools = boot.pools();
    for pool in POOLS {
        let want = state_of(expect.states, pool);
        test.check(
            pools.get(pool) == Some(want),
            &format!(
                "ZFSBootMenu imported {pool}: {} (expected {want})",
                pools.show(pool)
            ),
        );
    }
    let status = &boot.zbm_status;
    for section in ["special", "dedup", "logs", "cache", "spares"] {
        let pattern = regex(&format!(r"(?m)^\s*{section}\s*$"))?;
        test.check(
            pattern.is_match(status),
            &format!("tank shows its {section} section"),
        );
    }
    if !expect.states.iter().any(|(pool, _)| *pool == "tank") {
        test.check(
            regex(r"cache\s*\n\s*/dev/mapper/\S+\s+ONLINE")?.is_match(status),
            "cache vdev ONLINE",
        );
        test.check(
            regex(r"spares\s*\n\s*/dev/mapper/\S+\s+AVAIL")?.is_match(status),
            "hot spare AVAIL",
        );
        let raidz = regex(r"raidz2-0.*\n(?:\s+/dev/mapper/\S+\s+ONLINE.*\n){5}")?;
        test.check(
            raidz.find_iter(status).count() == 1,
            "raidz2-0 has five members after RAIDZ expansion",
        );
    }
    check_teardown(test, boot, expect.readonly);
    test.check(
        boot.saw("be-zfskey"),
        "boot environment asked for the ZFS key of its encrypted root",
    );
    let be_states = expect.be_states.unwrap_or(expect.states);
    check_be(test, boot, "tank", &POOLS, be_states);
    let features = boot.be_features();
    for feature in NEW_FEATURES {
        let state = features.get(&("tank".to_owned(), feature.to_owned()));
        test.check(
            state.is_some_and(|state| state == "active"),
            &format!("boot environment sees tank feature {feature} active"),
        );
    }
    Ok(())
}

/// The boot environment's command line as setup sets it (`becmdline`),
/// without the volumes of the disks in `skip`: dracut waits for every
/// `rd.luks.uuid=` volume and then drops to its emergency shell.
///
/// # Errors
///
/// When a disk has no LUKS header.
pub(super) fn be_cmdline(settings: &Settings, skip: &[usize]) -> Result<String> {
    let mut volumes = Vec::new();
    for index in (0..12).chain([14]) {
        if !skip.contains(&index) {
            volumes.push(luks_uuid(&disk(settings, index)?, None)?);
        }
    }
    let other = if skip.contains(&12) {
        None
    } else {
        Some(luks_uuid(&disk(settings, 12)?, None)?)
    };
    let part = if skip.contains(&15) {
        None
    } else {
        Some(luks_uuid(&disk(settings, 15)?, Some(1))?)
    };
    Ok(cmdline(&volumes, other.as_deref(), part.as_deref()))
}

/// The command line with the volumes `volumes` (d0-d11 and d14), the
/// volume `other` with its own key (d12) and the partition `part` (zfsA).
pub(super) fn cmdline(volumes: &[String], other: Option<&str>, part: Option<&str>) -> String {
    let mut words = vec![
        "console=ttyS0".to_owned(),
        "loglevel=4".to_owned(),
        "rd.luks.key=/etc/zbmtest/be.key".to_owned(),
    ];
    words.extend(volumes.iter().map(|uuid| format!("rd.luks.uuid={uuid}")));
    if let Some(uuid) = other {
        words.push(format!("rd.luks.key={uuid}=/etc/zbmtest/be-other.key"));
        words.push(format!("rd.luks.uuid={uuid}"));
    }
    if let Some(uuid) = part {
        words.push(format!("rd.luks.uuid={uuid}"));
    }
    words.join(" ")
}
