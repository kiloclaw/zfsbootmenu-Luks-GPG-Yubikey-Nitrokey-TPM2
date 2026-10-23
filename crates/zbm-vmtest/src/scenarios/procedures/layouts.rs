//! The preparation of the LUKS volumes, the data vdev layouts and the NVMe
//! mirror of Example 12.

use crate::error::Result;

use super::session::{Run, VDEV, count};

/// The data vdev layouts: section, pool and the vdevs its status shows.
const LAYOUTS: &[(&str, &str, &[&str])] = &[
    ("Example 1: Single disk", "zroot", &[]),
    ("Example 2: Two-way mirror", "zroot", &["mirror-0"]),
    ("Example 3: Three-way mirror", "zroot", &["mirror-0"]),
    (
        "Example 4: Striped mirrors",
        "zroot",
        &["mirror-0", "mirror-1"],
    ),
    ("Example 5: RAIDZ", "tank", &["raidz2-0"]),
    (
        "Example 6: dRAID",
        "tank",
        &["draid2:4d:7c:1s-0", "draid2-0-0"],
    ),
];

/// Conventions and preparation.
///
/// # Errors
///
/// When a step cannot be done.
pub(super) fn preparation(run: &mut Run<'_>) -> Result<()> {
    let test = run.test;
    run.doc(VDEV, "Conventions")?;
    let (_, out) = run
        .vm
        .sh_show(r#"printf "%s\n" "${POOL_OPTS[@]}" | grep compatibility"#)?;
    test.check(
        out.contains("openzfs-2.4-linux"),
        "POOL_OPTS uses the OpenZFS 2.4 compatibility set",
    );
    run.doc(
        VDEV,
        "Step 2: Define a helper that creates and opens LUKS volumes",
    )?;
    let out = run.doc(VDEV, "Step 3: Create the LUKS volumes")?;
    test.check(
        count(r"(?m)^/dev/mapper/luks-[0-9a-f-]{36}$", &out)? == 2,
        "VDEVS holds two luks-<UUID> devices",
    );
    let (_, out) = run
        .vm
        .sh_show("cryptsetup luksDump /dev/disk/by-id/ata-DISK1-part2 | grep -E '^Flags|PBKDF'")?;
    test.check(
        out.contains("allow-discards"),
        "--allow-discards --persistent stored the TRIM flag",
    );
    test.check(out.contains("argon2id"), "default PBKDF argon2id");
    run.reset(None)
}

/// The data vdev layouts, each created, checked and destroyed.
///
/// # Errors
///
/// When a step cannot be done.
pub(super) fn data_layouts(run: &mut Run<'_>) -> Result<()> {
    let test = run.test;
    for (section, pool, vdevs) in LAYOUTS {
        run.doc(VDEV, section)?;
        let status = run.status(pool)?;
        let shown = if vdevs.is_empty() {
            "a single disk".to_owned()
        } else {
            vdevs.join(", ")
        };
        test.check(
            status.contains("state: ONLINE") && vdevs.iter().all(|vdev| status.contains(vdev)),
            &format!("{section}: {pool} ONLINE with {shown}"),
        );
        let (_, value) = run
            .vm
            .sh(&format!("zpool get -H -o value compatibility {pool}"))?;
        test.check(
            value.contains("openzfs-2.4-linux"),
            &format!("{section}: compatibility openzfs-2.4-linux"),
        );
        run.reset(Some(pool))?;
    }
    Ok(())
}

/// Example 12: an NVMe mirror without a crypttab; the `nvme-cli` and
/// installed-system steps are not run here.
///
/// # Errors
///
/// When a step cannot be done.
pub(super) fn nvme_mirror(run: &mut Run<'_>) -> Result<()> {
    let test = run.test;
    run.doc_with(
        VDEV,
        "Example 12: A mirror of two NVMe SSDs, unlocked without a crypttab",
        &[
            r"^nvme ",
            r"^zfs set org\.zfsbootmenu",
            r"^install ",
            r"^dd ",
            r"^cat /etc/dracut",
            r"cryptsetup luksAddKey",
        ],
        &[],
    )?;
    let (_, out) = run
        .vm
        .sh_show("cryptsetup luksDump /dev/disk/by-id/nvme-SSD1-part2 | grep -E '^Flags|sector'")?;
    test.check(
        out.contains("4096") && out.contains("no-read-workqueue") && out.contains("allow-discards"),
        "Example 12: 4096-byte sectors and flags stored in the header",
    );
    let status = run.status("zroot")?;
    test.check(
        status.contains("mirror-0"),
        "Example 12: NVMe mirror created",
    );
    run.reset(Some("zroot"))
}
