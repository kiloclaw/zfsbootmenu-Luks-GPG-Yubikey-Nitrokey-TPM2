//! The scenarios that end in a shell: `skip-recover` and `oldzfs`.

use crate::boot::{Follow, ask_menu_for_shell, dialogs, follow_boot, prompt};
use crate::error::Result;
use crate::expect::regex;
use crate::images::{Image, hostid_arg};
use crate::pyfmt;
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::{PW, PW_OTHER};
use crate::vm::Vm;

use super::healthy::{Expect, expect_healthy};
use super::{BASE, LUKS_VOLUMES, disks};

/// Every LUKS prompt skipped; the volumes are unlocked with zluks in the
/// recovery shell and the boot continues.
pub(super) fn skip_recover(test: &Test, auto: &Image) -> Result<()> {
    let machine = Machine::boot(auto, BASE).with_disks(disks(test.settings(), &[])?);
    let mut vm = Vm::start(test, "skip-recover", &machine)?;
    let (skipped, index) = skip_prompts(&mut vm)?;
    // mixed and parts import DEGRADED through their unencrypted halves, so
    // ZFSBootMenu finds no boot environment and opens its emergency shell
    // (with no importable pool it would stay in the import retry loop).
    test.check(
        skipped >= 2 && index == 2,
        &format!(
            "{skipped} LUKS prompts skipped; no boot environment without tank, emergency shell"
        ),
    );
    vm.shell_ready()?;
    let (_, out) = vm.sh_show("zpool list -H -o name,health")?;
    test.check(
        !out.contains("tank") && out.contains("DEGRADED"),
        "only the pools with unencrypted halves are imported",
    );
    let (rc, _) = vm.sh_show("zluks status")?;
    test.check(rc == 0, "zluks status runs");
    let (prompts, rc) = unlock(&mut vm)?;
    test.check(
        rc == "0",
        &format!("zluks unlock opened the volumes ({prompts} prompts)"),
    );
    vm.sh_show("zluks status | grep -c open")?;
    test.step("leaving the shell; ZFSBootMenu imports tank and shows its menu");
    vm.sendline("exit")?;
    let follow = Follow {
        menu: true,
        ..Follow::default()
    };
    let boot = follow_boot(&mut vm, follow)?;
    test.check(
        boot.has_prompt("menu"),
        "the boot menu lists tank's boot environment after the shell",
    );
    // mixed and parts were imported, read-only, through their unencrypted
    // halves before zluks opened the LUKS halves; ZFSBootMenu leaves such
    // pools imported at kexec.
    let states = [("mixed", "DEGRADED"), ("parts", "DEGRADED")];
    let expect = Expect {
        states: &states,
        be_states: Some(&[]),
        readonly: &["mixed", "parts"],
    };
    expect_healthy(test, &boot, expect)
}

/// Skips every LUKS prompt, on the console or in the start-up view, until
/// ZFSBootMenu gives up; returns the number of prompts skipped and what
/// ended them: 1 the import retry loop, 2 the emergency shell. Where the
/// legacy menu finds no boot environment and opens the shell, the menu of
/// the Rust runtime says so and waits: the test opens the shell from it,
/// which counts as the emergency shell.
pub(super) fn skip_prompts(vm: &mut Vm<'_>) -> Result<(usize, usize)> {
    let patterns = [
        prompt::LUKS,
        prompt::RETRY,
        prompt::EMERGENCY,
        prompt::NO_ENVIRONMENTS,
        dialogs::LUKS,
        dialogs::RETRY,
    ];
    let mut skipped = 0;
    loop {
        match vm.expect(&patterns, 1800)? {
            0 => vm.sendline("")?,
            3 => {
                vm.test()
                    .step("the menu found no boot environment; opening the shell from it");
                ask_menu_for_shell(vm)?;
                return Ok((skipped, 2));
            }
            4 => dialogs::reply(vm, "")?,
            5 => return Ok((skipped, 1)),
            index => return Ok((skipped, index)),
        }
        skipped += 1;
    }
}

/// Runs `zluks unlock` in the shell, answering its prompts with the
/// passphrase, then the other one; returns the number of prompts and the
/// exit status.
pub(super) fn unlock(vm: &mut Vm<'_>) -> Result<(usize, String)> {
    vm.sendline("zluks unlock; echo RC=$?")?;
    let mut prompts = 0;
    while vm.expect(&[prompt::LUKS, r"RC=(\d+)\r*\n"], 1800)? == 0 {
        prompts += 1;
        vm.sendline(if prompts == 1 { PW } else { PW_OTHER })?;
    }
    Ok((prompts, vm.group(1)))
}

/// An image with an older ZFS refuses the ZFS 2.4.4 pools.
pub(super) fn oldzfs(test: &Test) -> Result<()> {
    let settings = test.settings();
    let Some((kernel, initrd)) = settings.old_image_files()? else {
        test.skip("ZBMTEST_OLD_KERNEL and ZBMTEST_OLD_INITRD are not set");
        return Ok(());
    };
    let append = format!("{BASE} {}", hostid_arg(settings)?);
    let machine = Machine::kernel(kernel, initrd, &append).with_disks(disks(settings, &[])?);
    let mut vm = Vm::start(test, "oldzfs", &machine)?;
    // mixed and parts have no active feature the older ZFS lacks, so it
    // imports them; tank, with raidz_expansion active, is refused, and so
    // is its boot environment.
    if answer_prompts(&mut vm)? == 1 {
        test.step("no pool imported; opening the recovery shell");
        vm.send("\x1b")?;
    } else {
        test.step("no boot environment found; ZFSBootMenu dropped to its emergency shell");
    }
    vm.shell_ready()?;
    let (_, out) = vm.sh_show("cat /sys/block/dm-*/dm/uuid 2>/dev/null | grep -c '^CRYPT-'")?;
    test.check(
        out.trim() == LUKS_VOLUMES.to_string(),
        &format!(
            "the older image unlocked the {LUKS_VOLUMES} volumes and booted nothing ({})",
            out.trim()
        ),
    );
    let (_, out) = vm.sh_show("zfs version; zpool list -H -o name,health,readonly")?;
    test.check(
        !regex(r"(?m)^tank\s")?.is_match(&out),
        "the older ZFS did not import tank",
    );
    let imported = imported(&out)?;
    test.note(&format!(
        "pools without unsupported active features imported by the older ZFS: {}",
        pyfmt::list(&imported)
    ));
    let (_, out) = vm.sh_show("zpool import 2>&1 | head -60")?;
    test.check(
        out.contains("tank")
            && (out.contains("raidz_expansion")
                || out.to_lowercase().contains("unsupported feature")),
        "zpool import names the unsupported feature of tank",
    );
    test.check(
        out.contains("cannot be imported"),
        "zpool import says tank cannot be imported",
    );
    vm.kill();
    Ok(())
}

/// Answers the LUKS prompts, on the console or in the start-up view (the
/// passphrase, the other one, then the passphrase) until ZFSBootMenu gives
/// up; returns what ended them: 1 the import retry loop, 2 the emergency
/// shell.
pub(super) fn answer_prompts(vm: &mut Vm<'_>) -> Result<usize> {
    let patterns = [
        prompt::LUKS,
        prompt::RETRY,
        prompt::EMERGENCY,
        dialogs::LUKS,
        dialogs::RETRY,
    ];
    let mut answers = [PW, PW_OTHER].into_iter();
    loop {
        match vm.expect(&patterns, 1800)? {
            0 => vm.sendline(answers.next().unwrap_or(PW))?,
            3 => dialogs::reply(vm, answers.next().unwrap_or(PW))?,
            4 => return Ok(1),
            index => return Ok(index),
        }
    }
}

/// The pools mixed and parts that `zpool list` shows ONLINE, sorted.
pub(super) fn imported(list: &str) -> Result<Vec<String>> {
    let pattern = regex(r"(?m)^(mixed|parts)\s+ONLINE")?;
    let mut pools: Vec<String> = pattern
        .captures_iter(list)
        .filter_map(|captures| captures.get(1).map(|pool| pool.as_str().to_owned()))
        .collect();
    pools.sort();
    Ok(pools)
}
