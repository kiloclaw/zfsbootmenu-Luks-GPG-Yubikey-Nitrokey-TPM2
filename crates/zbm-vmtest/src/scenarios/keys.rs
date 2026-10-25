//! Key management with a software TPM and GnuPG, on ZFS 2.4.4 pools.
//!
//! Disks: four, one GPT partition each (zfs0-zfs3). zfs0 and zfs1 are
//! enrolled with `systemd-cryptenroll` for TPM2 with the PIN 1234 (no
//! PCRs). zfs2 has two tokens: TPM2 with another PIN (5678) as token 0 and
//! a clevis TPM2 binding as token 1. zfs3 has a key file encrypted with
//! `gpg --symmetric` (passphrase gpgpass). Every volume also keeps the
//! passphrase pw. The crypttab names the mappings zroot-a, zroot-b (mirror
//! zroot, root of the boot environment), tank-a and tank-b (mirror tank).
//!
//! Scenarios:
//!
//! - `setup`: LUKS formatting and enrolment on the host (loop devices,
//!   swtpm), then pool creation with ZFS 2.4.4 in the recovery shell;
//! - `plugin`: systemd's TPM2 token plugin: a wrong PIN, the right PIN
//!   (reused for zfs1); zfs2 is unlocked silently by its second token
//!   (clevis needs no interaction and is tried before PIN tokens); the
//!   GnuPG passphrase for zfs3; boot to the boot environment. When only the
//!   first token were seen, zfs2 would ask for its PIN and passphrase;
//! - `native`: the same with `zfsbootmenu_luks_token_plugins="no"`
//!   (tpm2-tools);
//! - `lockout`: the TPM allows one wrong PIN: after it, ZFSBootMenu
//!   explains the lockout and falls back to the passphrase;
//! - `notpm`: no TPM device: the TPM2 volumes fall back to the passphrase.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::boot::{Boot, Follow, Kind, check_be, check_teardown, follow_boot};
use crate::disks::run as run_host;
use crate::error::{Error, Result};
use crate::images::{Image, ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::{PW, Settings};
use crate::swtpm::{self, Swtpm};
use crate::vm::Vm;

mod setup;
#[cfg(test)]
mod tests;

/// The disk group.
const GROUP: &str = "keys";

/// The crypttab of the images.
const CRYPTTAB: &str = "\
zroot-a  PARTLABEL=zfs0  none                                discard
zroot-b  PARTLABEL=zfs1  none                                discard
tank-a   PARTLABEL=zfs2  none                                -
tank-b   PARTLABEL=zfs3  /etc/zfsbootmenu/keys/tank.key.gpg  -
";

/// The build step that puts the GnuPG key file into the images.
const RC: &str = "mkdir -p /etc/zfsbootmenu/keys && \
                  cp /build/keys/tank.key.gpg /etc/zfsbootmenu/keys/";

/// The kernel command line of the boots after setup.
const BASE: &str = "console=ttyS0 zbm.timeout=3";

/// The state directory of the software TPM.
fn tpm_dir(settings: &Settings) -> PathBuf {
    settings.disks(GROUP).join("tpm")
}

/// The GnuPG key file of zfs3.
fn gpg_file(settings: &Settings) -> PathBuf {
    settings.disks(GROUP).join("tank.key.gpg")
}

/// The four disks.
fn disks(settings: &Settings, fresh: bool) -> Result<Vec<crate::qemu::Disk>> {
    super::disks(settings, GROUP, 0..4, fresh)
}

/// Runs a scenario of the group.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub fn run(test: &Test, scenario: &str) -> Result<()> {
    if scenario == "setup" {
        return setup::setup(test);
    }
    let settings = test.settings();
    if !gpg_file(settings).exists() {
        test.check(false, "run keys-setup first");
        return Ok(());
    }
    let tpm = Swtpm::new(&tpm_dir(settings))?;
    tpm.clear_lockout()?;
    match scenario {
        "plugin" | "native" => tokens(test, scenario),
        "lockout" => lockout(test, &tpm),
        "notpm" => notpm(test),
        _ => {
            test.check(false, &format!("unknown scenario {scenario}"));
            Ok(())
        }
    }
}

/// The image of the scenarios: with the token plugins, or with tpm2-tools
/// (`native`).
fn image(settings: &Settings, native: bool) -> Result<Image> {
    let path = gpg_file(settings);
    let key = fs::read(&path).map_err(Error::io(format!("reading {}", path.display())))?;
    let (variant, conf) = if native {
        ("keys-native", r#"zfsbootmenu_luks_token_plugins="no""#)
    } else {
        ("keys-plugin", "")
    };
    let spec = ImageSpec {
        crypttab: Some(CRYPTTAB.to_owned()),
        files: vec![("keys/tank.key.gpg".to_owned(), key)],
        rc: Some(RC.to_owned()),
        conf: conf.to_owned(),
        ..ImageSpec::new(variant)
    };
    zbm_image(settings, &spec)
}

/// Starts the machine of `scenario`, with the software TPM when `tpm`.
fn start<'t>(test: &'t Test, scenario: &str, image: &Image, tpm: bool) -> Result<Vm<'t>> {
    let settings = test.settings();
    let mut machine = Machine::boot(image, BASE).with_disks(disks(settings, false)?);
    if tpm {
        machine.tpm = Some(tpm_dir(settings));
    }
    Vm::start(test, scenario, &machine)
}

/// The checks of every boot: both pools imported, the teardown, the boot
/// environment.
fn boot_checks(test: &Test, boot: &Boot) {
    let pools = boot.pools();
    test.check(
        pools.get("zroot") == Some("ONLINE") && pools.get("tank") == Some("ONLINE"),
        &format!("ZFSBootMenu imported zroot and tank: {}", pools.text()),
    );
    check_teardown(test, boot, &[]);
    check_be(test, boot, "zroot", &["zroot", "tank"], &[]);
}

/// `plugin` and `native`: a wrong PIN, the right PIN, zfs2 unlocked by its
/// second token, the GnuPG passphrase.
fn tokens(test: &Test, scenario: &str) -> Result<()> {
    let image = image(test.settings(), scenario == "native")?;
    let mut vm = start(test, scenario, &image, true)?;
    let follow = Follow::default()
        .answer(Kind::Tpm2Pin, &["0000", "1234", ""])
        .answer(Kind::Gpg, &["gpgpass"]);
    let boot = follow_boot(&mut vm, follow)?;
    test.check(
        boot.prompts == ["tpm2pin", "tpm2pin", "gpg"],
        &format!(
            "wrong PIN, right PIN (reused for zfs1), no prompt for zfs2, GnuPG passphrase: {}",
            boot.prompts_text()
        ),
    );
    test.check(
        !boot.has_prompt("luks"),
        "no fallback to the LUKS passphrase: zfs2 was unlocked by its second token (clevis)",
    );
    boot_checks(test, &boot);
    Ok(())
}

/// Sets the TPM's dictionary-attack parameters: `tries` wrong PINs before
/// the lockout, `seconds` for the recovery and the lockout.
fn dictionary_lockout(settings: &Settings, tries: &str, seconds: &str) -> Result<()> {
    let pid_file = settings.disks(GROUP).join("swtpm-lock.pid");
    let port = swtpm::start_tcp(&tpm_dir(settings), &pid_file, "startup-clear")?;
    let result = run_host(Command::new("tpm2_dictionarylockout").args([
        "-T",
        &swtpm::tcti(port),
        "-s",
        "-n",
        tries,
        "-t",
        seconds,
        "-l",
        seconds,
    ]));
    swtpm::stop_pid_file(&pid_file);
    result.map(drop)
}

/// The TPM allows one wrong PIN: after it, ZFSBootMenu explains the
/// lockout and falls back to the passphrase.
fn lockout(test: &Test, tpm: &Swtpm) -> Result<()> {
    let settings = test.settings();
    dictionary_lockout(settings, "1", "3600")?;
    let image = image(settings, false)?;
    let mut vm = start(test, "lockout", &image, true)?;
    let follow = Follow::default()
        .answer(Kind::Tpm2Pin, &["0000", "9999"])
        .answer(Kind::Luks, &[PW, PW, PW])
        .answer(Kind::Gpg, &["gpgpass"]);
    let boot = follow_boot(&mut vm, follow)?;
    let log = read_log(&vm.serial_log())?;
    // The start-up view says it in the dialog that asks next.
    let lockout = "locked against PIN guessing";
    test.check(
        log.contains(lockout) || boot.has_shown(lockout),
        "ZFSBootMenu explains the TPM lockout",
    );
    test.check(
        boot.has_prompt("luks"),
        &format!(
            "falls back to the passphrase after the lockout ({})",
            boot.prompts_text()
        ),
    );
    boot_checks(test, &boot);
    tpm.clear_lockout()?;
    dictionary_lockout(settings, "32", "600")
}

/// The text of a serial log.
fn read_log(path: &Path) -> Result<String> {
    let bytes = fs::read(path).map_err(Error::io(format!("reading {}", path.display())))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// No TPM device: the TPM2 volumes fall back to the passphrase.
fn notpm(test: &Test) -> Result<()> {
    let image = image(test.settings(), false)?;
    let mut vm = start(test, "notpm", &image, false)?;
    let follow = Follow::default()
        .answer(Kind::Luks, &[PW, PW, PW])
        .answer(Kind::Gpg, &["gpgpass"]);
    let boot = follow_boot(&mut vm, follow)?;
    test.check(
        !boot.has_prompt("tpm2pin"),
        "no TPM2 PIN prompt without a TPM",
    );
    test.check(
        boot.has_prompt("luks"),
        &format!(
            "TPM2 volumes fall back to the passphrase ({})",
            boot.prompts_text()
        ),
    );
    boot_checks(test, &boot);
    Ok(())
}
