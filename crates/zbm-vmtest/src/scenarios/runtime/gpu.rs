//! `runtime-rust-gpu`: the graphics drivers of the image load before
//! anything is asked, for the GPUs `zbm.gpu` chooses (ROADMAP.md #17).
//!
//! The image takes `virtio_gpu` and `bochs`, the drivers of QEMU's virtual
//! GPUs, which need no firmware, through the build option
//! `zfsbootmenu_graphics_drivers`; the machine has QEMU's standard VGA,
//! which `bochs` drives, and a virtio GPU at `0000:00:09.0`. Three boots,
//! each escaping to the recovery shell:
//!
//! - without `zbm.gpu`: both drivers load, before the countdown, and each
//!   GPU has its driver;
//! - `zbm.gpu=0000:00:09.0`: only the virtio GPU has a driver; the
//!   standard VGA has a driver override that no driver matches;
//! - `zbm.gpu=off`: neither GPU has a driver, which also shows that udev
//!   did not load the drivers before ZFSBootMenu (`modprobe.d`).

use crate::error::Result;
use crate::images::{Image, ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

/// The PCI address of the virtio GPU.
const VIRTIO: &str = "0000:00:09.0";

/// The drivers of the two GPUs, one line each: the standard VGA's (vendor
/// 0x1234) and the virtio GPU's, or `none`.
const DRIVERS: &str = "for d in /sys/bus/pci/devices/*; do \
    [ \"$(cat $d/vendor)\" = 0x1234 ] || continue; \
    if [ -e $d/driver ]; then echo \"vga $(basename $(readlink $d/driver))\"; \
    else echo 'vga none'; fi; done; \
    v=$(ls -d /sys/bus/pci/devices/0000:00:09.0/virtio*/driver 2>/dev/null | head -n 1); \
    if [ -n \"$v\" ]; then echo \"virtio $(basename $(readlink $v))\"; \
    else echo 'virtio none'; fi";

/// The driver overrides of the GPUs.
const OVERRIDES: &str =
    "cat /sys/bus/pci/devices/*/driver_override 2>/dev/null | grep -v '^(null)$'";

/// A boot: its label, the `zbm.gpu` option, and the drivers the two GPUs
/// must have.
struct Boot {
    label: &'static str,
    option: &'static str,
    vga: &'static str,
    virtio: &'static str,
}

const BOOTS: [Boot; 3] = [
    Boot {
        label: "all",
        option: "",
        vga: "bochs",
        virtio: "virtio_gpu",
    },
    Boot {
        label: "virtio",
        option: " zbm.gpu=0000:00:09.0",
        vga: "none",
        virtio: "virtio_gpu",
    },
    Boot {
        label: "off",
        option: " zbm.gpu=off",
        vga: "none",
        virtio: "none",
    },
];

/// Boots the image three times and checks the drivers.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let spec = ImageSpec {
        runtime: Some(ZbmRuntime::Rust),
        conf: "zfsbootmenu_graphics_drivers=\"virtio_gpu bochs\"".into(),
        ..ImageSpec::new("auto-gpu-load")
    };
    let image = zbm_image(settings, &spec)?;
    for boot in &BOOTS {
        let mut vm = start(test, &image, boot)?;
        check(test, &mut vm, boot)?;
        vm.kill();
    }
    Ok(())
}

fn start<'t>(test: &'t Test, image: &Image, boot: &Boot) -> Result<Vm<'t>> {
    let settings = test.settings();
    let mut machine = Machine::boot(image, &format!("{}{}", super::ESCAPE_APPEND, boot.option))
        .with_disks([super::disk(settings, 0)?]);
    machine
        .extra
        .extend(["-device".into(), "virtio-vga,addr=0x9,id=gpu1".into()]);
    let mut vm = Vm::start(test, boot.label, &machine)?;
    super::escape_to_shell(&mut vm)?;
    Ok(vm)
}

fn check(test: &Test, vm: &mut Vm<'_>, boot: &Boot) -> Result<()> {
    let label = boot.label;
    let (_, drivers) = vm.sh_show(DRIVERS)?;
    test.check(
        drivers.contains(&format!("vga {}", boot.vga)),
        &format!("{label}: the standard VGA's driver is {}", boot.vga),
    );
    test.check(
        drivers.contains(&format!("virtio {}", boot.virtio)),
        &format!("{label}: the virtio GPU's driver is {}", boot.virtio),
    );
    let (_, overrides) = vm.sh_show(OVERRIDES)?;
    let left_alone = boot.option.contains(VIRTIO);
    test.check(
        overrides.contains("zfsbootmenu-none") == left_alone,
        &format!("{label}: a driver override only for a GPU zbm.gpu leaves alone"),
    );
    let log = super::serial_text(vm)?;
    let loaded = log.find("graphics: loaded virtio_gpu");
    // The countdown, not the kernel's "Booting paravirtualized kernel"
    let countdown = log.find("tplain/ROOT/be in");
    if boot.virtio == "none" {
        test.check(loaded.is_none(), &format!("{label}: no driver loaded"));
        test.check(
            log.contains("zbm.gpu=off: the graphics drivers stay unloaded"),
            &format!("{label}: the log says why"),
        );
    } else {
        test.check(
            loaded
                .zip(countdown)
                .is_some_and(|(loaded, countdown)| loaded < countdown),
            &format!("{label}: the drivers load before the countdown"),
        );
    }
    Ok(())
}
