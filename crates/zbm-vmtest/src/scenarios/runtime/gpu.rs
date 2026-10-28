//! `runtime-rust-gpu`: the graphics drivers of the image load before
//! anything is asked, for the GPUs `zbm.gpu` chooses (ROADMAP.md #17).
//!
//! The image takes `virtio_gpu` and `bochs`, the drivers of QEMU's virtual
//! GPUs, which need no firmware, through the build option
//! `zfsbootmenu_graphics_drivers`; the machine has QEMU's standard VGA,
//! which `bochs` drives, and a virtio GPU at `0000:00:09.0`, one that is
//! not a VGA (`virtio-gpu-pci`): QEMU leaves its standard VGA out when
//! another VGA is added. The image must list the drivers and keep udev
//! from loading them. Three boots, each escaping to the recovery shell:
//!
//! - without `zbm.gpu`: both drivers load, before the countdown, and each
//!   GPU has its driver;
//! - `zbm.gpu=0000:00:09.0`: only the virtio GPU has a driver; the
//!   standard VGA has a driver override that no driver matches;
//! - `zbm.gpu=off`: neither GPU has a driver, which also shows that udev
//!   did not load the drivers before ZFSBootMenu (`modprobe.d`).
//!
//! A fourth boot lets the countdown boot the environment: before `kexec`,
//! ZFSBootMenu hands both GPUs back, unbound from their drivers (a teardown
//! hook of the image turns kernel messages on the serial console on again,
//! to show it), and the environment's kernel brings up both displays with
//! its own drivers (`BE-DRM`). It saves its logs on a FAT disk
//! (`zbm.save_logs`), whose saved kernel log must hold the hand-back, which
//! comes after the save.

use crate::boot::{Follow, follow_boot};
use crate::disks::vfat_disk;
use crate::error::{Error, Result};
use crate::images::{Image, ImageSpec, zbm_image};
use crate::qemu::{Disk, Machine};
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

/// The PCI address of the virtio GPU.
const VIRTIO: &str = "0000:00:09.0";

/// The drivers of the two GPUs, one line each: the standard VGA's (vendor
/// 0x1234) and the virtio GPU's, or `none`. The image has no `basename`,
/// so Bash finds the name of the driver's directory itself.
const DRIVERS: &str = "for d in /sys/bus/pci/devices/*; do \
    [ \"$(cat $d/vendor)\" = 0x1234 ] || continue; \
    if [ -e $d/driver ]; then p=$(cd $d/driver && pwd -P); echo \"vga ${p##*/}\"; \
    else echo 'vga none'; fi; done; v=none; \
    for p in /sys/bus/pci/devices/0000:00:09.0/virtio*/driver; do \
    [ -e \"$p\" ] && v=$(cd \"$p\" && pwd -P) && v=${v##*/}; done; \
    echo \"virtio $v\"";

/// ZFSBootMenu's messages in the kernel log about the graphics drivers, and
/// the start-up program's first, in their order. `zfsbootmenu-preinit`
/// turns kernel messages on the console off before it loads the drivers,
/// so they are not on the serial console.
const KERNEL_LOG: &str =
    "dmesg | grep -E 'ZFSBootMenu: (graphics|zbm.gpu|running init stage)' | head -n 20";

/// What the image holds for the drivers, and what the kernel has: the
/// image's list of drivers, its modprobe.d file, the drivers loaded, and
/// the display controllers.
const STATE: &str = "echo \"list: $(cat /etc/zfsbootmenu/graphics-drivers)\"; \
    sed 's/^/modprobe.d: /' /etc/modprobe.d/zfsbootmenu-graphics.conf; \
    grep -E '^(virtio_gpu|bochs) ' /proc/modules | sed 's/^/loaded: /'; \
    for d in /sys/bus/pci/devices/*; do \
    echo \"pci: ${d##*/} $(cat $d/class) $(cat $d/vendor) $(cat $d/device)\"; done | grep ' 0x03'";

/// The driver overrides of the GPUs.
const OVERRIDES: &str =
    "cat /sys/bus/pci/devices/*/driver_override 2>/dev/null | grep -v '^(null)$'";

/// A teardown hook: kernel messages on the serial console again, which
/// `zfsbootmenu-preinit` turned off, for those of the hand-back.
const PRINTK_HOOK: (&str, &str) = (
    "hooks/teardown.d/50-printk.sh",
    "#!/bin/sh\necho 7 > /proc/sys/kernel/printk\n",
);

/// The label of the FAT disk the boot that boots the environment saves its
/// logs on.
const LOGS_LABEL: &str = "ZBMGPULOGS";

/// The command line of the boot that boots the environment.
const KEXEC_APPEND: &str = "console=ttyS0 zbm.timeout=3 loglevel=7 zbm.save_logs=LABEL=ZBMGPULOGS";

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
        files: vec![(PRINTK_HOOK.0.to_owned(), PRINTK_HOOK.1.as_bytes().to_vec())],
        ..ImageSpec::new("auto-gpu-load")
    };
    let image = zbm_image(settings, &spec)?;
    for boot in &BOOTS {
        let mut vm = start(test, &image, boot)?;
        check(test, &mut vm, boot)?;
        vm.kill();
    }
    check_kexec(test, &image)
}

/// The machine with both GPUs, booted from `image` with `append`.
fn machine(test: &Test, image: &Image, append: &str) -> Result<Machine> {
    let mut machine = Machine::boot(image, append).with_disks([super::disk(test.settings(), 0)?]);
    machine
        .extra
        .extend(["-device".into(), "virtio-gpu-pci,addr=0x9,id=gpu1".into()]);
    Ok(machine)
}

/// The countdown boots the environment: both GPUs are handed back before
/// `kexec`, and its kernel brings up both displays.
fn check_kexec(test: &Test, image: &Image) -> Result<()> {
    let dir = test.settings().disks("runtime");
    std::fs::create_dir_all(&dir).map_err(Error::io(format!("creating {}", dir.display())))?;
    let logs = vfat_disk(&dir.join("gpu-logs.img"), LOGS_LABEL, &[], "64M", 512)?;
    let machine = machine(test, image, KEXEC_APPEND)?.with_disks([Disk::new(logs.clone(), "logs")]);
    let mut vm = Vm::start(test, "kexec", &machine)?;
    let follow = Follow {
        timeout: Some(1200),
        ..Follow::default()
    };
    let boot = follow_boot(&mut vm, follow)?;
    let serial = super::serial_text(&vm)?;
    let be_start = serial.find("BE-INIT-START").unwrap_or(serial.len());
    let before_kexec = &serial[..be_start];
    test.check(
        before_kexec.contains(&format!("graphics: handed {VIRTIO} (1af4:"))
            && before_kexec.contains("back: unbound from virtio-pci"),
        "kexec: the virtio GPU was unbound from its driver before kexec",
    );
    test.check(
        before_kexec.contains("back: unbound from bochs"),
        "kexec: the standard VGA was unbound from its driver before kexec",
    );
    let drm = boot.be_line("BE-DRM");
    test.check(
        drm.contains("=virtio_gpu") && drm.contains("=bochs"),
        &format!("kexec: the environment's kernel brought up both displays ({drm})"),
    );
    vm.kill();
    let saved = super::saved::saved_kernel_log(&logs)?;
    test.check(
        saved.contains(&format!("graphics: handed {VIRTIO} (1af4:")),
        "kexec: the saved kernel log holds the hand-back",
    );
    Ok(())
}

fn start<'t>(test: &'t Test, image: &Image, boot: &Boot) -> Result<Vm<'t>> {
    let append = format!("{}{}", super::ESCAPE_APPEND, boot.option);
    let mut vm = Vm::start(test, boot.label, &machine(test, image, &append)?)?;
    super::escape_to_shell(&mut vm)?;
    Ok(vm)
}

fn check(test: &Test, vm: &mut Vm<'_>, boot: &Boot) -> Result<()> {
    let label = boot.label;
    let (_, state) = vm.sh_show(STATE)?;
    test.check(
        state.contains("list: virtio_gpu bochs"),
        &format!("{label}: the image lists its graphics drivers"),
    );
    test.check(
        state.contains("modprobe.d: blacklist virtio_gpu")
            && state.contains("modprobe.d: blacklist bochs"),
        &format!("{label}: the image keeps udev from loading them"),
    );
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
    let (_, log) = vm.sh_show(KERNEL_LOG)?;
    let loaded = log.find("graphics: loaded virtio_gpu");
    // The start-up program, whose start-up view shows the countdown
    let startup = log.find("running init stage");
    if boot.virtio == "none" {
        test.check(loaded.is_none(), &format!("{label}: no driver loaded"));
        test.check(
            log.contains("zbm.gpu=off: the graphics drivers stay unloaded"),
            &format!("{label}: the log says why"),
        );
    } else {
        test.check(
            log.contains("graphics: 0000:00:09.0 (1af4:1050) is a virtio GPU"),
            &format!("{label}: the virtio GPU is known to come back after kexec"),
        );
        test.check(
            loaded
                .zip(startup)
                .is_some_and(|(loaded, startup)| loaded < startup),
            &format!("{label}: the drivers load before start-up and its countdown"),
        );
    }
    Ok(())
}
