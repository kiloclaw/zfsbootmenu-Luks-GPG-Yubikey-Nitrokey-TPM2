//! The auxiliary vdevs and the administration of pools: removing a vdev,
//! replacing a failed disk, RAIDZ expansion, attaching and adding vdevs.

use crate::error::Result;
use crate::expect::regex;

use super::session::{ADMIN, Run, VDEV, count};

/// Auxiliary vdevs on the RAIDZ2 pool of Example 5.
///
/// # Errors
///
/// When a step cannot be done.
pub(super) fn auxiliary(run: &mut Run<'_>) -> Result<()> {
    let test = run.test;
    run.doc(VDEV, "Example 5: RAIDZ")?;
    // zpool add accepts a mirror on RAIDZ2 only when it is three-way
    // (parity + 1).
    let (rc, out) = run.vm.sh_show(concat!(
        "VDEVS=(); luks_create /dev/disk/by-id/nvme-FAST{1,2}-part2 && ",
        r#"zpool add tank special mirror "${VDEVS[@]}""#
    ))?;
    test.check(
        rc != 0 && out.contains("mismatched replication level"),
        "zpool add refuses a two-way special mirror on RAIDZ2 (as documented)",
    );
    run.vm
        .sh(r#"for v in "${VDEVS[@]}"; do cryptsetup close "${v##*/}"; done"#)?;
    run.doc(VDEV, "Example 7: Special allocation class")?;
    run.doc(VDEV, "Example 8: Log (SLOG)")?;
    run.doc(VDEV, "Example 9: Cache (L2ARC)")?;
    run.doc(VDEV, "Example 10: Hot spare")?;
    let status = run.status("tank")?;
    for kind in ["special", "dedup", "logs", "cache", "spares"] {
        let pattern = regex(&format!(r"(?m)^\s*{kind}\s*$"))?;
        test.check(
            pattern.is_match(&status),
            &format!("tank has its {kind} vdev"),
        );
    }
    Ok(())
}

/// The administration sections on the pool of [`auxiliary`]: listing its
/// LUKS devices, removing its cache vdev, replacing a failed disk.
///
/// # Errors
///
/// When a step cannot be done.
pub(super) fn administration(run: &mut Run<'_>) -> Result<()> {
    let test = run.test;
    run.doc(ADMIN, "Administration")?;
    let (_, out) = run
        .vm
        .sh_show(r"pool_luks_devices tank | sort | tr '\n' ' '")?;
    let devices = out.split_whitespace().count();
    test.check(
        devices == 16,
        &format!("pool_luks_devices lists the 16 LUKS devices behind tank ({devices})"),
    );
    let (_, cache) = run.vm.sh(
        "zpool status -P tank | awk '/^[[:space:]]*cache[[:space:]]*$/ { getline; print $1 }'",
    )?;
    let cache = cache.trim();
    let name = cache.replace("/dev/mapper/", "");
    run.doc_with(
        ADMIN,
        "Removing an auxiliary vdev",
        &[],
        &[
            ("/dev/mapper/luks-9a72f853-...", cache),
            ("luks-9a72f853-...", &name),
        ],
    )?;
    let status = run.status("tank")?;
    let config = status.rsplit("config:").next().unwrap_or_default();
    test.check(
        !config.contains("cache"),
        "cache vdev removed and its volume closed",
    );
    replace_failed(run)
}

/// A failed disk: takes the sixth RAIDZ member offline, then runs the
/// documented replacement.
///
/// # Errors
///
/// When a step cannot be done.
fn replace_failed(run: &mut Run<'_>) -> Result<()> {
    let test = run.test;
    let (_, failed) = run.vm.sh(
        "zpool status -P tank | awk '/raidz2-0/ { for (i = 0; i < 6; i++) { getline; d = $1 } \
         print d; exit }'",
    )?;
    let failed = failed.trim();
    run.vm.must(&format!("zpool offline tank {failed}"))?;
    let status = run.status("tank")?;
    test.check(
        status.contains("DEGRADED"),
        "tank DEGRADED after a member went offline",
    );
    let replace = format!("zpool replace tank {failed}");
    run.doc_with(
        ADMIN,
        "Replacing a failed disk",
        &[r"^zpool status"],
        &[
            ("zpool replace zroot 12345678901234567890", &replace),
            (
                "zpool wait -t resilver zroot",
                "zpool wait -t resilver tank",
            ),
        ],
    )?;
    let status = run.status("tank")?;
    test.check(
        !status.contains("DEGRADED") && status.contains("state: ONLINE"),
        "replacement resilvered; tank ONLINE",
    );
    run.reset(Some("tank"))
}

/// RAIDZ expansion (OpenZFS 2.3 and later).
///
/// # Errors
///
/// When a step cannot be done.
pub(super) fn expansion(run: &mut Run<'_>) -> Result<()> {
    let test = run.test;
    run.doc(VDEV, "Example 5: RAIDZ")?;
    run.doc(ADMIN, "Expanding a RAIDZ vdev")?;
    let status = run.status("tank")?;
    let members = count(r"/dev/mapper/luks-\S+\s+ONLINE", &status)?;
    test.check(
        members == 7,
        &format!("raidz2-0 has 7 members after expansion ({members})"),
    );
    let (_, value) = run
        .vm
        .sh("zpool get -H -o value feature@raidz_expansion tank")?;
    test.check(value.contains("active"), "feature raidz_expansion active");
    run.reset(Some("tank"))
}

/// Single disk, then a mirror, then striped mirrors.
///
/// # Errors
///
/// When a step cannot be done.
pub(super) fn single_to_mirror(run: &mut Run<'_>) -> Result<()> {
    let test = run.test;
    run.doc(VDEV, "Example 1: Single disk")?;
    let (_, first) = run
        .vm
        .sh(r#"zpool status -P zroot | awk '$1 ~ "^/dev/mapper/" { print $1; exit }'"#)?;
    run.doc_with(
        ADMIN,
        "Turning a single disk into a mirror",
        &[],
        &[("/dev/mapper/luks-5d0c3a1e-...", first.trim())],
    )?;
    run.doc(ADMIN, "Adding a vdev")?;
    let status = run.status("zroot")?;
    test.check(
        status.contains("mirror-0")
            && status.contains("mirror-1")
            && status.contains("state: ONLINE"),
        "single disk -> mirror (attach) -> striped mirrors (add)",
    );
    run.reset(Some("zroot"))
}
