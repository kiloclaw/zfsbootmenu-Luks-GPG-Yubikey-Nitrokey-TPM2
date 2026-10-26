//! The GPUs known to come back after `kexec` (ROADMAP.md #17): handed back
//! by ZFSBootMenu ([`super::hand_back`]), they started again under the
//! booted kernel with a working display. Only these get a graphics driver,
//! unless `zbm.gpu_untested` asks for the others too; a GPU joins the list
//! once it booted twenty times in a row that way.

use super::{Gpu, QEMU_VGA, VIRTIO};

/// A GPU known to come back: its PCI vendor and device ID, and its name.
struct Known {
    vendor: u16,
    device: u16,
    name: &'static str,
}

/// The GPUs known to come back after `kexec`.
const KNOWN: &[Known] = &[
    // QEMU's standard VGA (`bochs`) and its virtio GPU (`virtio_gpu`): the
    // VM test `runtime-rust-gpu`
    Known {
        vendor: QEMU_VGA,
        device: 0x1111,
        name: "QEMU standard VGA",
    },
    Known {
        vendor: VIRTIO,
        device: 0x1050,
        name: "virtio GPU",
    },
];

/// The name of `gpu` if it is known to come back after `kexec`.
pub(super) fn known_name(gpu: &Gpu) -> Option<&'static str> {
    KNOWN
        .iter()
        .find(|known| known.vendor == gpu.vendor && known.device == gpu.device)
        .map(|known| known.name)
}
