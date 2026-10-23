//! The demo machine's boot health and file changes: what the checks of each
//! environment find, and what `zfs diff` reports for a snapshot.

use crate::menu::world::{Change, ChangeKind, Check, Verdict};

/// A check that passed.
fn pass(name: &str, detail: &str) -> Check {
    Check {
        name: name.to_owned(),
        verdict: Verdict::Pass,
        detail: detail.to_owned(),
        fix: None,
    }
}

/// A check that did not pass, with its fix.
fn problem(name: &str, verdict: Verdict, detail: &str, fix: &str) -> Check {
    Check {
        name: name.to_owned(),
        verdict,
        detail: detail.to_owned(),
        fix: Some(fix.to_owned()),
    }
}

/// The checks of the demo environment `name` booting `kernel`: `tank` is
/// nearly full, and Debian's old ZFS has no module for its kernel and cannot
/// read the pool's newest feature.
pub fn checks(name: &str, kernel: &str) -> Vec<Check> {
    let pair = pass(
        "Kernel and initramfs",
        &format!("{kernel} with its initramfs"),
    );
    let root = pass("Root dataset", "mountpoint / and canmount=noauto");
    let space_ok = pass("Free space", "62% of the pool is free");
    let low = problem(
        "Free space",
        Verdict::Warning,
        "tank has 4% free: writes during boot may fail",
        "destroy old snapshots, or boot it read-only",
    );
    match name {
        "tank/ROOT/arch" => vec![
            pair,
            pass("ZFS module", &format!("zfs 2.4.4 for {kernel}")),
            pass("Pool features", "ZFS 2.4.4 reads every feature of tank"),
            root,
            low,
        ],
        "tank/ROOT/debian" => vec![
            pair,
            problem(
                "ZFS module",
                Verdict::Failure,
                &format!("no zfs.ko for {kernel}: its initramfs cannot import a pool"),
                "rebuild the initramfs in a chroot (j), or boot another environment",
            ),
            problem(
                "Pool features",
                Verdict::Warning,
                "tank uses fast_dedup, which ZFS 2.1.11 of this environment cannot read",
                "upgrade ZFS in this environment before it imports tank read/write",
            ),
            root,
            low,
        ],
        _ => vec![
            pair,
            pass("ZFS module", &format!("zfs 2.4.4 for {kernel}")),
            pass("Pool features", "its ZFS reads every enabled feature"),
            root,
            space_ok,
        ],
    }
}

fn change(kind: ChangeKind, path: &str, preview: &[&str]) -> Change {
    Change {
        kind,
        path: path.to_owned(),
        renamed_to: None,
        preview: preview.iter().map(|line| (*line).to_owned()).collect(),
    }
}

/// What changed since a snapshot of the demo machine, less `restored`.
pub fn changes(restored: &[String]) -> Vec<Change> {
    let mut renamed = change(ChangeKind::Renamed, "/etc/default/zfs", &[]);
    renamed.renamed_to = Some("/etc/default/zfs.bak".to_owned());
    let all = vec![
        change(
            ChangeKind::Modified,
            "/etc/fstab",
            &[
                "-tank/home  /home  zfs  defaults           0 0",
                "+tank/home  /home  zfs  defaults,noauto    0 0",
                "+/dev/sdc1  /mnt/backup  ext4  defaults  0 2",
            ],
        ),
        change(
            ChangeKind::Created,
            "/etc/modprobe.d/nvidia.conf",
            &["+blacklist nouveau", "+options nvidia-drm modeset=1"],
        ),
        change(
            ChangeKind::Modified,
            "/etc/default/grub",
            &["-GRUB_TIMEOUT=5", "+GRUB_TIMEOUT=0"],
        ),
        change(ChangeKind::Removed, "/usr/lib/libdrm.so.2", &[]),
        renamed,
        change(ChangeKind::Modified, "/boot/initramfs-6.12.9_1.img", &[]),
    ];
    all.into_iter()
        .filter(|change| !restored.contains(&change.path))
        .collect()
}
