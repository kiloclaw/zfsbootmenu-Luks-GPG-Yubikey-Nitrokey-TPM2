//! `keys-setup`: LUKS formatting and enrolment on the host (loop devices,
//! swtpm), then pool creation with ZFS 2.4.4 in the recovery shell.

use std::fs::{self, DirBuilder, File};
use std::io::Read as _;
use std::os::unix::fs::DirBuilderExt as _;
use std::process::{Command, Output};

use crate::disks::{loop_partition, partition, run, run_shell};
use crate::error::{Error, Result};
use crate::expect::regex;
use crate::images::{ImageSpec, zbm_image};
use crate::pyfmt;
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::{FAST_PBKDF, PW};
use crate::swtpm;
use crate::vm::Vm;

use super::super::payload;
use super::{GROUP, disks, gpg_file, tpm_dir};

/// Prepares the volumes on the host, then creates the pools and the boot
/// environment in the recovery shell.
pub(super) fn setup(test: &Test) -> Result<()> {
    host_prepare(test)?;
    let settings = test.settings();
    let image = zbm_image(settings, &ImageSpec::new("auto"))?;
    let mut machine_disks = disks(settings, false)?;
    machine_disks.push(payload(settings)?);
    let machine = Machine::boot(&image, "console=ttyS0 zbm.luks=off zbm.retry_delay=3")
        .with_disks(machine_disks);
    let mut vm = Vm::start(test, "setup", &machine)?;
    vm.recovery_shell()?;
    vm.send_functions()?;
    for (index, name) in ["zroot-a", "zroot-b", "tank-a", "tank-b"]
        .iter()
        .enumerate()
    {
        vm.must(&format!(
            "printf '%s' {PW} | cryptsetup open --key-file=- /dev/disk/by-partlabel/zfs{index} \
             {name}"
        ))?;
    }
    let opts = "-o ashift=12 -o compatibility=openzfs-2.4-linux -O compression=lz4 -m none";
    vm.must(&format!(
        "zpool create -f {opts} zroot mirror /dev/mapper/zroot-a /dev/mapper/zroot-b"
    ))?;
    vm.must(&format!(
        "zpool create -f {opts} tank mirror /dev/mapper/tank-a /dev/mapper/tank-b"
    ))?;
    vm.must("mkbe zroot")?;
    vm.must(
        "becmdline zroot/ROOT/be $(for i in 0 1 2 3; do \
         cryptsetup luksUUID /dev/disk/by-partlabel/zfs$i; done)",
    )?;
    vm.must(
        "zpool export -a && for m in zroot-a zroot-b tank-a tank-b; do cryptsetup close $m; done; \
         grep -q ' /pl ' /proc/mounts && umount /pl; true",
    )?;
    vm.poweroff()
}

/// Formats and enrols the four volumes on loop devices, with a software
/// TPM on TCP; the loop devices and the TPM are released on every path.
fn host_prepare(test: &Test) -> Result<()> {
    let settings = test.settings();
    let tpm = tpm_dir(settings);
    if tpm.exists() {
        run(Command::new("rm").arg("-rf").arg(&tpm))?;
    }
    fs::create_dir_all(&tpm).map_err(Error::io(format!("creating {}", tpm.display())))?;
    run(Command::new("swtpm_setup")
        .args(["--tpm2", "--tpmstate"])
        .arg(&tpm)
        .arg("--overwrite"))?;
    let pid_file = settings.disks(GROUP).join("swtpm.pid");
    let port = swtpm::start_tcp(&tpm, &pid_file, "not-need-init,startup-clear")?;
    let tcti = swtpm::tcti(port);
    let mut loops = Vec::new();
    let result = (0..4).try_for_each(|index| prepare_volume(test, &tcti, index, &mut loops));
    for device in &loops {
        Command::new("losetup").arg("-d").arg(device).output().ok();
    }
    swtpm::stop_pid_file(&pid_file);
    result
}

/// Formats and enrols the volume of disk `index` on a loop device, which
/// is added to `loops`.
fn prepare_volume(test: &Test, tcti: &str, index: usize, loops: &mut Vec<String>) -> Result<()> {
    let disk = crate::disks::disk(test.settings(), GROUP, index, "384M", true)?;
    partition(&disk, &[(&format!("zfs{index}"), None)])?;
    let part = loop_partition(&disk, 1)?;
    loops.push(part.clone());
    Command::new("udevadm").arg("settle").output().ok();
    run_shell(&format!(
        "printf '%s' {PW} | cryptsetup luksFormat -q --type luks2 {FAST_PBKDF} --key-file=- {part}"
    ))?;
    enrol(test, tcti, index, &part)?;
    if index == 3 {
        gpg_key(test, &part)?;
    }
    let dump = run(Command::new("cryptsetup")
        .args(["luksDump", "--dump-json-metadata"])
        .arg(&part))?;
    let types = token_types(&dump)?;
    let want: &[&str] = match index {
        0 | 1 => &["systemd-tpm2"],
        2 => &["systemd-tpm2", "clevis"],
        _ => &[],
    };
    test.check(
        types == want,
        &format!("zfs{index} tokens: {}", pyfmt::list(&types)),
    );
    if index == 2 {
        check_clevis(test, tcti, &part, &dump)?;
    }
    Ok(())
}

/// Checks that the clevis binding of `part`, whose LUKS metadata is
/// `dump`, gives its key back from the software TPM `tcti`: a binding
/// sealed to another TPM fails here, where it is made.
fn check_clevis(test: &Test, tcti: &str, part: &str, dump: &str) -> Result<()> {
    let Some(id) = clevis_token(dump) else {
        test.check(false, "zfs2 has a clevis token");
        return Ok(());
    };
    let decrypted = output(&mut clevis_decrypt(part, id, tcti))?;
    let stderr = String::from_utf8_lossy(&decrypted.stderr);
    let stderr: String = stderr.trim().chars().take(80).collect();
    test.check(
        decrypted.status.success() && !decrypted.stdout.is_empty(),
        &format!("clevis binding of zfs2 opens with the software TPM {stderr}"),
    );
    Ok(())
}

/// Enrols the volume `part` of disk `index`: TPM2 with a PIN on zfs0-zfs2,
/// and a clevis TPM2 binding as the second token of zfs2.
fn enrol(test: &Test, tcti: &str, index: usize, part: &str) -> Result<()> {
    if index < 3 {
        let pin = if index == 2 { "5678" } else { "1234" };
        let enrolled = output(
            Command::new("systemd-cryptenroll")
                .arg(format!("--tpm2-device={tcti}"))
                .args(["--tpm2-pcrs=", "--tpm2-with-pin=yes"])
                .arg(part)
                .env("PASSWORD", PW)
                .env("NEWPIN", pin),
        )?;
        test.check(
            enrolled.status.success(),
            &format!("systemd-cryptenroll TPM2 + PIN on zfs{index}"),
        );
    }
    if index == 2 {
        let bound = output(
            Command::new("sh")
                .arg("-c")
                .arg(format!(
                    "printf '%s' {PW} | clevis luks bind -y -k - -d {part} tpm2 '{{}}'"
                ))
                .envs(clevis_check_env(tcti)),
        )?;
        let stderr = String::from_utf8_lossy(&bound.stderr);
        let stderr: String = stderr.trim().chars().take(80).collect();
        test.check(
            bound.status.success(),
            &format!("clevis luks bind tpm2 on zfs2 (second token) {stderr}"),
        );
    }
    Ok(())
}

/// Adds a random key file to the volume `part` and encrypts it with
/// `gpg --symmetric`.
fn gpg_key(test: &Test, part: &str) -> Result<()> {
    let settings = test.settings();
    let dir = settings.disks(GROUP);
    let key = dir.join("tank.key");
    let mut bytes = [0_u8; 64];
    File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut bytes))
        .map_err(Error::io("reading /dev/urandom"))?;
    fs::write(&key, bytes).map_err(Error::io(format!("writing {}", key.display())))?;
    run_shell(&format!(
        "printf '%s' {PW} | cryptsetup luksAddKey -q {FAST_PBKDF} --key-file=- {part} {}",
        key.display()
    ))?;
    let gnupg = dir.join("gnupg");
    run(Command::new("rm").arg("-rf").arg(&gnupg))?;
    DirBuilder::new()
        .mode(0o700)
        .create(&gnupg)
        .map_err(Error::io(format!("creating {}", gnupg.display())))?;
    let encrypted = gpg_file(settings);
    if encrypted.exists() {
        fs::remove_file(&encrypted)
            .map_err(Error::io(format!("removing {}", encrypted.display())))?;
    }
    run(Command::new("gpg")
        .arg("--homedir")
        .arg(&gnupg)
        .args(["-q", "--batch", "--pinentry-mode", "loopback"])
        .args(["--passphrase", "gpgpass", "--symmetric", "-o"])
        .arg(&encrypted)
        .arg(&key))?;
    fs::remove_file(&key).map_err(Error::io(format!("removing {}", key.display())))?;
    test.check(
        encrypted.exists(),
        "key file of zfs3 encrypted with gpg --symmetric",
    );
    Ok(())
}

/// Runs a program whose failure the test records; returns how it ended.
fn output(command: &mut Command) -> Result<Output> {
    command.output().map_err(|error| Error::Command {
        command: format!("{command:?}"),
        status: "not run".into(),
        output: error.to_string(),
    })
}

/// The types of the systemd-tpm2 and clevis tokens in the output of
/// `cryptsetup luksDump --dump-json-metadata`, in order.
pub(super) fn token_types(dump: &str) -> Result<Vec<String>> {
    let pattern = regex(r#""type":"(systemd-tpm2|clevis)""#)?;
    let dump = dump.replace(' ', "");
    Ok(pattern
        .captures_iter(&dump)
        .filter_map(|captures| captures.get(1).map(|kind| kind.as_str().to_owned()))
        .collect())
}

/// The variables that name the TPM `tcti` to both clevis tpm2 pins: the
/// shell pin's tpm2-tools read `TPM2TOOLS_TCTI`, and clevis-pin-tpm2,
/// which the clevis scripts run in its place where it is installed
/// (Fedora), reads `TCTI` and otherwise takes the host's own TPM.
pub(super) fn clevis_check_env(tcti: &str) -> [(&'static str, String); 2] {
    [
        ("TPM2TOOLS_TCTI", tcti.to_owned()),
        ("TCTI", tcti.to_owned()),
    ]
}

/// `clevis decrypt` of the clevis token `id` of `part`, with the TPM
/// `tcti` and no other: it prints the binding's key. The token's JWE goes
/// to it in its compact form, as ZFSBootMenu decrypts it; `clevis luks
/// pass` finds the token in the `luksDump` text, which, with systemd's
/// token plugins and cryptsetup 2.8, lacks every token after the first.
pub(super) fn clevis_decrypt(part: &str, id: u32, tcti: &str) -> Command {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(format!(
            "cryptsetup token export --token-id {id} {part} | jose fmt -j- -Og jwe -o- \
             | jose jwe fmt -i- -c | clevis decrypt"
        ))
        .envs(clevis_check_env(tcti));
    command
}

/// The ID of the clevis token in the output of `cryptsetup luksDump
/// --dump-json-metadata`, if it has one.
pub(super) fn clevis_token(dump: &str) -> Option<u32> {
    let metadata: serde_json::Value = serde_json::from_str(dump).ok()?;
    let (id, _) = metadata["tokens"]
        .as_object()?
        .iter()
        .find(|(_, token)| token["type"] == "clevis")?;
    id.parse().ok()
}
