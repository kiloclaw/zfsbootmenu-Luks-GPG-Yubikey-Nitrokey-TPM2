//! The scenarios that boot the complex pools: `healthy`, `crypttab`,
//! `degraded`, `hotplug`, `hotplug-auto` and `badkey`.

use std::fs;
use std::thread;
use std::time::Duration;

use crate::boot::{Follow, Kind, follow_boot};
use crate::error::{Error, Result};
use crate::images::{Image, ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::{PW, PW_OTHER, ZFSKEY};
use crate::vm::Vm;

use super::healthy::{Expect, be_cmdline, expect_healthy};
use super::{BASE, POOLS, crypttab as crypttab_path, disk, disks};

/// Starts the machine of `scenario`: `image` booted with `append`, without
/// the disks in `skip`.
fn start<'t>(
    test: &'t Test,
    scenario: &str,
    image: &Image,
    append: &str,
    skip: &[usize],
) -> Result<Vm<'t>> {
    let machine = Machine::boot(image, append).with_disks(disks(test.settings(), skip)?);
    Vm::start(test, scenario, &machine)
}

/// The answers of the boots: the passphrase, then the other one.
fn passphrases<'a>() -> Follow<'a> {
    Follow::default().answer(Kind::Luks, &[PW, PW_OTHER])
}

/// Automatic unlocking: two passphrases, the ZFS key in ZFSBootMenu and in
/// the boot environment; every pool ONLINE.
pub(super) fn healthy(test: &Test, auto: &Image) -> Result<()> {
    let mut vm = start(test, "healthy", auto, BASE, &[])?;
    let boot = follow_boot(&mut vm, passphrases())?;
    test.check(
        boot.count("luks") == 2,
        &format!(
            "two LUKS prompts: one passphrase for 14 volumes, one for the volume with its own \
             ({})",
            boot.prompts_text()
        ),
    );
    test.check(
        boot.count("zfskey") == 1,
        "ZFSBootMenu asked once for the ZFS key",
    );
    expect_healthy(test, &boot, Expect::default())
}

/// The same through an embedded crypttab.
pub(super) fn crypttab(test: &Test) -> Result<()> {
    let settings = test.settings();
    let path = crypttab_path(settings);
    let text =
        fs::read_to_string(&path).map_err(Error::io(format!("reading {}", path.display())))?;
    let spec = ImageSpec {
        crypttab: Some(text),
        ..ImageSpec::new("complex-crypttab")
    };
    let image = zbm_image(settings, &spec)?;
    let mut vm = start(test, "crypttab", &image, BASE, &[])?;
    let boot = follow_boot(&mut vm, passphrases())?;
    test.check(
        boot.count("luks") == 2,
        &format!(
            "two LUKS prompts in crypttab mode ({})",
            boot.prompts_text()
        ),
    );
    expect_healthy(test, &boot, Expect::default())
}

/// A RAIDZ2 member and the plain mirror half are missing.
pub(super) fn degraded(test: &Test, auto: &Image) -> Result<()> {
    // The boot environment's initramfs must not wait for the missing
    // volume: its command line for this boot leaves it out, as MOD+E in
    // ZFSBootMenu would (docs/luks/administration.rst).
    let skip = [1, 13];
    let kcl = be_cmdline(test.settings(), &skip)?;
    let append = format!("{BASE} zbm.kcl_override=\"{kcl}\"");
    let mut vm = start(test, "degraded", auto, &append, &skip)?;
    let boot = follow_boot(&mut vm, passphrases())?;
    let states = [("tank", "DEGRADED"), ("mixed", "DEGRADED")];
    let expect = Expect {
        states: &states,
        ..Expect::default()
    };
    expect_healthy(test, &boot, expect)
}

/// `zbm.luks=` lists every volume; d0 appears while ZFSBootMenu waits for
/// it.
pub(super) fn hotplug(test: &Test, auto: &Image) -> Result<()> {
    let mut specs: Vec<String> = (0..13)
        .chain([14])
        .map(|index| format!("ID=virtio-d{index}"))
        .collect();
    specs.push("PARTLABEL=zfsA".to_owned());
    let append = format!("{BASE} zbm.luks={}", specs.join(","));
    let d0 = disk(test.settings(), 0)?;
    let mut vm = start(test, "hotplug", auto, &append, &[0])?;
    let follow = passphrases().on(Kind::Missing, move |vm| vm.hotplug_disk(&d0, "d0"));
    let boot = follow_boot(&mut vm, follow)?;
    test.check(
        boot.saw("luks"),
        &format!("prompts: {}", boot.prompts_text()),
    );
    expect_healthy(test, &boot, Expect::default())
}

/// Only the special, dedup, log, cache and spare disks are present; the
/// rest appear during the import retry loop.
///
/// All late disks, LUKS volumes and the unencrypted halves of mixed and
/// parts, are plugged in within one retry interval: the retry must unlock
/// the late volumes before it imports. Disks that arrive after a retry has
/// imported the pools DEGRADED are not used, as the loop ends once a pool
/// imports (see LATE DEVICES in zfsbootmenu-luks(7)).
pub(super) fn hotplug_auto(test: &Test, auto: &Image) -> Result<()> {
    let late = [0, 1, 2, 3, 12, 13, 14, 15];
    let mut plugged = Vec::new();
    for index in late {
        plugged.push((disk(test.settings(), index)?, format!("d{index}")));
    }
    let append = "console=ttyS0 zbm.timeout=3 zbm.retry_delay=60";
    let mut vm = start(test, "hotplug-auto", auto, append, &late)?;
    let follow = Follow::default()
        .answer(Kind::Luks, &[PW, PW, PW_OTHER])
        .on(Kind::Retry, move |vm| {
            vm.test()
                .step("ZFSBootMenu is in its import retry loop; adding the remaining disks");
            for (path, serial) in &plugged {
                vm.hotplug_disk(path, serial)?;
                thread::sleep(Duration::from_secs(1));
            }
            Ok(())
        });
    let boot = follow_boot(&mut vm, follow)?;
    test.check(
        boot.count("luks") >= 2,
        &format!(
            "late volumes were unlocked during the retry loop ({})",
            boot.prompts_text()
        ),
    );
    expect_healthy(test, &boot, Expect::default())
}

/// A wrong ZFS key: the recovery shell, with the volumes and pools
/// available.
pub(super) fn badkey(test: &Test, auto: &Image) -> Result<()> {
    let mut vm = start(test, "badkey", auto, BASE, &[])?;
    let follow = Follow {
        no_be: true,
        ..passphrases().answer(Kind::ZfsKey, &["wrong1", "wrong2", "wrong3"])
    };
    let boot = follow_boot(&mut vm, follow)?;
    test.check(
        boot.saw("keyload-failed"),
        "a wrong ZFS key ends in the recovery shell (root mount failure)",
    );
    vm.shell_ready()?;
    let (_, out) = vm.sh_show("zluks status")?;
    test.check(
        out.matches("open").count() >= 15,
        "all LUKS volumes stay open in the recovery shell",
    );
    let (_, out) = vm.sh_show("zpool list -H -o name,health")?;
    test.check(
        POOLS.iter().all(|pool| out.contains(pool)),
        "pools stay imported for recovery",
    );
    let (rc, out) = vm.sh_show(&format!(
        "printf '{ZFSKEY}' | zfs load-key -L prompt tank/ROOT/be && \
         zfs get -H -o value keystatus tank/ROOT/be"
    ))?;
    test.check(
        rc == 0 && out.contains("available"),
        "the key can be loaded from the recovery shell",
    );
    vm.kill();
    Ok(())
}
