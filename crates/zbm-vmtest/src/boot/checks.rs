//! The checks common to the scenarios: of a boot that reached the test
//! boot environment, and of the teardown before `kexec`.

use super::{Boot, pools};
use crate::record::Test;

/// The checks of a boot that reached the test boot environment.
pub fn check_be(
    test: &Test,
    boot: &Boot,
    root_pool: &str,
    pools: &[&str],
    states: &[(&str, &str)],
) {
    let state_of = |pool: &str| {
        states
            .iter()
            .find(|(name, _)| *name == pool)
            .map_or("ONLINE", |(_, state)| *state)
    };
    test.check(
        !boot.has_prompt("BE-INIT-START") && !boot.be.is_empty(),
        "kexec into the ZFS 2.4.4 boot environment",
    );
    if boot.be.is_empty() {
        return;
    }
    let kernel = boot.be_line("BE-KERNEL");
    test.check(
        kernel.starts_with("7.3"),
        &format!("boot environment kernel {kernel}"),
    );
    let zfs = boot.be_line("BE-ZFS-VERSION");
    test.check(
        zfs.contains("zfs-2.4.4") && zfs.contains("zfs-kmod-2.4.4"),
        &format!("boot environment runs ZFS 2.4.4 ({zfs})"),
    );
    let root = boot.be_line("BE-ROOT");
    test.check(
        root.starts_with(&format!("{root_pool}/")) && format!(" {root} ").contains(" zfs "),
        &format!("root file system {root}"),
    );
    let be_pools = self::pools(&boot.be_status());
    for pool in pools {
        let want = state_of(pool);
        let got = be_pools.show(pool);
        test.check(
            be_pools.get(pool) == Some(want),
            &format!("boot environment imported {pool}: {got} (expected {want})"),
        );
    }
    test.check(
        boot.be_line("BE-WRITE") == "ok",
        "root dataset is writable in the boot environment",
    );
    for line in boot.be_lines.get("BE-SCRUB").into_iter().flatten() {
        let pool = line.split_whitespace().next().unwrap_or_default();
        let healthy = line.contains("is healthy") || line.contains("errors: No known data errors");
        if state_of(pool) == "ONLINE" {
            let rest: String = line[pool.len()..].trim().chars().take(80).collect();
            test.check(
                healthy,
                &format!("scrub of {pool} in the boot environment: {rest}"),
            );
        }
    }
}

/// Every LUKS mapping closed and every pool exported before kexec, except
/// `readonly_pools`: pools imported read-only through unencrypted devices
/// only, which ZFSBootMenu leaves imported as it does without LUKS.
pub fn check_teardown(test: &Test, boot: &Boot, readonly_pools: &[&str]) {
    let Some((dm, pools, state)) = &boot.teardown else {
        test.check(false, "teardown report missing");
        return;
    };
    let mut left: Vec<&str> = pools
        .split_whitespace()
        .filter(|pool| !readonly_pools.contains(pool))
        .collect();
    left.sort_unstable();
    left.dedup();
    test.check(
        dm.is_empty() && left.is_empty() && state.is_empty(),
        &format!(
            "teardown exported pools and closed volumes (dm=[{dm}] pools=[{pools}] state=[{state}])"
        ),
    );
}
