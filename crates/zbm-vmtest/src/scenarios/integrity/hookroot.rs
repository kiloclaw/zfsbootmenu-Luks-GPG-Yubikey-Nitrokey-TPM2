//! `integrity-hookroot` (ADR 0009): the hooks of `zbm.hookroot` go into
//! PCR 9 before they run, so a token bound to PCR 9 unseals only with the
//! hook root it was enrolled with.
//!
//! On the host, `auto0`'s TPM2 tokens are enrolled again: one bound to
//! PCR 9 as zeros, as `setup` binds it (the other scenarios boot without a
//! hook root), and one bound to PCR 9 after the hook [`HOOK`], the value
//! computed here as ZFSBootMenu extends it. Then two boots of `tauto`'s
//! boot environment with the TPM and a hook root on a FAT disk: with
//! [`HOOK`], the volume unlocks itself and the boot environment's log
//! replays to the value enrolled; with another hook of the same name, the
//! TPM releases nothing and the passphrase is asked.

use std::process::Command;

use sha2::{Digest as _, Sha256};

use super::{
    BASE, GROUP, Record, disk, image, printed_log, replay, serial_text, swtpm_running, tpm_dir,
};
use crate::boot::{Boot, Follow, Kind, check_be, follow_boot};
use crate::disks::{loop_partition, vfat_disk};
use crate::error::{Error, Result};
use crate::qemu::{Disk, Machine};
use crate::record::Test;
use crate::settings::PW;
use crate::swtpm::Swtpm;
use crate::vm::Vm;

/// The hook's place in the hook root, below `zbm`, as ZFSBootMenu names it.
const HOOK_NAME: &str = "early-setup.d/10-zbmtest";

/// The hook the token is enrolled with.
const HOOK: &str = "#!/bin/sh\necho ZBMTEST-HOOKROOT-ENROLLED\n";

/// Another hook of the same name: someone else's.
const OTHER: &str = "#!/bin/sh\necho ZBMTEST-HOOKROOT-OTHER\n";

/// The command line of the boots: the hook root on the disk `hooks`.
const HOOK_ROOT: &str = "zbm.hookroot=/dev/disk/by-id/virtio-hooks//zbm";

/// PCR 9 in the SHA-256 bank after the hook `contents`, from zeros, as
/// ZFSBootMenu extends it: the digest of the hook's name, a NUL and its
/// contents (`zbm_crypt::measure::Event::hook`).
pub(super) fn after_hook(contents: &str) -> [u8; 32] {
    let event = Sha256::new()
        .chain_update(HOOK_NAME)
        .chain_update([0])
        .chain_update(contents)
        .finalize();
    Sha256::new()
        .chain_update([0; 32])
        .chain_update(event)
        .finalize()
        .into()
}

/// The lower-case hexadecimal of `bytes`.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        text.push_str(&format!("{byte:02x}"));
        text
    })
}

/// The scenario.
///
/// # Errors
///
/// When a step cannot be done.
pub fn run(test: &Test) -> Result<()> {
    enrol(test, &[[0; 32], after_hook(HOOK)])?;
    let enrolled = boot(test, "hookroot-enrolled", HOOK, None)?;
    check_enrolled(test, &enrolled.0, &enrolled.1);
    let other = boot(test, "hookroot-other", OTHER, Some(PW))?;
    check_other(test, &other.0, &other.1);
    Ok(())
}

/// Enrols `auto0` again, on the host, with a TPM2 token without a PIN for
/// each value of PCR 9 in `values`; the tokens before are wiped.
fn enrol(test: &Test, values: &[[u8; 32]]) -> Result<()> {
    let settings = test.settings();
    let path = crate::disks::disk(settings, GROUP, 0, "384M", false)?;
    let device = loop_partition(&path, 1)?;
    let result = swtpm_running(settings, |tcti| {
        for (index, value) in values.iter().enumerate() {
            let mut command = Command::new("systemd-cryptenroll");
            command
                .arg(format!("--tpm2-device={tcti}"))
                .arg(format!("--tpm2-pcrs=9:sha256={}", hex(value)))
                .arg("--tpm2-with-pin=no");
            if index == 0 {
                command.arg("--wipe-slot=tpm2");
            }
            let enrolled = command
                .arg(&device)
                .env("PASSWORD", PW)
                .output()
                .map_err(Error::io("running systemd-cryptenroll"))?;
            test.check(
                enrolled.status.success(),
                &format!("auto0 enrolled with PCR 9 = {}", hex(value)),
            );
        }
        Ok(())
    });
    Command::new("losetup").arg("-d").arg(&device).output().ok();
    result
}

/// Boots `tauto` with the TPM and a hook root holding `hook`, typing
/// `passphrase` if it is asked; the boot and the serial log.
fn boot(
    test: &Test,
    scenario: &str,
    hook: &str,
    passphrase: Option<&str>,
) -> Result<(Boot, String)> {
    let settings = test.settings();
    let dir = settings.disks(GROUP);
    let name = format!("{scenario}.img");
    let files: [(&str, &[u8]); 1] = [(&format!("zbm/{HOOK_NAME}"), hook.as_bytes())];
    let hooks = vfat_disk(&dir.join(name), "HOOKS", &files, "64M", 512)?;
    let disks = vec![disk(settings, 0, false)?, Disk::new(hooks, "hooks")];
    let append = format!("{BASE} zbm.prefer=tauto {HOOK_ROOT}");
    let mut machine = Machine::boot(&image(settings)?, &append).with_disks(disks);
    let state = tpm_dir(settings);
    Swtpm::new(&state)?.clear_lockout()?;
    machine.tpm = Some(state);
    let mut vm = Vm::start(test, scenario, &machine)?;
    let follow = match passphrase {
        Some(passphrase) => Follow::default().answer(Kind::Luks, &[passphrase]),
        None => Follow::default(),
    };
    let boot = follow_boot(&mut vm, follow)?;
    let log = serial_text(&vm)?;
    check_be(test, &boot, "tauto", &["tauto"], &[]);
    Ok((boot, log))
}

/// PCR 9 when ZFSBootMenu unsealed: after the records before the boot's
/// own (the starts and the hooks).
fn unsealed_at(records: &[Record]) -> Vec<u8> {
    let before: Vec<Record> = records
        .iter()
        .take_while(|record| matches!(record.event.as_str(), "start" | "hook"))
        .cloned()
        .collect();
    replay(&before, 9)
}

/// The checks of the boot with the hook enrolled.
pub(super) fn check_enrolled(test: &Test, boot: &Boot, log: &str) {
    test.check(
        log.contains("ZBMTEST-HOOKROOT-ENROLLED"),
        "the hook root's hook ran",
    );
    test.check(
        !boot.has_prompt("luks"),
        &format!(
            "the token bound to the hook root unsealed, nothing asked ({})",
            boot.prompts_text()
        ),
    );
    let records = printed_log(&boot.be);
    test.check(
        records
            .iter()
            .any(|record| record.event == "hook" && record.value == HOOK_NAME),
        "the event log has the hook",
    );
    test.check(
        unsealed_at(&records) == after_hook(HOOK),
        "PCR 9 before the boot, replayed from the log, is the value enrolled (zbm measure unseal)",
    );
}

/// The checks of the boot with another hook of the same name.
pub(super) fn check_other(test: &Test, boot: &Boot, log: &str) {
    test.check(log.contains("ZBMTEST-HOOKROOT-OTHER"), "the other hook ran");
    test.check(
        boot.has_prompt("luks"),
        &format!(
            "with another hook the TPM released nothing: the passphrase was asked ({})",
            boot.prompts_text()
        ),
    );
}
