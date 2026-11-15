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
use super::{GROUP, HARDENING, disks, gpg_file, tpm_dir, zfs0_uuid};

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
/// The `hardening` file goes first, so that a setup that fails leaves none,
/// and is written, with zfs0's LUKS UUID, when every volume is enrolled.
fn host_prepare(test: &Test) -> Result<()> {
    let settings = test.settings();
    let hardening = settings.disks(GROUP).join(HARDENING);
    if hardening.exists() {
        fs::remove_file(&hardening)
            .map_err(Error::io(format!("removing {}", hardening.display())))?;
    }
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
    let help = cryptenroll_help();
    let mut loops = Vec::new();
    let result: Result<Vec<String>> = (0..4)
        .map(|index| prepare_volume(test, &tcti, &help, index, &mut loops))
        .collect();
    for device in &loops {
        Command::new("losetup").arg("-d").arg(device).output().ok();
    }
    swtpm::stop_pid_file(&pid_file);
    let mut lines = result?;
    lines.push(format!("zfs0 UUID: {}", zfs0_uuid(settings)?));
    fs::write(&hardening, lines.join("\n") + "\n")
        .map_err(Error::io(format!("writing {}", hardening.display())))
}

/// The help of `systemd-cryptenroll`, empty when it does not run: it says
/// whether it can harden a TPM2 PIN with Argon2id (systemd 262 and later).
fn cryptenroll_help() -> String {
    Command::new("systemd-cryptenroll")
        .arg("--help")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

/// Formats and enrols the volume of disk `index` on a loop device, which
/// is added to `loops`, with the options `help` offers; returns the line of
/// the volume in the `hardening` file.
fn prepare_volume(
    test: &Test,
    tcti: &str,
    help: &str,
    index: usize,
    loops: &mut Vec<String>,
) -> Result<String> {
    let disk = crate::disks::disk(test.settings(), GROUP, index, "384M", true)?;
    partition(&disk, &[(&format!("zfs{index}"), None)])?;
    let part = loop_partition(&disk, 1)?;
    loops.push(part.clone());
    Command::new("udevadm").arg("settle").output().ok();
    run_shell(&format!(
        "printf '%s' {PW} | cryptsetup luksFormat -q --type luks2 {FAST_PBKDF} --key-file=- {part}"
    ))?;
    let options = pin_options(help, index);
    enrol(test, tcti, index, &part, &options)?;
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
    let kdf = pin_kdf(&dump);
    let line = format!("zfs{index} PIN: {kdf}");
    test.check(is_hardened_as_enrolled(index, &options, &kdf), &line);
    Ok(line)
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
/// with the PIN `options`, and a clevis TPM2 binding as the second token of
/// zfs2.
fn enrol(test: &Test, tcti: &str, index: usize, part: &str, options: &[String]) -> Result<()> {
    if index < 3 {
        let pin = if index == 2 { "5678" } else { "1234" };
        let enrolled = output(
            Command::new("systemd-cryptenroll")
                .arg(format!("--tpm2-device={tcti}"))
                .arg("--tpm2-pcrs=")
                .args(options)
                .arg(part)
                .env("PASSWORD", PW)
                .env("NEWPIN", pin),
        )?;
        test.check(
            enrolled.status.success(),
            &format!(
                "systemd-cryptenroll TPM2 + PIN on zfs{index} ({})",
                options.join(" ")
            ),
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

/// The option of `systemd-cryptenroll` that systemd 262 added with the
/// Argon2id hardening of TPM2 PINs.
const ARGON2ID_OPTION: &str = "--tpm2-argon2id-memory";

/// The PIN option of zfs1 where systemd can harden PINs with Argon2id: the
/// PBKDF2 of systemd 255 to 261, with a salt.
const PIN_DIRECT: &str = "--tpm2-with-pin=direct";

/// The fixed Argon2id parameters of the test's PINs, which systemd uses as
/// given, without its benchmark: 64 MiB, 2 passes, 4 lanes. A token sized
/// on the host by the benchmark could take gigabytes, more than the VM
/// has.
const SMALL_ARGON2ID: [&str; 3] = [
    "--tpm2-argon2id-memory=64M",
    "--tpm2-argon2id-iterations=2",
    "--tpm2-argon2id-parallelism=4",
];

/// The hardening that [`SMALL_ARGON2ID`] asks for.
const SMALL_KDF: PinKdf = PinKdf::Argon2id {
    memory: 64 << 20,
    passes: 2,
    lanes: 4,
};

/// The PIN options of `systemd-cryptenroll` for the volume of disk `index`,
/// by its `help`. Where it can harden a PIN with Argon2id (systemd 262),
/// zfs0 and zfs2 are, with [`SMALL_ARGON2ID`], and zfs1 keeps the PBKDF2
/// of `direct`; before, `yes` is PBKDF2 (systemd 255 to 261) everywhere.
pub(super) fn pin_options(help: &str, index: usize) -> Vec<String> {
    let argon2id = help.contains(ARGON2ID_OPTION);
    let mut options = vec![if argon2id && index == 1 {
        PIN_DIRECT.to_owned()
    } else {
        "--tpm2-with-pin=yes".to_owned()
    }];
    if argon2id && index != 1 {
        options.extend(SMALL_ARGON2ID.map(str::to_owned));
    }
    options
}

/// How the TPM2 PIN of a volume is hardened, as its token records it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PinKdf {
    /// Argon2id (systemd 262 `yes`).
    Argon2id {
        /// The memory, in bytes.
        memory: u64,
        /// The passes over it.
        passes: u64,
        /// The lanes.
        lanes: u64,
    },
    /// PBKDF2 with the token's salt (systemd 255 to 261, or 262 `direct`).
    Pbkdf2,
    /// No PIN token, or a PIN without salt (systemd before 255).
    None,
}

impl std::fmt::Display for PinKdf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            PinKdf::Argon2id {
                memory,
                passes,
                lanes,
            } if memory % (1 << 20) == 0 => {
                write!(f, "argon2id ({} MiB, {passes}, {lanes})", memory >> 20)
            }
            PinKdf::Argon2id {
                memory,
                passes,
                lanes,
            } => {
                write!(f, "argon2id ({memory} bytes, {passes}, {lanes})")
            }
            PinKdf::Pbkdf2 => f.write_str("pbkdf2"),
            PinKdf::None => f.write_str("none"),
        }
    }
}

/// How the first systemd-tpm2 token with a PIN is hardened, in the output
/// of `cryptsetup luksDump --dump-json-metadata` or of `cryptsetup token
/// export`. What was enrolled is read here, not from the options: systemd
/// 262 without Argon2id in its libcrypto falls back from `yes` to `direct`.
pub(super) fn pin_kdf(dump: &str) -> PinKdf {
    let Ok(metadata) = serde_json::from_str::<serde_json::Value>(dump) else {
        return PinKdf::None;
    };
    let tokens: Vec<&serde_json::Value> = match metadata["tokens"].as_object() {
        Some(tokens) => tokens.values().collect(),
        None => vec![&metadata],
    };
    let Some(token) = tokens
        .into_iter()
        .find(|token| token["type"] == "systemd-tpm2" && token["tpm2-pin"] == true)
    else {
        return PinKdf::None;
    };
    let field = |name: &str| token[format!("tpm2_argon2id_{name}")].as_u64();
    match (field("memcost"), field("iterations"), field("lanes")) {
        (Some(memory), Some(passes), Some(lanes)) => PinKdf::Argon2id {
            memory,
            passes,
            lanes,
        },
        _ if token["tpm2_salt"].is_string() => PinKdf::Pbkdf2,
        _ => PinKdf::None,
    }
}

/// Whether the volume of disk `index`, enrolled with the PIN `options`, is
/// hardened as `kdf`: as [`SMALL_ARGON2ID`] asked, or with PBKDF2 where
/// systemd fell back to it; zfs1 with PBKDF2; zfs3 without a PIN. A PIN
/// without salt passes only before systemd 255, where the options are
/// `yes` alone: systemd 262's `direct` always salts it.
pub(super) fn is_hardened_as_enrolled(index: usize, options: &[String], kdf: &PinKdf) -> bool {
    let argon2id = options
        .iter()
        .any(|option| option.starts_with(ARGON2ID_OPTION));
    let systemd_262 = argon2id || options.iter().any(|option| option == PIN_DIRECT);
    match kdf {
        _ if index == 3 => *kdf == PinKdf::None,
        PinKdf::Argon2id { .. } => argon2id && *kdf == SMALL_KDF,
        PinKdf::Pbkdf2 => true,
        PinKdf::None => !systemd_262,
    }
}
