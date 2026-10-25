//! The QEMU command line of a test machine, and its monitor.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::images::Image;
use crate::settings::OVMF_CODE;

/// A disk attached as a virtio block device with a serial number, which
/// the guest sees as `/dev/disk/by-id/virtio-<serial>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Disk {
    /// The disk image.
    pub path: PathBuf,
    /// Its serial number.
    pub serial: String,
}

impl Disk {
    /// The disk `path` with the serial `serial`.
    pub fn new(path: impl Into<PathBuf>, serial: impl Into<String>) -> Disk {
        Disk {
            path: path.into(),
            serial: serial.into(),
        }
    }
}

/// The serial of the payload disk, which is only read: tests running side
/// by side can share it.
pub const PAYLOAD: &str = "payload";

/// What a test machine has.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Machine {
    /// Memory in MiB.
    pub mem: u32,
    /// Processors.
    pub smp: u32,
    /// The kernel, initramfs and command line to boot directly.
    pub kernel: Option<(PathBuf, PathBuf, String)>,
    /// Virtio disks.
    pub disks: Vec<Disk>,
    /// An NVMe disk and its logical block size.
    pub nvme: Option<(PathBuf, u32)>,
    /// USB sticks: image and drive name.
    pub usb: Vec<(PathBuf, String)>,
    /// UEFI firmware variables; `None`: BIOS.
    pub ovmf_vars: Option<PathBuf>,
    /// Options of a user network, if any.
    pub net: Option<String>,
    /// The state directory of a software TPM, if any.
    pub tpm: Option<PathBuf>,
    /// Other arguments.
    pub extra: Vec<String>,
    /// Whether its image runs shadow mode before each boot.
    pub shadow: bool,
}

impl Default for Machine {
    fn default() -> Machine {
        Machine {
            mem: 4096,
            smp: 2,
            kernel: None,
            disks: Vec::new(),
            nvme: None,
            usb: Vec::new(),
            ovmf_vars: None,
            net: None,
            tpm: None,
            extra: Vec::new(),
            shadow: false,
        }
    }
}

impl Machine {
    /// A machine that boots `image` with the command line `append`.
    pub fn boot(image: &Image, append: &str) -> Machine {
        Machine {
            shadow: image.shadow,
            ..Machine::kernel(&image.kernel, &image.initrd, append)
        }
    }

    /// A machine that boots `kernel` and `initrd` with `append`.
    pub fn kernel(kernel: &Path, initrd: &Path, append: &str) -> Machine {
        Machine {
            kernel: Some((kernel.to_owned(), initrd.to_owned(), append.to_owned())),
            ..Machine::default()
        }
    }

    /// With the disks `disks` added.
    #[must_use]
    pub fn with_disks(mut self, disks: impl IntoIterator<Item = Disk>) -> Machine {
        self.disks.extend(disks);
        self
    }
}

/// The sockets of a running machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sockets {
    /// The serial console (QEMU listens and waits for the harness).
    pub serial: PathBuf,
    /// The monitor.
    pub monitor: PathBuf,
    /// The software TPM's control socket, if any.
    pub tpm: Option<PathBuf>,
}

/// The `-drive` and `-device` arguments of a virtio disk.
pub fn disk_args(disk: &Disk) -> [String; 4] {
    // The payload disk is only read, so tests running side by side can
    // share it.
    let options = if disk.serial == PAYLOAD {
        "readonly=on,file.locking=off"
    } else {
        "discard=unmap"
    };
    let serial = &disk.serial;
    [
        "-drive".into(),
        format!(
            "file={},if=none,id=drv-{serial},format=raw,{options}",
            disk.path.display()
        ),
        "-device".into(),
        format!("virtio-blk-pci,drive=drv-{serial},serial={serial},id=dev-{serial}"),
    ]
}

/// The arguments of `qemu-system-x86_64` for `machine` with the
/// accelerator `accel`.
pub fn args(machine: &Machine, accel: &str, sockets: &Sockets) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-accel".into(),
        accel.into(),
        "-cpu".into(),
        "max".into(),
        "-m".into(),
        machine.mem.to_string(),
        "-smp".into(),
        machine.smp.to_string(),
        "-display".into(),
        "none".into(),
        "-no-reboot".into(),
        "-chardev".into(),
        format!(
            "socket,id=serial0,path={},server=on,wait=on",
            sockets.serial.display()
        ),
        "-serial".into(),
        "chardev:serial0".into(),
        "-monitor".into(),
        format!("unix:{},server,nowait", sockets.monitor.display()),
    ];
    if let Some(vars) = &machine.ovmf_vars {
        args.extend([
            "-machine".into(),
            "q35".into(),
            "-drive".into(),
            format!("if=pflash,format=raw,readonly=on,file={OVMF_CODE}"),
            "-drive".into(),
            format!("if=pflash,format=raw,file={}", vars.display()),
        ]);
    }
    if let Some((kernel, initrd, append)) = &machine.kernel {
        args.extend(["-kernel".into(), kernel.display().to_string()]);
        args.extend(["-initrd".into(), initrd.display().to_string()]);
        args.extend(["-append".into(), append.clone()]);
    }
    for disk in &machine.disks {
        args.extend(disk_args(disk));
    }
    args.extend(storage_args(machine));
    if let Some(net) = &machine.net {
        args.extend(["-netdev".into(), format!("user,id=n0,{net}")]);
        args.extend(["-device".into(), "e1000,netdev=n0".into()]);
    }
    args.extend(machine.extra.iter().cloned());
    if let Some(tpm) = &sockets.tpm {
        args.extend([
            "-chardev".into(),
            format!("socket,id=chrtpm,path={}", tpm.display()),
            "-tpmdev".into(),
            "emulator,id=tpm0,chardev=chrtpm".into(),
            "-device".into(),
            "tpm-tis,tpmdev=tpm0".into(),
        ]);
    }
    args
}

/// The NVMe disk, the USB controller and the USB sticks.
fn storage_args(machine: &Machine) -> Vec<String> {
    let mut args = Vec::new();
    if let Some((path, size)) = &machine.nvme {
        args.extend([
            "-drive".into(),
            format!(
                "file={},if=none,id=nv0,format=raw,discard=unmap",
                path.display()
            ),
            "-device".into(),
            "nvme,id=nvme0,serial=ZBMTEST0001".into(),
            "-device".into(),
            format!(
                "nvme-ns,drive=nv0,bus=nvme0,nsid=1,logical_block_size={size},\
                 physical_block_size={size}"
            ),
        ]);
    }
    args.extend(["-device".into(), "qemu-xhci,id=xhci".into()]);
    for (path, id) in &machine.usb {
        args.extend([
            "-drive".into(),
            format!("file={},if=none,id={id},format=raw", path.display()),
            "-device".into(),
            format!("usb-storage,bus=xhci.0,drive={id},id=dev-{id}"),
        ]);
    }
    args
}

/// Runs a command on the monitor at `socket`; returns its answer, without
/// prompts and echo.
///
/// # Errors
///
/// [`Error::Io`] when the monitor cannot be reached.
pub fn monitor(socket: &Path, command: &str) -> Result<String> {
    let io = Error::io(format!("monitor command {command}"));
    let talk = || -> std::io::Result<String> {
        let mut stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(Duration::from_millis(800)))?;
        thread::sleep(Duration::from_millis(300));
        let mut greeting = [0_u8; 65536];
        // The greeting and its prompt; how much of it arrives does not matter.
        let _greeting = stream.read(&mut greeting);
        stream.write_all(format!("{command}\n").as_bytes())?;
        thread::sleep(Duration::from_millis(800));
        let mut answer = vec![0_u8; 65536];
        let read = stream.read(&mut answer).unwrap_or_default();
        Ok(String::from_utf8_lossy(&answer[..read]).into_owned())
    };
    talk().map(|answer| monitor_reply(&answer)).map_err(io)
}

/// The reply in what the monitor sent back: its lines other than the
/// prompt and the echo.
pub fn monitor_reply(answer: &str) -> String {
    let text = crate::text::clean(answer);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with("(qemu)"))
        .collect();
    lines.join("\n").trim().to_owned()
}

#[cfg(test)]
mod tests;
