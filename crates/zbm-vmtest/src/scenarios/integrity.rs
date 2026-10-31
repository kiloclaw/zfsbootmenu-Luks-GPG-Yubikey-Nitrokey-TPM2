//! Boot integrity in the Rust runtime (ROADMAP.md, SE10).
//!
//! Disks: d0 holds a LUKS volume (`auto0`) enrolled with
//! `systemd-cryptenroll` for TPM2 without a PIN, bound to PCR 9 as it is
//! before ZFSBootMenu measures the boot (zeros: the firmware, SeaBIOS,
//! leaves PCRs 8 and 9 alone), which the machine unlocks by itself; its
//! pool `tauto` holds a boot environment.
//! d1 holds `tplain`, a pool of this host without encryption, and d2
//! `tforeign`, left imported by the host `deadbeef`, each with a boot
//! environment.
//!
//! Scenarios:
//!
//! - `setup`: the LUKS volume and its enrolment on the host (a loop device,
//!   swtpm), then the pools in the recovery shell;
//! - `unattended`: with the TPM, the volume unlocks itself, so start-up
//!   holds to it: `tforeign` is not imported (`hostid` imports as
//!   `strict`), `tauto`'s boot environment boots by itself, and `tplain`'s,
//!   made the default with `zbm.prefer`, waits in the menu; without the
//!   TPM, the passphrase is typed and `tplain`'s boots by itself again;
//! - `lockdown`: with `lockdown=integrity`, the boot environment's kernel,
//!   which is not signed, is not loaded: `kexec_file_load` refuses it, and
//!   `kexec` does not load it instead;
//! - `measured`: with the TPM, `tauto`'s boot environment boots measured
//!   (ADR 0008): its PCRs 8 and 9 are what the event log its initramfs
//!   kept replays to, the log measured the kernel and initramfs as they are
//!   on the dataset, which `zbm measure predict` reads, and the command
//!   line the kernel got; PCR 9 no longer holds the value `auto0`'s token is
//!   bound to, so the booted system cannot unseal it again;
//! - `hookroot` (ADR 0009): a token bound to PCR 9 after a
//!   hook root's hook unseals with that hook, and not with another.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use sha2::{Digest as _, Sha256};

use crate::boot::{Boot, Follow, Kind, check_be, follow_boot, prompt};
use crate::disks::{loop_partition, partition, run, run_shell};
use crate::error::{Error, Result};
use crate::images::{Image, ImageSpec, be_kver, zbm_image};
use crate::qemu::{Disk, Machine};
use crate::record::Test;
use crate::settings::{FAST_PBKDF, PW, Settings, ZbmRuntime};
use crate::swtpm::{self, Swtpm};
use crate::text::clean;
use crate::vm::Vm;

mod hookroot;
mod logs;

use logs::{LOGS_LABEL, after_refusal, kernel_log_in_shell, logs_disk, saved_log};
#[cfg(test)]
mod tests;

/// The disk group.
const GROUP: &str = "integrity";

/// The options of the pools.
const OPTS: &str = "-o ashift=12 -o compatibility=openzfs-2.4-linux -O compression=lz4 \
                    -O acltype=posixacl -O xattr=sa -m none";

/// The command line of the boot environments without LUKS.
const BE_CMDLINE: &str = "console=ttyS0 loglevel=4";

/// The host ID of the other host, `deadbeef`, as `/etc/hostid` holds it.
const FOREIGN: [u8; 4] = [0xef, 0xbe, 0xad, 0xde];

/// The kernel command line of the boots after setup. ZFSBootMenu keeps its
/// log off the serial console: the checks read it from the logs a boot
/// saves ([`logs::LOGS_LABEL`]), or from `dmesg` in the recovery shell.
const BASE: &str = "console=ttyS0 zbm.timeout=3 loglevel=7";

/// What start-up logs when the volumes unlocked themselves.
const HELD: &str = "pools import with this machine's host ID alone";

/// What start-up logs for a default boot environment on no such volume.
const WAITS: &str = "is on no volume that unlocked itself";

/// What the boot logs when lockdown kept the kernel out.
const REFUSED: &str = "lockdown is on: the kernel refused";

/// What the boot shows when the kernel was not loaded.
const NOT_LOADED: &str = "Unable to load kernel or initramfs into memory";

/// A line of the boot environment's report: it booted.
const BE_BOOTED: &str = "BE-KERNEL";

/// The state directory of the software TPM.
fn tpm_dir(settings: &Settings) -> PathBuf {
    settings.disks(GROUP).join("tpm")
}

/// The disk `d<index>`, made again when `fresh`.
fn disk(settings: &Settings, index: usize, fresh: bool) -> Result<Disk> {
    let path = crate::disks::disk(settings, GROUP, index, "384M", fresh)?;
    Ok(Disk::new(path, format!("d{index}")))
}

/// Runs a scenario of the group.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub fn run_scenario(test: &Test, scenario: &str) -> Result<()> {
    if scenario == "setup" {
        return setup(test);
    }
    if !tpm_dir(test.settings()).exists() {
        test.check(false, "run integrity-setup first");
        return Ok(());
    }
    match scenario {
        "unattended" => unattended(test),
        "lockdown" => lockdown(test),
        "measured" => measured(test),
        "hookroot" => hookroot::run(test),
        _ => {
            test.check(false, &format!("unknown scenario {scenario}"));
            Ok(())
        }
    }
}

/// The Rust image of the scenarios.
fn image(settings: &Settings) -> Result<Image> {
    let mut spec = ImageSpec::new("integrity");
    spec.runtime = Some(ZbmRuntime::Rust);
    zbm_image(settings, &spec)
}

/// The volume on the host, then the pools in the recovery shell.
fn setup(test: &Test) -> Result<()> {
    host_prepare(test)?;
    let settings = test.settings();
    let image = zbm_image(settings, &ImageSpec::new("auto"))?;
    let disks = vec![
        disk(settings, 0, false)?,
        disk(settings, 1, true)?,
        disk(settings, 2, true)?,
        super::payload(settings)?,
    ];
    let machine =
        Machine::boot(&image, "console=ttyS0 zbm.luks=off zbm.retry_delay=3").with_disks(disks);
    let mut vm = Vm::start(test, "setup", &machine)?;
    vm.recovery_shell()?;
    vm.send_functions()?;
    vm.must(&format!(
        "printf '%s' {PW} | cryptsetup open --key-file=- /dev/disk/by-partlabel/auto0 auto0"
    ))?;
    vm.must(&format!("zpool create -f {OPTS} tauto /dev/mapper/auto0"))?;
    vm.must("mkbe tauto")?;
    // Its initramfs opens the volume with the key file it holds, the
    // passphrase.
    vm.must("becmdline tauto/ROOT/be $(cryptsetup luksUUID /dev/disk/by-partlabel/auto0)")?;
    for (index, pool) in [(1, "tplain"), (2, "tforeign")] {
        let (rc, _) = vm.sh(&format!(
            "zpool create -f {OPTS} {pool} /dev/disk/by-id/virtio-d{index}"
        ))?;
        test.check(rc == 0, &format!("zpool create {pool}"));
        vm.must(&format!("mkbe {pool}"))?;
        vm.must(&format!(
            "zfs set org.zfsbootmenu:commandline='{BE_CMDLINE}' {pool}/ROOT/be"
        ))?;
    }
    vm.must("finish")?;
    vm.must("cryptsetup close auto0 || true")?;
    // tforeign belongs to another host and stays imported, as after a crash
    vm.must(&foreign_import())?;
    let (rc, out) = vm.sh_show("od -An -tx4 /etc/hostid; zpool list -H -o name")?;
    test.check(
        rc == 0 && out.contains("deadbeef") && out.contains("tforeign"),
        "tforeign imported by host deadbeef",
    );
    vm.sh("sync")?;
    vm.poweroff()
}

/// The command that writes the other host's ID and imports `tforeign`.
fn foreign_import() -> String {
    let hostid = FOREIGN.iter().fold(String::new(), |mut out, byte| {
        write!(out, "\\x{byte:02x}").ok();
        out
    });
    format!("printf '{hostid}' > /etc/hostid && zpool import -N tforeign")
}

/// Formats d0's volume on a loop device and enrols it for TPM2 without a
/// PIN, with a software TPM on TCP; the loop device and the TPM are released
/// on every path.
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
    let mut part = None;
    let result = swtpm_running(settings, |tcti| prepare_volume(test, tcti, &mut part));
    if let Some(part) = part {
        Command::new("losetup").arg("-d").arg(part).output().ok();
    }
    result
}

/// Runs `with` with the TCTI of the software TPM of the group, on TCP,
/// stopped on every path.
fn swtpm_running<T>(settings: &Settings, with: impl FnOnce(&str) -> Result<T>) -> Result<T> {
    let pid_file = settings.disks(GROUP).join("swtpm.pid");
    let port = swtpm::start_tcp(&tpm_dir(settings), &pid_file, "not-need-init,startup-clear")?;
    let result = with(&swtpm::tcti(port));
    swtpm::stop_pid_file(&pid_file);
    result
}

/// Formats the volume of d0 and enrols it; its loop device in `part`.
fn prepare_volume(test: &Test, tcti: &str, part: &mut Option<String>) -> Result<()> {
    let path = crate::disks::disk(test.settings(), GROUP, 0, "384M", true)?;
    partition(&path, &[("auto0", None)])?;
    let device = loop_partition(&path, 1)?;
    *part = Some(device.clone());
    Command::new("udevadm").arg("settle").output().ok();
    run_shell(&format!(
        "printf '%s' {PW} | cryptsetup luksFormat -q --type luks2 {FAST_PBKDF} --key-file=- \
         {device}"
    ))?;
    let enrolled = Command::new("systemd-cryptenroll")
        .arg(format!("--tpm2-device={tcti}"))
        .args(["--tpm2-pcrs=9", "--tpm2-with-pin=no"])
        .arg(&device)
        .env("PASSWORD", PW)
        .output()
        .map_err(Error::io("running systemd-cryptenroll"))?;
    test.check(
        enrolled.status.success(),
        "systemd-cryptenroll TPM2 without a PIN, bound to PCR 9, on auto0",
    );
    Ok(())
}

/// Starts the machine of `scenario` with the disks `indexes` and the
/// command line `append`, with the software TPM when `tpm`, saving its logs
/// on the FAT disk `logs` when there is one.
fn start<'t>(
    test: &'t Test,
    scenario: &str,
    (append, indexes): (&str, &[usize]),
    (tpm, logs): (bool, Option<&Path>),
) -> Result<Vm<'t>> {
    let settings = test.settings();
    let image = image(settings)?;
    let mut disks = indexes
        .iter()
        .map(|&index| disk(settings, index, false))
        .collect::<Result<Vec<_>>>()?;
    let mut append = append.to_owned();
    if let Some(logs) = logs {
        append.push_str(&format!(" zbm.save_logs=LABEL={LOGS_LABEL}"));
        disks.push(Disk::new(logs.to_owned(), "logs"));
    }
    let mut machine = Machine::boot(&image, &append).with_disks(disks);
    if tpm {
        let state = tpm_dir(settings);
        Swtpm::new(&state)?.clear_lockout()?;
        machine.tpm = Some(state);
    }
    Vm::start(test, scenario, &machine)
}

/// The text of the machine's serial console.
fn serial_text(vm: &Vm<'_>) -> Result<String> {
    let path = vm.serial_log();
    let bytes = fs::read(&path).map_err(Error::io(format!("reading {}", path.display())))?;
    Ok(clean(&String::from_utf8_lossy(&bytes)))
}

/// `unattended`: the three boots.
fn unattended(test: &Test) -> Result<()> {
    unlocked_itself(test)?;
    waits_for_someone(test)?;
    typed(test)
}

/// With the TPM, `zbm.prefer=tauto`: nothing is asked, `tforeign` stays out
/// and `tauto`'s boot environment boots by itself.
fn unlocked_itself(test: &Test) -> Result<()> {
    let append = format!("{BASE} zbm.prefer=tauto");
    let scenario = "unattended-tpm";
    let logs = logs_disk(test, scenario)?;
    let mut vm = start(test, scenario, (&append, &[0, 1, 2]), (true, Some(&logs)))?;
    let boot = follow_boot(&mut vm, Follow::default())?;
    vm.kill();
    check_unlocked_itself(test, &boot, &saved_log(test, scenario, &logs)?);
    check_be(test, &boot, "tauto", &["tauto"], &[]);
    Ok(())
}

/// The checks of [`unlocked_itself`], on its `boot` and the kernel `log`
/// it saved.
fn check_unlocked_itself(test: &Test, boot: &Boot, log: &str) {
    test.check(
        !boot.has_prompt("luks") && !boot.has_prompt("tpm2pin"),
        &format!(
            "the TPM unlocked auto0 with nothing asked ({})",
            boot.prompts_text()
        ),
    );
    test.check(
        log.contains(HELD),
        "start-up holds to the volume that unlocked",
    );
    let pools = boot.pools();
    test.check(
        pools.get("tauto") == Some("ONLINE") && !pools.contains("tforeign"),
        &format!(
            "tauto imported, tforeign of another host not adopted ({})",
            pools.sorted_names()
        ),
    );
}

/// With the TPM, `zbm.prefer=tplain`: `tplain` is on no volume that
/// unlocked itself, so its boot environment waits in the menu.
fn waits_for_someone(test: &Test) -> Result<()> {
    let append = format!("{BASE} zbm.prefer=tplain");
    let mut vm = start(
        test,
        "unattended-plain",
        (&append, &[0, 1, 2]),
        (true, None),
    )?;
    vm.expect1(prompt::NEW_MENU, 1200)?;
    test.step("the menu shows; waiting past the countdown's 3 seconds");
    thread::sleep(Duration::from_secs(15));
    let serial = serial_text(&vm)?;
    let kernel = kernel_log_in_shell(&mut vm)?;
    vm.kill();
    check_waits(test, &serial, &kernel);
    Ok(())
}

/// The checks of [`waits_for_someone`], on its `serial` log and the lines of
/// its `kernel` log.
fn check_waits(test: &Test, serial: &str, kernel: &str) {
    test.check(
        kernel.contains(&format!("tplain/ROOT/be {WAITS}")),
        "start-up says why tplain's boot environment does not boot by itself",
    );
    test.check(
        !serial.contains(BE_BOOTED) && !serial.contains(" for tplain/ROOT/be"),
        "nothing booted by itself",
    );
}

/// Without the TPM, `zbm.prefer=tplain`: the passphrase is typed, a person
/// is there, and `tplain`'s boot environment boots by itself.
fn typed(test: &Test) -> Result<()> {
    let append = format!("{BASE} zbm.prefer=tplain");
    let scenario = "attended";
    let logs = logs_disk(test, scenario)?;
    let mut vm = start(test, scenario, (&append, &[0, 1, 2]), (false, Some(&logs)))?;
    let follow = Follow::default().answer(Kind::Luks, &[PW]);
    let boot = follow_boot(&mut vm, follow)?;
    vm.kill();
    check_typed(test, &boot, &saved_log(test, scenario, &logs)?);
    check_be(test, &boot, "tplain", &["tplain"], &[]);
    Ok(())
}

/// The checks of [`typed`], on its `boot` and the kernel `log` it saved.
fn check_typed(test: &Test, boot: &Boot, log: &str) {
    test.check(
        boot.has_prompt("luks"),
        &format!("the passphrase was asked ({})", boot.prompts_text()),
    );
    test.check(
        !log.is_empty() && !log.contains(HELD),
        "start-up keeps the usual defaults (the saved kernel log does not say it holds)",
    );
}

/// `lockdown`: `lockdown=integrity`, and `tplain`'s boot environment, whose
/// kernel is not signed, is not loaded.
fn lockdown(test: &Test) -> Result<()> {
    let append = format!("{BASE} zbm.prefer=tplain lockdown=integrity");
    let mut vm = start(test, "lockdown", (&append, &[1]), (false, None))?;
    let index = vm.expect(&[NOT_LOADED, BE_BOOTED], 1200)?;
    let log = if index == 0 {
        after_refusal(&mut vm)?
    } else {
        String::new()
    };
    vm.kill();
    check_lockdown(test, index == 0, &log);
    Ok(())
}

/// The checks of [`lockdown`]: whether the boot said the kernel was not
/// loaded, and the lines of its kernel `log`.
fn check_lockdown(test: &Test, not_loaded: bool, log: &str) {
    test.check(
        not_loaded,
        "the kernel was not loaded (if it booted, the kernel of the image may lack the lockdown \
         LSM, or the boot environment's kernel is signed)",
    );
    test.check(
        log.contains(REFUSED),
        "ZFSBootMenu saw lockdown and kept to kexec_file_load",
    );
}

/// `measured`: with the TPM, `tauto`'s boot environment boots by itself,
/// measured.
fn measured(test: &Test) -> Result<()> {
    let append = format!("{BASE} zbm.prefer=tauto");
    let mut vm = start(test, "measured", (&append, &[0]), (true, None))?;
    let boot = follow_boot(&mut vm, Follow::default())?;
    test.check(
        !boot.has_prompt("luks"),
        "the TPM unlocked auto0, bound to PCR 9, before the boot was measured",
    );
    check_be(test, &boot, "tauto", &["tauto"], &[]);
    let payload = Payload::read(test.settings(), &be_kver(test.settings())?)?;
    check_measured(test, &boot, &payload);
    Ok(())
}

/// The SHA-256 digests of the boot environment's kernel and initramfs, as
/// they are on its dataset: the payload's.
#[derive(Debug, Eq, PartialEq)]
struct Payload {
    kernel: Vec<u8>,
    initramfs: Vec<u8>,
}

impl Payload {
    /// The digests of the payload's files, of the kernel `kver`.
    fn read(settings: &Settings, kver: &str) -> Result<Payload> {
        let boot = settings.work.join("payload/boot");
        let digest = |name: String| {
            let path = boot.join(name);
            let bytes =
                fs::read(&path).map_err(Error::io(format!("reading {}", path.display())))?;
            Ok::<_, Error>(Sha256::digest(&bytes).to_vec())
        };
        Ok(Payload {
            kernel: digest(format!("vmlinuz-{kver}"))?,
            initramfs: digest(format!("initramfs-{kver}.img"))?,
        })
    }
}

/// One record of the event log, read apart from the runtime's reader, as
/// ADR 0008 describes it: its PCR, its event, the value it names, and its
/// SHA-256 digest.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Record {
    pcr: u64,
    event: String,
    value: String,
    sha256: Vec<u8>,
}

/// The bytes of the hexadecimal `text`, if it is that.
fn unhex(text: &str) -> Option<Vec<u8>> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect()
}

/// The records of the event log the boot environment printed (`BE-ML| `
/// lines, a JSON record each, whose record separators it printed as
/// spaces), as far as they read.
fn printed_log(be: &str) -> Vec<Record> {
    be.lines()
        .filter_map(|line| line.split_once("BE-ML| ").map(|(_, record)| record.trim()))
        .map_while(|record| {
            let json: serde_json::Value = serde_json::from_str(record).ok()?;
            let sha256 = json.get("digests")?.as_array()?.iter().find_map(|digest| {
                (digest.get("hashAlg")?.as_str()? == "sha256")
                    .then(|| unhex(digest.get("digest")?.as_str()?))?
            })?;
            Some(Record {
                pcr: json.get("pcr")?.as_u64()?,
                event: json.pointer("/content/event")?.as_str()?.to_owned(),
                value: json.pointer("/content/value")?.as_str()?.to_owned(),
                sha256,
            })
        })
        .collect()
}

/// The SHA-256 value of `pcr` after `records`: from zeros, or a start's
/// value, each extension the digest of the old value and the record's.
fn replay(records: &[Record], pcr: u64) -> Vec<u8> {
    records
        .iter()
        .filter(|record| record.pcr == pcr)
        .fold(vec![0; 32], |value, record| {
            if record.event == "start" {
                record.sha256.clone()
            } else {
                Sha256::new()
                    .chain_update(&value)
                    .chain_update(&record.sha256)
                    .finalize()
                    .to_vec()
            }
        })
}

/// The value of `pcr` in the SHA-256 bank that the boot environment read
/// (`BE-PCR: 9:sha256=...`), if it did.
fn be_pcr(boot: &Boot, pcr: u64) -> Option<Vec<u8>> {
    let prefix = format!("{pcr}:sha256=");
    let text = boot
        .be_lines
        .get("BE-PCR")?
        .iter()
        .find_map(|line| line.strip_prefix(&prefix))?;
    unhex(text)
}

/// The checks of [`measured`], on its `boot` and the `payload`'s digests.
fn check_measured(test: &Test, boot: &Boot, payload: &Payload) {
    let records = printed_log(&boot.be);
    let events: Vec<&str> = records.iter().map(|record| record.event.as_str()).collect();
    test.check(
        events
            == [
                "start",
                "start",
                "kernel",
                "initramfs",
                "command-line",
                "separator",
            ],
        &format!("the boot environment kept ZFSBootMenu's event log ({events:?})"),
    );
    for pcr in [8, 9] {
        test.check(
            be_pcr(boot, pcr) == Some(replay(&records, pcr)),
            &format!("PCR {pcr} is what the log replays to"),
        );
    }
    let digest = |event: &str| {
        records
            .iter()
            .find(|record| record.event == event)
            .map(|record| record.sha256.as_slice())
    };
    test.check(
        digest("kernel") == Some(payload.kernel.as_slice())
            && digest("initramfs") == Some(payload.initramfs.as_slice()),
        "the kernel and initramfs were measured as they are on the dataset (as predicted)",
    );
    let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let measured_line = records
        .iter()
        .find(|record| record.event == "command-line")
        .map(|record| words(&record.value));
    test.check(
        measured_line == Some(words(boot.be_line("BE-CMDLINE"))),
        "the command line measured is the one the kernel got",
    );
    let start = records
        .iter()
        .find(|record| record.event == "start" && record.pcr == 9);
    test.check(
        start.is_some_and(|start| be_pcr(boot, 9).is_some_and(|now| now != start.sha256)),
        "PCR 9 moved on from the value auto0's token is bound to",
    );
}
