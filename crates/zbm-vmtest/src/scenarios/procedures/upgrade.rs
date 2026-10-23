//! Upgrading a pool from the OpenZFS 2.3 compatibility set
//! (`docs/luks/zfs-versions.rst`).
//!
//! Two pools are created with `compatibility=openzfs-2.3-linux` on LUKS
//! volumes: `zroot` is upgraded with the commands of "Upgrading a Pool",
//! `zoff` with the compatibility property turned off. `zpool upgrade`
//! never enables `dynamic_gang_header` (`ZFEATURE_FLAG_NO_UPGRADE`); the
//! documentation enables it with `zpool set`.

use std::collections::HashMap;

use crate::error::Result;
use crate::pyfmt;
use crate::vm::{Sh, Vm};

use super::session::{Run, ZV};

/// The features OpenZFS 2.4 adds.
pub(super) const NEW_IN_24: [&str; 3] = [
    "block_cloning_endian",
    "physical_rewrite",
    "dynamic_gang_header",
];

/// The command of "Upgrading a Pool" that enables `dynamic_gang_header`.
const DGH_SET: &str = "feature@dynamic_gang_header=enabled";

/// Feature states of a pool, by feature.
pub(super) type Features = HashMap<String, String>;

/// The feature states in the output of `zpool get -H -o property,value`.
pub(super) fn parse_features(out: &str) -> Features {
    out.lines()
        .filter(|line| line.starts_with("feature@"))
        .filter_map(|line| {
            let line = line.replace("feature@", "");
            let mut fields = line.split_whitespace();
            Some((fields.next()?.to_owned(), fields.next()?.to_owned()))
        })
        .collect()
}

/// The feature states of `pool`.
///
/// # Errors
///
/// When the command does not end in time.
fn feature_states(vm: &mut Vm<'_>, pool: &str) -> Result<Features> {
    let (_, out) = vm.sh_with(
        &format!("zpool get -H -o property,value all {pool} | grep feature@"),
        Sh::default().quiet(),
    )?;
    Ok(parse_features(&out))
}

/// The state of `feature`, if the pool has it.
pub(super) fn state<'f>(features: &'f Features, feature: &str) -> Option<&'f str> {
    features.get(feature).map(String::as_str)
}

/// The states of the features of [`NEW_IN_24`], as Python writes a list.
pub(super) fn new_states(features: &Features) -> String {
    pyfmt::option_list(&NEW_IN_24.map(|feature| state(features, feature)))
}

/// Whether every feature of [`NEW_IN_24`] is in `wanted` state.
pub(super) fn has_all_new(features: &Features, wanted: &str) -> bool {
    NEW_IN_24
        .iter()
        .all(|feature| state(features, feature) == Some(wanted))
}

/// Whether `zpool upgrade` enabled the features it can enable.
fn is_upgraded(features: &Features) -> bool {
    state(features, "block_cloning_endian") == Some("enabled")
        && state(features, "physical_rewrite") == Some("enabled")
}

/// Upgrades `zroot` as documented, and `zoff` with the compatibility
/// property turned off.
///
/// # Errors
///
/// When a step cannot be done.
pub(super) fn upgrade_pool(run: &mut Run<'_>) -> Result<()> {
    create_pools(run)?;
    upgrade_zroot(run)?;
    upgrade_zoff(run)?;
    run.vm.sh("zpool destroy zoff")?;
    run.reset(Some("zroot"))
}

/// Creates `zroot` and `zoff` with the OpenZFS 2.3 compatibility set.
fn create_pools(run: &mut Run<'_>) -> Result<()> {
    let test = run.test;
    run.vm.must(
        "V=(); U=(); mkluks pw \"--pbkdf argon2id --pbkdf-memory 65536 --iter-time 500\" \
         /dev/disk/by-id/ata-DISK1-part2 /dev/disk/by-id/ata-DISK2-part2",
    )?;
    for (pool, index) in [("zroot", 0), ("zoff", 1)] {
        run.vm.must(&format!(
            "zpool create -f -o ashift=12 -o compatibility=openzfs-2.3-linux -m none {pool} \
             \"${{V[{index}]}}\""
        ))?;
        let features = feature_states(&mut run.vm, pool)?;
        test.check(
            has_all_new(&features, "disabled"),
            &format!(
                "{pool}: openzfs-2.3-linux leaves the 2.4 features disabled ({})",
                new_states(&features)
            ),
        );
        test.check(
            state(&features, "raidz_expansion") == Some("enabled")
                && state(&features, "fast_dedup") == Some("enabled"),
            &format!("{pool}: openzfs-2.3-linux enables the 2.3 features"),
        );
    }
    Ok(())
}

/// Upgrades `zroot` with the documented commands, first without their
/// `zpool set` step.
fn upgrade_zroot(run: &mut Run<'_>) -> Result<()> {
    let test = run.test;
    run.vm.sh_show("zpool upgrade zroot")?;
    let features = feature_states(&mut run.vm, "zroot")?;
    test.check(
        state(&features, "block_cloning_endian") == Some("disabled"),
        "zpool upgrade keeps to the compatibility set",
    );
    let dgh_set: Vec<String> = run
        .commands(ZV, "Upgrading a Pool")?
        .into_iter()
        .filter(|command| command.contains(DGH_SET))
        .collect();
    test.check(
        dgh_set.len() == 1,
        "Upgrading a Pool enables dynamic_gang_header with zpool set",
    );
    run.doc_with(ZV, "Upgrading a Pool", &[DGH_SET], &[])?;
    let features = feature_states(&mut run.vm, "zroot")?;
    test.check(
        is_upgraded(&features),
        "openzfs-2.4-linux and zpool upgrade enable block_cloning_endian and physical_rewrite",
    );
    let dgh = state(&features, "dynamic_gang_header");
    test.check(
        dgh == Some("disabled"),
        &format!(
            "zpool upgrade leaves dynamic_gang_header {} (documented: disabled)",
            pyfmt::option(dgh)
        ),
    );
    let command = dgh_set.first().map_or("false", String::as_str);
    let (rc, _) = run.vm.sh_show(command)?;
    let features = feature_states(&mut run.vm, "zroot")?;
    test.check(
        rc == 0 && has_all_new(&features, "enabled"),
        &format!(
            "Upgrading a Pool: the 2.4 features are enabled ({})",
            new_states(&features)
        ),
    );
    Ok(())
}

/// Upgrades `zoff` with its compatibility property turned off, then
/// enables `dynamic_gang_header`.
fn upgrade_zoff(run: &mut Run<'_>) -> Result<()> {
    let test = run.test;
    run.vm
        .sh_show("zpool set compatibility=off zoff && zpool upgrade zoff")?;
    let features = feature_states(&mut run.vm, "zoff")?;
    test.check(
        is_upgraded(&features),
        "compatibility=off: zpool upgrade enables block_cloning_endian and physical_rewrite",
    );
    let dgh = state(&features, "dynamic_gang_header");
    test.check(
        dgh == Some("disabled"),
        &format!(
            "compatibility=off: zpool upgrade leaves dynamic_gang_header {} \
             (documented: disabled)",
            pyfmt::option(dgh)
        ),
    );
    run.vm
        .sh_show("zpool set feature@dynamic_gang_header=enabled zoff")?;
    let features = feature_states(&mut run.vm, "zoff")?;
    test.check(
        state(&features, "dynamic_gang_header") == Some("enabled"),
        "zpool set feature@dynamic_gang_header=enabled enables it",
    );
    Ok(())
}
