//! UEFI boot from an NVMe SSD with 4 KiB sectors, without any crypttab.
//!
//! The NVMe disk has an EFI system partition and one LUKS2 partition
//! (`zroot-luks`) with 4 KiB encryption sectors and the TRIM and work-queue
//! flags stored in its header. The ZFSBootMenu EFI image (built by the
//! container) is the firmware's fallback loader `EFI/BOOT/BOOTX64.EFI`; its
//! command line names the volume with `zbm.luks=UUID=`. The ZFS 2.4.4 boot
//! environment unlocks the volume with systemd's `luks.uuid=`, `luks.key=`
//! and `luks.crypttab=no` parameters.
//!
//! Scenarios:
//!
//! - `setup`: partitions on the host, creates LUKS and the pool in the
//!   recovery shell;
//! - `boot`: firmware, ZFSBootMenu (one passphrase), boot environment;
//! - `usbkey`: `zbm.luks_key=` names a key file on a FAT USB stick: no
//!   prompt;
//! - `usblate`: the stick is inserted while ZFSBootMenu waits for it: no
//!   prompt;
//! - `nostick`: no stick: ZFSBootMenu waits, then asks for the passphrase.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::boot::{Boot, Follow, check_be, check_teardown, follow_boot};
use crate::disks::vfat_disk;
use crate::error::{Error, Result};
use crate::expect::regex;
use crate::gpt::{self, Partition};
use crate::images::{ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::{FAST_PBKDF, OVMF_VARS, PW, Settings};
use crate::vm::Vm;

use super::payload;

/// The logical sector size of the NVMe disk.
const SECTOR: u64 = 4096;

/// [`SECTOR`] as QEMU's NVMe namespace takes it.
const NVME_BLOCK: u32 = 4096;

/// The size of the EFI system partition: 128 MiB in 4 KiB sectors.
const ESP_SECTORS: u64 = 32768;

/// The first sector of the EFI system partition.
const ESP_START: u64 = 256;

/// The size of the NVMe disk.
const NVME_SIZE: u64 = 2 << 30;

/// The scenarios that boot with the key file on a USB stick.
const USB_SCENARIOS: [&str; 3] = ["usbkey", "usblate", "nostick"];

/// The files of the group.
struct Files {
    /// Their directory.
    dir: PathBuf,
}

impl Files {
    /// The files of the group under the work directory of `settings`.
    fn new(settings: &Settings) -> Files {
        Files {
            dir: settings.disks("firmware"),
        }
    }

    /// The NVMe disk.
    fn nvme(&self) -> PathBuf {
        self.dir.join("nvme.img")
    }

    /// The UUID of the LUKS volume, which `setup` records.
    fn uuid(&self) -> PathBuf {
        self.dir.join("uuid")
    }

    /// The USB stick.
    fn stick(&self) -> PathBuf {
        self.dir.join("usb.img")
    }
}

/// Runs `setup`, `boot`, `usbkey`, `usblate` or `nostick`.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub fn run(test: &Test, scenario: &str) -> Result<()> {
    let files = Files::new(test.settings());
    if scenario == "setup" {
        return setup(test, &files);
    }
    let uuid = fs::read_to_string(files.uuid()).unwrap_or_default();
    let uuid = uuid.trim();
    if uuid.is_empty() {
        test.check(false, "run 'firmware.py setup' first");
        return Ok(());
    }
    if !matches!(scenario, "boot" | "usbkey" | "usblate" | "nostick") {
        test.check(false, &format!("unknown scenario {scenario}"));
        return Ok(());
    }
    let boot = boot(test, &files, scenario, uuid)?;
    checks(test, &boot, uuid);
    Ok(())
}

/// Creates the NVMe disk: a GPT with the EFI system partition and the
/// LUKS partition.
fn make_nvme(files: &Files) -> Result<()> {
    let io = |what: &str| Error::io(format!("{what} {}", files.nvme().display()));
    fs::create_dir_all(&files.dir).map_err(io("creating the directory of"))?;
    let parts = [
        Partition {
            name: "ESP",
            type_guid: gpt::ESP,
            first: ESP_START,
            sectors: Some(ESP_SECTORS),
        },
        Partition {
            name: "zroot-luks",
            type_guid: gpt::LINUX,
            first: ESP_START + ESP_SECTORS,
            sectors: None,
        },
    ];
    fs::remove_file(files.nvme()).ok();
    gpt::write(&files.nvme(), NVME_SIZE, SECTOR, &parts).map_err(io("writing"))
}

/// The size of the EFI system partition, as `vfat_disk` takes it.
fn esp_size() -> String {
    format!("{}M", ESP_SECTORS * SECTOR / (1 << 20))
}

/// Copies the file `from` into `to` at the byte `offset`.
fn copy_at(from: &Path, to: &Path, offset: u64) -> io::Result<()> {
    let mut out = OpenOptions::new().write(true).open(to)?;
    let mut source = File::open(from)?;
    out.seek(SeekFrom::Start(offset))?;
    io::copy(&mut source, &mut out)?;
    Ok(())
}

/// Writes an EFI system partition holding the EFI image `efi` into the
/// NVMe disk.
fn write_esp(files: &Files, efi: &Path) -> Result<()> {
    let esp = files.dir.join("esp.img");
    let image = fs::read(efi).map_err(Error::io(format!("reading {}", efi.display())))?;
    let contents: &[(&str, &[u8])] = &[("EFI/BOOT/BOOTX64.EFI", &image)];
    vfat_disk(&esp, "EFI", contents, &esp_size(), SECTOR)?;
    copy_at(&esp, &files.nvme(), ESP_START * SECTOR).map_err(Error::io(
        "copying the EFI system partition into the NVMe disk",
    ))?;
    fs::remove_file(&esp).map_err(Error::io(format!("removing {}", esp.display())))
}

/// A copy of the firmware variables' template, for one boot.
fn ovmf(files: &Files) -> Result<PathBuf> {
    let path = files.dir.join("OVMF_VARS.fd");
    fs::copy(OVMF_VARS, &path).map_err(Error::io(format!("copying {OVMF_VARS}")))?;
    Ok(path)
}

/// Partitions the NVMe disk on the host, then creates the LUKS volume and
/// the pool in the recovery shell.
fn setup(test: &Test, files: &Files) -> Result<()> {
    make_nvme(files)?;
    let settings = test.settings();
    let image = zbm_image(settings, &ImageSpec::new("auto"))?;
    let mut machine = Machine::boot(&image, "console=ttyS0 zbm.luks=off zbm.retry_delay=3")
        .with_disks([payload(settings)?]);
    machine.nvme = Some((files.nvme(), NVME_BLOCK));
    let mut vm = Vm::start(test, "setup", &machine)?;
    vm.recovery_shell()?;
    vm.send_functions()?;
    let (_, out) = vm.sh_show("cat /sys/block/nvme0n1/queue/logical_block_size")?;
    test.check(
        out.trim().ends_with("4096"),
        "NVMe namespace has 4096-byte logical blocks",
    );
    format_luks(test, &mut vm)?;
    create_pool(&mut vm)?;
    let (_, out) = vm.sh_show("echo UUID=$UUID")?;
    let uuid = regex(r"UUID=([0-9a-f-]{36})")?
        .captures(&out)
        .and_then(|captures| captures.get(1))
        .map(|uuid| uuid.as_str().to_owned());
    test.check(uuid.is_some(), "volume UUID recorded");
    fs::write(files.uuid(), uuid.unwrap_or_default())
        .map_err(Error::io("recording the volume UUID"))?;
    vm.must("finish")?;
    vm.poweroff()
}

/// Formats and opens the LUKS volume, with its flags stored in its header.
fn format_luks(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    vm.must(&format!(
        "printf '%s' {PW} | cryptsetup luksFormat -q --type luks2 --sector-size 4096 \
         {FAST_PBKDF} --key-file=- /dev/disk/by-partlabel/zroot-luks"
    ))?;
    vm.must("UUID=$(cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks)")?;
    vm.must(&format!(
        "printf '%s' {PW} | cryptsetup open --allow-discards --perf-no_read_workqueue \
         --perf-no_write_workqueue --persistent --key-file=- \
         /dev/disk/by-partlabel/zroot-luks luks-$UUID"
    ))?;
    let (_, out) = vm.sh_show(
        "cryptsetup luksDump /dev/disk/by-partlabel/zroot-luks | grep -E 'Flags|sector'",
    )?;
    test.check(
        out.contains("allow-discards")
            && out.contains("no-read-workqueue")
            && out.contains("no-write-workqueue"),
        "activation flags stored in the LUKS2 header",
    );
    Ok(())
}

/// Creates the pool and the boot environment, with the boot environment's
/// command line.
fn create_pool(vm: &mut Vm<'_>) -> Result<()> {
    vm.must(
        "zpool create -f -o ashift=12 -o autotrim=on -o compatibility=openzfs-2.4-linux \
         -O compression=lz4 -O acltype=posixacl -O xattr=sa -m none zroot /dev/mapper/luks-$UUID",
    )?;
    vm.must("mkbe zroot")?;
    vm.must(
        "zfs set org.zfsbootmenu:commandline=\"console=ttyS0 loglevel=4 luks.crypttab=no \
         luks.uuid=$UUID luks.key=$UUID=/etc/zbmtest/be.key \
         luks.options=$UUID=discard,no-read-workqueue,no-write-workqueue\" zroot/ROOT/be",
    )?;
    Ok(())
}

/// The command line of the EFI image.
fn efi_cmdline(uuid: &str, usb: bool) -> String {
    let mut cmdline = format!("console=ttyS0 loglevel=4 zbm.timeout=3 zbm.luks=UUID={uuid}");
    if usb {
        cmdline.push_str(" zbm.luks_key=/keys/zroot.key:LABEL=ZBMKEY");
    }
    cmdline
}

/// The EFI image for the volume `uuid`, with the key file on a USB stick
/// when `usb`.
fn efi_image(settings: &Settings, uuid: &str, usb: bool) -> Result<PathBuf> {
    let variant = if usb { "firmware-usb" } else { "firmware" };
    let spec = ImageSpec {
        efi: true,
        cmdline: Some(efi_cmdline(uuid, usb)),
        ..ImageSpec::new(variant)
    };
    Ok(zbm_image(settings, &spec)?.efi)
}

/// Boots the NVMe disk with UEFI as `scenario` says; returns what the boot
/// showed.
fn boot(test: &Test, files: &Files, scenario: &str, uuid: &str) -> Result<Boot> {
    let usb = USB_SCENARIOS.contains(&scenario);
    write_esp(files, &efi_image(test.settings(), uuid, usb)?)?;
    let stick = files.stick();
    vfat_disk(
        &stick,
        "ZBMKEY",
        &[("keys/zroot.key", PW.as_bytes())],
        "64M",
        512,
    )?;
    let sticks = if scenario == "usbkey" {
        vec![(stick.clone(), "usb0".to_owned())]
    } else {
        Vec::new()
    };
    let machine = Machine {
        nvme: Some((files.nvme(), NVME_BLOCK)),
        ovmf_vars: Some(ovmf(files)?),
        usb: sticks,
        mem: 4096,
        ..Machine::default()
    };
    let mut vm = Vm::start(test, scenario, &machine)?;
    match scenario {
        "boot" => {
            let boot = follow_boot(&mut vm, Follow::default())?;
            test.check(
                boot.prompts == ["luks"],
                &format!(
                    "one passphrase prompt in ZFSBootMenu ({})",
                    boot.prompts_text()
                ),
            );
            Ok(boot)
        }
        "usbkey" => {
            let boot = follow_boot(&mut vm, Follow::default())?;
            test.check(
                !boot.has_prompt("luks"),
                &format!(
                    "no passphrase prompt: the key file on the stick was used ({})",
                    boot.prompts_text()
                ),
            );
            Ok(boot)
        }
        "usblate" => usblate(test, &mut vm, &stick),
        _ => nostick(test, &mut vm),
    }
}

/// Inserts the stick while ZFSBootMenu waits for it.
fn usblate(test: &Test, vm: &mut Vm<'_>, stick: &Path) -> Result<Boot> {
    vm.expect1("Insert the key device", 2400)?;
    test.step("ZFSBootMenu waits for the key device");
    thread::sleep(Duration::from_secs(1));
    vm.hotplug_usb(stick, "usb0")?;
    let boot = follow_boot(vm, Follow::default())?;
    test.check(
        !boot.has_prompt("luks"),
        &format!(
            "no passphrase prompt after the stick arrived ({})",
            boot.prompts_text()
        ),
    );
    Ok(boot)
}

/// Lets ZFSBootMenu wait for a stick that never arrives.
fn nostick(test: &Test, vm: &mut Vm<'_>) -> Result<Boot> {
    vm.expect1("Insert the key device", 2400)?;
    test.step("ZFSBootMenu waits for the key device, which never arrives");
    let boot = follow_boot(vm, Follow::default())?;
    test.check(
        boot.prompts == ["luks"],
        &format!(
            "after the wait, one passphrase prompt ({})",
            boot.prompts_text()
        ),
    );
    Ok(boot)
}

/// The checks of every boot: the pool, the teardown, the boot environment
/// and the volume as the boot environment opened it.
fn checks(test: &Test, boot: &Boot, uuid: &str) {
    let pools = boot.pools();
    test.check(
        pools.get("zroot") == Some("ONLINE"),
        &format!("ZFSBootMenu imported zroot: {}", pools.show("zroot")),
    );
    check_teardown(test, boot, &[]);
    check_be(test, boot, "zroot", &["zroot"], &[]);
    let cmdline = boot.be_line("BE-CMDLINE");
    test.check(
        cmdline.contains("luks.crypttab=no")
            && cmdline.contains(&format!("luks.uuid={uuid}"))
            && cmdline.contains("root=zfs:zroot/ROOT/be"),
        "boot environment command line has the luks.* parameters and root=zfs:",
    );
    let dm: Vec<&str> = boot
        .be
        .lines()
        .filter(|line| line.contains("BE-DM| "))
        .collect();
    let dm = dm.join("\n");
    test.check(
        dm.contains("sector_size:4096"),
        "boot environment: 4096-byte encryption sectors",
    );
    test.check(
        dm.contains("allow_discards")
            && dm.contains("no_read_workqueue")
            && dm.contains("no_write_workqueue"),
        "boot environment: TRIM and no-workqueue flags active",
    );
    test.check(
        !boot.has_prompt("be-luks-prompt"),
        "the boot environment did not ask for a passphrase",
    );
}

#[cfg(test)]
mod tests;
