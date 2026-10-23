//! Example 11, made bootable, and the documented verification after a
//! reboot (Step 4).

use crate::boot::{Boot, Follow, check_teardown, follow_boot};
use crate::error::Result;
use crate::expect::regex;
use crate::images::Image;
use crate::qemu::{Disk, Machine};
use crate::record::Test;
use crate::vm::Vm;

use super::session::{Run, VDEV, count};

/// Example 11 and making the pool bootable; powers the machine off.
///
/// # Errors
///
/// When a step cannot be done.
pub(super) fn bootable(mut run: Run<'_>) -> Result<()> {
    let test = run.test;
    run.doc(VDEV, "Example 11: A complete layout")?;
    run.doc(VDEV, "Step 1: Create a boot environment")?;
    run.doc(VDEV, "Step 2: Let the installed system unlock the volumes")?;
    let (_, crypttab) = run.vm.sh_show("cat /etc/crypttab")?;
    test.check(
        count(
            r"(?m)^luks-[0-9a-f-]{36} UUID=[0-9a-f-]{36} none luks,discard$",
            &crypttab,
        )? == 11,
        "crypttab loop wrote 11 valid entries",
    );
    run.vm.must(
        "mkdir -p /be /pl; grep -q ' /pl ' /proc/mounts || \
         mount -o ro /dev/disk/by-label/PAYLOAD /pl",
    )?;
    run.vm.must(
        "mount -t zfs -o zfsutil zroot/ROOT/os /be && cp -a /pl/rootfs/. /be/ && \
         mkdir -p /be/boot && cp /pl/boot/vmlinuz-* /pl/boot/initramfs-* /be/boot/ && umount /be",
    )?;
    run.vm.must("finish")?;
    run.vm.poweroff()
}

/// Step 4: boots again with automatic unlocking and checks what
/// ZFSBootMenu shows.
///
/// # Errors
///
/// When the machine cannot be started or the boot does not go as awaited.
pub(super) fn verify(test: &Test, image: &Image, disks: Vec<Disk>) -> Result<()> {
    let machine = Machine::boot(image, "console=ttyS0 zbm.timeout=3").with_disks(disks);
    let mut vm = Vm::start(test, "verify", &machine)?;
    let follow = Follow {
        no_be: true,
        ..Follow::default()
    };
    let boot = follow_boot(&mut vm, follow)?;
    check_boot(test, &boot)?;
    vm.kill();
    Ok(())
}

/// The checks of Step 4.
///
/// # Errors
///
/// Only for an invalid pattern.
pub(super) fn check_boot(test: &Test, boot: &Boot) -> Result<()> {
    test.check(
        boot.prompts == ["luks"],
        &format!(
            "one passphrase prompt for the 11 volumes ({})",
            boot.prompts_text()
        ),
    );
    let status = &boot.zbm_status;
    test.check(
        boot.pools().get("zroot") == Some("ONLINE"),
        "Step 4: zroot ONLINE",
    );
    let cache = regex(r"cache\s*\n\s*/dev/mapper/luks-\S+\s+ONLINE")?;
    test.check(cache.is_match(status), "Step 4: cache ONLINE");
    let spare = regex(r"spares\s*\n\s*/dev/mapper/luks-\S+\s+AVAIL")?;
    test.check(spare.is_match(status), "Step 4: spare AVAIL");
    test.check(
        !status.contains("UNAVAIL") && !status.contains("DEGRADED"),
        "Step 4: nothing UNAVAIL",
    );
    test.check(
        boot.zluks.matches("zroot").count() >= 11,
        "Step 4: zluks status maps the 11 volumes to zroot",
    );
    check_teardown(test, boot, &[]);
    Ok(())
}
