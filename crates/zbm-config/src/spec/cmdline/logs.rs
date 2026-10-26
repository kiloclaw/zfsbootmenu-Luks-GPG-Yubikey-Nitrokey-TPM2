//! The logs ZFSBootMenu keeps on the EFI system partition (ROADMAP.md #9).

use super::super::{Doc, OptionSpec, Reader, Source, Value};

/// `zbm.save_logs`: where the runtime saves its journal, crash reports and
/// kernel log before it boots, or `off`.
pub static SAVE_LOGS: OptionSpec = OptionSpec {
    name: "zbm.save_logs",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "Save the logs of each boot on the EFI system partition",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.save_logs**",
        body: SAVE_LOGS_DOC,
    },
    deprecation: None,
    examples: &[
        "zbm.save_logs",
        "zbm.save_logs=PARTUUID=0fc63daf-8483-4772-8e79-3d69d8477de4",
        "zbm.save_logs=LABEL=EFI",
    ],
};

const SAVE_LOGS_DOC: &str = r"
In images built with the Rust runtime, save the logs of each boot on an EFI system partition, which
survive the boot and a restart: the runtime journal, the crash reports and the kernel log go to a
directory named for the time (UTC), such as */EFI/zfsbootmenu/logs/2026-10-24T01-29-15Z*, once
the selected kernel is loaded, before the teardown hooks (which may unbind a USB device the
partition is on), and after a program of the runtime crashes. The newest five stay. In images with
graphics drivers, the kernel log is written again last before *kexec*, with how the GPUs were
handed back (see **zbm.gpu_untested**), when the partition is still there.
The partition is mounted only for the write, and must be FAT, with the *vfat* module in the image.
With no value, or **esp**, it is the partition the boot loader loaded ZFSBootMenu from (systemd's
EFI stub records it in ZFSBootMenu's EFI bundles), or else the machine's only EFI system partition;
a tag such as *PARTUUID=...* or *LABEL=...*, or a device path, names another. **zbm debug saved**
lists the saved boots from the recovery shell, and **zbm debug save** saves now. **zbm report**
writes a bug report on the same partition, below */EFI/zfsbootmenu/reports*, or in a directory it is
given, such as a USB stick: the versions, the pools' status, each LUKS header's tokens and key
slots, the TPM's state and the logs, with no key material. Logs are off by default and with
**zbm.save_logs=off**. The journal holds no secrets: fields that could are written as *[redacted]*.

**zbm.save_logs=LABEL=EFI**

  Save the logs on the partition labelled *EFI*.
";
