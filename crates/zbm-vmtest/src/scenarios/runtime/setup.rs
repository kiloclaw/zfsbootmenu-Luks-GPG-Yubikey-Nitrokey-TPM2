//! `runtime-setup`: the pools `tplain` (d0, exported by this host) and
//! `tforeign` (d1, left imported by the host `deadbeef`), each holding the
//! test boot environment as its bootfs, and `tkeys` (d2), whose boot
//! environment's key comes from a key source (`keysource`). Only `tplain`'s
//! environment has an `/etc/resolv.conf`: systemd-resolved's link into its
//! empty `/run` ([`RESOLV_LINK`], for `resolv`). `tplain` also holds
//! [`CRAFTED`], a filesystem that is no boot environment, with the
//! payload's static busybox, copied in, as its `/bin/sh` and its `tmp` and
//! `dev` links out of it ([`TMP_LINK`], [`DEV_LINK`], for `recovery`'s
//! chroot of it), its `mnt` and `kernels` links out of it ([`MNT_LINK`],
//! [`KERNELS_LINK`]) beside its child [`CRAFTED_MNT`], for `recovery`'s
//! kernel searches of that child, and a `/boot` ([`crafted_boot`]) whose
//! files link out of it, a FIFO and a kernel name with a newline among
//! them, for `recovery`'s search and boot of it.

use std::fmt::Write as _;

use crate::error::Result;
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::{ZFSKEY, ZbmRuntime};
use crate::vm::Vm;

/// The options of the pools.
pub(crate) const OPTS: &str = "-o ashift=12 -o compatibility=openzfs-2.4-linux -O compression=lz4 \
                    -O acltype=posixacl -O xattr=sa -m none";

/// The command line of the boot environments.
pub(crate) const BE_CMDLINE: &str = "console=ttyS0 loglevel=4";

/// The host ID of the other host, `deadbeef`, as `/etc/hostid` holds it.
const FOREIGN: [u8; 4] = [0xef, 0xbe, 0xad, 0xde];

/// The pools, in the order of their disks.
const POOLS: [&str; 2] = ["tplain", "tforeign"];

/// Where `tplain`'s environment's `/etc/resolv.conf` leads: the stub of
/// systemd-resolved, below the environment's `/run`, which is empty.
pub(crate) const RESOLV_LINK: &str = "../run/systemd/resolve/stub-resolv.conf";

/// A filesystem of `tplain` that is no boot environment, whose mount
/// point `/crafted` is never mounted by itself (`canmount=noauto`): it
/// holds `bin/busybox`, `bin/sh`, its link to it, the directories `proc`,
/// `sys` and `etc`, the links `tmp` ([`TMP_LINK`]), `dev`
/// ([`DEV_LINK`]), `mnt` ([`MNT_LINK`]) and `kernels` ([`KERNELS_LINK`]),
/// its own `/etc/zfsbootmenu.conf` ([`CRAFTED_OWN`]) and a `/boot`
/// ([`crafted_boot`]).
pub(crate) const CRAFTED: &str = "tplain/crafted";

/// What [`CRAFTED`]'s own `/etc/zfsbootmenu.conf` holds, a line: its
/// `/boot/vmlinuz-3.kcl` is an absolute link to that file, whose name in
/// the image is ZFSBootMenu's configuration.
pub(crate) const CRAFTED_OWN: &str = "crafted-own";

/// The child of [`CRAFTED`] named `mnt`, no boot environment either, with
/// no `/boot`, never mounted by itself (`canmount=noauto`): its name puts
/// its mount point and kernel list, in the legacy layout, on [`CRAFTED`]'s
/// links `mnt` and `kernels` while [`CRAFTED`] is mounted.
pub(crate) const CRAFTED_MNT: &str = "tplain/crafted/mnt";

/// Where [`CRAFTED`]'s `tmp` leads: the image's `/etc`, by an absolute
/// link.
pub(crate) const TMP_LINK: &str = "/etc";

/// Where [`CRAFTED`]'s `dev` leads: the image's `/etc/zfs`, by a relative
/// link that climbs to `/` from the chroot's mount point, followed in the
/// image.
pub(crate) const DEV_LINK: &str = "../../../../../../../etc/zfs";

/// Where [`CRAFTED`]'s `mnt` leads: the image's `/etc`, by an absolute
/// link.
pub(crate) const MNT_LINK: &str = "/etc";

/// Where [`CRAFTED`]'s `kernels` leads: a file of the image that does not
/// exist, by an absolute link; a kernel list written through it makes it.
pub(crate) const KERNELS_LINK: &str = "/tmp/crafted-kernels";

/// The passphrase in the key file of `tkeys`'s boot environment.
pub(super) const BE_KEY: &str = "tkeys-be-key";

/// Creates the pools in the recovery shell.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let payload = super::super::payload(settings)?;
    let image = super::image(settings, "auto", ZbmRuntime::Legacy, None)?;
    let mut disks = super::super::disks(settings, "runtime", 0..3, true)?;
    disks.push(payload);
    let machine = Machine::boot(&image, super::SHELL_APPEND).with_disks(disks);
    let mut vm = Vm::start(test, "setup", &machine)?;
    vm.recovery_shell()?;
    vm.send_functions()?;
    for (index, pool) in POOLS.iter().enumerate() {
        let (rc, _) = vm.sh(&format!(
            "zpool create -f {OPTS} {pool} /dev/disk/by-id/virtio-d{index}"
        ))?;
        test.check(rc == 0, &format!("zpool create {pool}"));
        vm.must(&format!("mkbe {pool}"))?;
        vm.must(&format!(
            "zfs set org.zfsbootmenu:commandline='{BE_CMDLINE}' {pool}/ROOT/be"
        ))?;
    }
    vm.must(&resolver_link())?;
    vm.must(&crafted())?;
    keyed_pool(test, &mut vm)?;
    vm.must("finish")?;
    // tforeign belongs to another host and stays imported, as after a crash
    vm.must(&foreign_import())?;
    let (rc, out) = vm.sh_show("od -An -tx4 /etc/hostid; zpool list -H -o name")?;
    test.check(
        rc == 0 && out.contains("deadbeef"),
        "tforeign imported by host deadbeef",
    );
    vm.sh("sync")?;
    vm.poweroff()
}

/// Creates `tkeys` on d2: the key source `tkeys/keystore`, encrypted with
/// the passphrase [`ZFSKEY`] and mounted at `/etc/zfs/keys`, holds the key
/// file of the boot environment, which is encrypted with it and names the
/// key source. The ZFSBootMenu images do not have the key file.
fn keyed_pool(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let (rc, _) = vm.sh(&format!(
        "zpool create -f {OPTS} tkeys /dev/disk/by-id/virtio-d2"
    ))?;
    test.check(rc == 0, "zpool create tkeys");
    vm.must(&format!(
        "printf '{ZFSKEY}\\n{ZFSKEY}\\n' | zfs create -o encryption=on -o keyformat=passphrase \
         -o keylocation=prompt -o mountpoint=/etc/zfs/keys tkeys/keystore"
    ))?;
    vm.must(&format!("printf '{BE_KEY}\\n' > /etc/zfs/keys/be.key"))?;
    vm.must(
        "mkbe tkeys -o encryption=on -o keyformat=passphrase \
         -o keylocation=file:///etc/zfs/keys/be.key",
    )?;
    vm.must(&format!(
        "zfs set org.zfsbootmenu:keysource=tkeys/keystore \
         org.zfsbootmenu:commandline='{BE_CMDLINE}' tkeys/ROOT/be"
    ))?;
    Ok(())
}

/// The command that makes `tplain`'s `/etc/resolv.conf` the link
/// [`RESOLV_LINK`], with the environment mounted on `/be` meanwhile, as
/// `mkbe` mounts it; it fails when the link cannot be made.
fn resolver_link() -> String {
    format!(
        "mount -t zfs -o zfsutil tplain/ROOT/be /be && \
         {{ ln -s {RESOLV_LINK} /be/etc/resolv.conf; made=$?; umount /be && [ $made -eq 0 ]; }}"
    )
}

/// The command that makes [`CRAFTED`] on `tplain`, mounted on `/c`
/// meanwhile, with the busybox of the payload, which is static, while the
/// payload is mounted on `/pl`, and its `/boot` ([`crafted_boot`]), then
/// its child [`CRAFTED_MNT`]; it fails when a part cannot be made.
fn crafted() -> String {
    format!(
        "zfs create -o mountpoint=/crafted -o canmount=noauto {CRAFTED} && mkdir -p /c && \
         mount -t zfs -o zfsutil {CRAFTED} /c && \
         {{ mkdir -p /c/bin /c/proc /c/sys /c/etc && cp /pl/rootfs/bin/busybox /c/bin/ && \
         ln -s /bin/busybox /c/bin/sh && ln -s {TMP_LINK} /c/tmp && ln -s {DEV_LINK} /c/dev && \
         ln -s {MNT_LINK} /c/mnt && ln -s {KERNELS_LINK} /c/kernels && {}; \
         made=$?; umount /c && [ $made -eq 0 ]; }} && \
         zfs create -o canmount=noauto {CRAFTED_MNT}",
        crafted_boot()
    )
}

/// The command that makes [`CRAFTED`]'s `/boot`, mounted on `/c`, whose
/// files are empty but for the links (ROADMAP.md SB20):
///
/// - `vmlinuz-3` and `initramfs-3.img`, with `vmlinuz-3.kcl` an absolute
///   link to `/etc/zfsbootmenu.conf`, which [`CRAFTED`] has, holding
///   [`CRAFTED_OWN`];
/// - `vmlinuz-4`, with `initramfs-4.img` an absolute link to `/etc/hostid`,
///   which [`CRAFTED`] does not have;
/// - `vmlinuz-6` and `initramfs-6.img`, with `vmlinuz-6.kcl` a FIFO, which
///   nothing writes to;
/// - `vmlinuz-9`, a newline, then `tplain`, `x` and `y` after tabs, and its
///   initramfs of that name: a line of a kernel list naming `tplain`'s
///   root dataset, if the name were written raw. It sorts last.
///
/// The shell's `$'…'` makes the newline and the tabs, so none is typed.
fn crafted_boot() -> String {
    format!(
        "mkdir -p /c/boot && : > /c/boot/vmlinuz-3 && : > /c/boot/initramfs-3.img && \
         printf '%s\\n' {CRAFTED_OWN} > /c/etc/zfsbootmenu.conf && \
         ln -s /etc/zfsbootmenu.conf /c/boot/vmlinuz-3.kcl && \
         : > /c/boot/vmlinuz-4 && ln -s /etc/hostid /c/boot/initramfs-4.img && \
         : > /c/boot/vmlinuz-6 && : > /c/boot/initramfs-6.img && mkfifo /c/boot/vmlinuz-6.kcl && \
         : > $'/c/boot/vmlinuz-9\\ntplain\\tx\\ty' && \
         : > $'/c/boot/initramfs-9\\ntplain\\tx\\ty.img'"
    )
}

/// The command that writes the other host's ID and imports `tforeign`.
pub(crate) fn foreign_import() -> String {
    let hostid = FOREIGN.iter().fold(String::new(), |mut out, byte| {
        write!(out, "\\x{byte:02x}").ok();
        out
    });
    format!("printf '{hostid}' > /etc/hostid && zpool import -N tforeign")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn tplain_s_resolv_conf_is_systemd_resolved_s_link_made_while_mounted() {
        assert_eq!(
            resolver_link(),
            "mount -t zfs -o zfsutil tplain/ROOT/be /be && { ln -s \
             ../run/systemd/resolve/stub-resolv.conf /be/etc/resolv.conf; made=$?; \
             umount /be && [ $made -eq 0 ]; }"
        );
    }

    #[test]
    fn tplain_s_crafted_filesystem_links_out_of_it_has_a_boot_made_mounted_then_its_child_mnt() {
        assert_eq!(
            crafted(),
            "zfs create -o mountpoint=/crafted -o canmount=noauto tplain/crafted && \
             mkdir -p /c && mount -t zfs -o zfsutil tplain/crafted /c && { mkdir -p /c/bin \
             /c/proc /c/sys /c/etc && cp /pl/rootfs/bin/busybox /c/bin/ && ln -s /bin/busybox \
             /c/bin/sh && ln -s /etc /c/tmp && ln -s ../../../../../../../etc/zfs /c/dev && \
             ln -s /etc /c/mnt && ln -s /tmp/crafted-kernels /c/kernels && mkdir -p /c/boot && \
             : > /c/boot/vmlinuz-3 && : > /c/boot/initramfs-3.img && \
             printf '%s\\n' crafted-own > /c/etc/zfsbootmenu.conf && \
             ln -s /etc/zfsbootmenu.conf /c/boot/vmlinuz-3.kcl && : > /c/boot/vmlinuz-4 && \
             ln -s /etc/hostid /c/boot/initramfs-4.img && : > /c/boot/vmlinuz-6 && \
             : > /c/boot/initramfs-6.img && mkfifo /c/boot/vmlinuz-6.kcl && \
             : > $'/c/boot/vmlinuz-9\\ntplain\\tx\\ty' && \
             : > $'/c/boot/initramfs-9\\ntplain\\tx\\ty.img'; \
             made=$?; umount /c && [ $made -eq 0 ]; } && \
             zfs create -o canmount=noauto tplain/crafted/mnt"
        );
    }

    #[test]
    fn tplain_s_crafted_boot_is_typed_without_a_newline_or_tab_and_made_below_c() {
        let boot = crafted_boot();

        assert!(!boot.contains(['\n', '\t']), "{boot}");
        for part in boot.split(" && ") {
            let made = part.rsplit(' ').next().unwrap_or_default();
            let made = made.trim_start_matches("$'");
            assert!(made.starts_with("/c/"), "{part}");
        }
    }

    #[test]
    fn the_other_host_id_is_written_little_endian() {
        assert_eq!(
            foreign_import(),
            r"printf '\xef\xbe\xad\xde' > /etc/hostid && zpool import -N tforeign"
        );
    }
}
