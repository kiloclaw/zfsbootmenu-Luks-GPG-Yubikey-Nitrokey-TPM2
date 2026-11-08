//! `menu-setup`: the pools of the menu tests, made in the recovery shell
//! of the legacy image, as `runtime-setup` makes its own with the helpers
//! of `tests/luks/vm/functions.sh`; then the pristine copies of their
//! disks, which each test copies again.
//!
//! - `tmenu` (d0) holds the test boot environment, with a second kernel,
//!   `0.0.1-zbmtest`, older by its version, a copy of the first with its
//!   initramfs; and in `/zbmtest`, `marker`, `restore-me` and `removed`,
//!   owned by 1234:5678, mode 0640. Its snapshots, a second apart, oldest
//!   first: `one`; `two`, cloned as `tmenu/two-clone`; `held`, held by
//!   `zbmtest-hold`; `three`, with the bookmark `#three`; then `restore-me`
//!   changes and `removed` goes; `four`; `five`. A rollback to `four` keeps
//!   the changes, which a diff against `one` shows.
//! - `tother` (d1): a checkpoint, then `tother/after`.
//! - `tlocked` (d3), in a LUKS2 volume of a fixed UUID, the passphrase
//!   `pw` and a fast key derivation.
//! - `tforeign` (d2), left imported by the host `deadbeef`.

use crate::error::Result;
use crate::qemu::{Disk, Machine};
use crate::record::Test;
use crate::settings::{Settings, ZbmRuntime};
use crate::vm::Vm;

use super::super::runtime::setup::{BE_CMDLINE, OPTS, foreign_import};
use super::super::runtime::{SHELL_APPEND, image};
use super::checks::RESTORED;
use super::{DISKS, GROUP, UUID, pristine};

/// What `restore-me` holds after `three`.
const CHANGED: &str = "changed after three";

/// The size of `tmenu`'s disk, d0: its boot environment, with the second
/// kernel, twice, as a duplicate makes it, beyond the 128 MiB at least
/// that ZFS keeps back. The others' is the 384M of every group.
const TMENU_SIZE: &str = "1G";

/// The commands that make the pools, in order, before they are exported.
fn commands() -> Vec<String> {
    let be = "tmenu/ROOT/be";
    let files = "/be/zbmtest";
    vec![
        format!("zpool create -f {OPTS} tmenu /dev/disk/by-id/virtio-d0"),
        "mkbe tmenu".to_owned(),
        format!("zfs set org.zfsbootmenu:commandline='{BE_CMDLINE}' {be}"),
        format!("mkdir -p /be && mount -t zfs -o zfsutil {be} /be"),
        "(cd /be/boot && k=$(echo vmlinuz-*) && cp $k vmlinuz-0.0.1-zbmtest \
         && cp initramfs-${k#vmlinuz-}.img initramfs-0.0.1-zbmtest.img)"
            .to_owned(),
        format!(
            "mkdir -p {files} && echo marker > {files}/marker \
             && echo '{RESTORED}' > {files}/restore-me && echo removed > {files}/removed \
             && chown 1234:5678 {files}/* && chmod 0640 {files}/*"
        ),
        format!("zfs snapshot {be}@one"),
        format!("sleep 1; zfs snapshot {be}@two && zfs clone {be}@two tmenu/two-clone"),
        format!("sleep 1; zfs snapshot {be}@held && zfs hold zbmtest-hold {be}@held"),
        format!("sleep 1; zfs snapshot {be}@three && zfs bookmark {be}@three {be}#three"),
        format!("echo '{CHANGED}' > {files}/restore-me && rm {files}/removed"),
        format!("sleep 1; zfs snapshot {be}@four"),
        format!("sleep 1; zfs snapshot {be}@five"),
        "umount /be".to_owned(),
        format!("zpool create -f {OPTS} tother /dev/disk/by-id/virtio-d1"),
        "zpool checkpoint tother && zfs create tother/after".to_owned(),
        format!(
            "mkluks pw '--pbkdf pbkdf2 --pbkdf-force-iterations 1000 --uuid {UUID}' \
             /dev/disk/by-id/virtio-d3"
        ),
        format!("zpool create -f {OPTS} tlocked /dev/mapper/luks-{UUID}"),
        format!("zpool create -f {OPTS} tforeign /dev/disk/by-id/virtio-d2"),
    ]
}

/// Creates the pools in the recovery shell, then keeps the disks.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let payload = super::super::payload(settings)?;
    let image = image(settings, "auto", ZbmRuntime::Legacy, None)?;
    let mut disks = empty_disks(settings)?;
    let paths: Vec<_> = disks.iter().map(|disk| disk.path.clone()).collect();
    disks.push(payload);
    let machine = Machine::boot(&image, SHELL_APPEND).with_disks(disks);
    let mut vm = Vm::start(test, "setup", &machine)?;
    vm.recovery_shell()?;
    vm.send_functions()?;
    make_pools(test, &mut vm)?;
    vm.sh("sync")?;
    vm.poweroff()?;
    for (index, path) in paths.iter().enumerate() {
        crate::disks::copy_sparse(path, &pristine(settings, index))?;
    }
    test.step("the disks are kept as pristine copies");
    Ok(())
}

/// The fresh, empty disks d0 to d3.
fn empty_disks(settings: &Settings) -> Result<Vec<Disk>> {
    let tmenu = crate::disks::disk(settings, GROUP, 0, TMENU_SIZE, true)?;
    let others = super::super::disks(settings, GROUP, 1..DISKS, true)?;
    Ok([Disk::new(tmenu, "d0")].into_iter().chain(others).collect())
}

/// Makes the pools, exports them and closes the volume, then leaves
/// `tforeign` imported by the other host.
fn make_pools(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    for command in commands() {
        vm.must(&command)?;
    }
    vm.must("finish")?;
    vm.must(&foreign_import())?;
    let (code, out) = vm.sh_show("od -An -tx4 /etc/hostid; zpool list -H -o name")?;
    let pools: Vec<&str> = out.lines().skip(1).map(str::trim).collect();
    test.check(
        code == 0 && out.contains("deadbeef") && pools == ["tforeign"],
        "tforeign imported by host deadbeef, and no other pool",
    );
    Ok(())
}

#[cfg(test)]
mod tests;
