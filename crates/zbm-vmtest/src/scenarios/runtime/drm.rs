//! `runtime-rust-drm`: the menu prototype on the display without a
//! terminal (ROADMAP.md #19, step 1). The Rust image is built with the
//! `menu-demo` feature and the `virtio_gpu` and `evdev` drivers, and boots
//! twice:
//!
//! - on a virtio GPU, whose DRM driver is `virtio_gpu` (the card's device
//!   is the PCI function it sits on, driven by `virtio-pci`);
//! - on QEMU's standard VGA in a VESA mode set at boot (`vga=0x318`), which
//!   `simpledrm` drives on the firmware's framebuffer, without a graphics
//!   driver.
//!
//! Each time, from the recovery shell on the serial console, `zbm debug
//! menu-demo` draws the demo's menu on the display. The test reads the
//! screen's text from the file the demo writes, counts the colours in a
//! QEMU screenshot, types keys on QEMU's keyboard (`sendkey`), which reach
//! the demo through the input devices, and checks that the kernel console
//! draws again once the menu has booted and handed the display back.

use std::fs;
use std::path::Path;

use crate::error::Result;
use crate::images::{Image, ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

use super::colours::{BAR, screen_colours, screendump};

/// The driver of the first DRM card.
const DRIVER: &str = "readlink -f /sys/class/drm/card0/device/driver";

/// Starts the demo in the background, with the screen's text in
/// `/tmp/menu.txt`.
const START: &str = "{ /libexec/zbm debug menu-demo --wait --dump /tmp/menu.txt \
    > /tmp/demo.log 2>&1 & }";

/// Waits for the main view, and prints the screen's text.
const MENU: &str = "for i in {1..150}; do grep -q 'Boot Environments' /tmp/menu.txt 2>/dev/null \
    && break; sleep 0.2; done; cat /tmp/menu.txt";

/// Keeps the line of the selected environment.
const SELECTED: &str = "grep '▶' /tmp/menu.txt > /tmp/before; cat /tmp/before";

/// Waits for the selection to move, and prints its line.
const MOVED: &str = "for i in {1..50}; do \
    [ \"$(grep '▶' /tmp/menu.txt)\" != \"$(cat /tmp/before)\" ] && break; sleep 0.2; done; \
    grep '▶' /tmp/menu.txt";

/// Waits for the demo to end, and prints what it said.
const ENDED: &str = "for i in {1..50}; do grep -q 'menu-demo:' /tmp/demo.log && break; \
    sleep 0.2; done; cat /tmp/demo.log";

/// The fewest colours of the menu on the console theme: text, titles, muted
/// text, the selection, the default's mark and the status bar.
pub(super) const MENU_COLOURS: usize = 6;

/// The colours of the bar the kernel console draws once it is back.
pub(super) const CONSOLE_COLOURS: usize = 16;

/// What the test does through QEMU's monitor: screenshots and keys.
pub(super) struct Monitor {
    /// Writes a screenshot of the machine to a path.
    pub(super) dump: fn(&Vm<'_>, &Path) -> Result<()>,
    /// Presses a key, by QEMU's name (`down`, `ret`).
    pub(super) key: fn(&Vm<'_>, &str) -> Result<()>,
}

/// Presses `key` on the machine's keyboard.
fn sendkey(vm: &Vm<'_>, key: &str) -> Result<()> {
    vm.monitor(&format!("sendkey {key}")).map(drop)
}

/// Boots the image on both displays and checks the menu on each.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let image = zbm_image(test.settings(), &spec())?;
    let monitor = Monitor {
        dump: screendump,
        key: sendkey,
    };
    // The card's device on virtio-vga is the PCI function, whose driver is
    // virtio-pci; virtio_gpu drives the virtio device below it.
    for (name, driver) in [("virtio", "virtio"), ("simpledrm", "simple")] {
        let mut vm = boot(test, &image, name)?;
        check_menu(test, &mut vm, name, driver, &monitor)?;
        vm.kill();
    }
    Ok(())
}

/// The image of the test.
pub(super) fn spec() -> ImageSpec {
    ImageSpec {
        runtime: Some(ZbmRuntime::Rust),
        conf: "add_drivers+=\" virtio_gpu evdev \"".into(),
        features: Some("zbm/menu-demo".into()),
        ..ImageSpec::new("drm")
    }
}

/// Boots `image` into the recovery shell: on a virtio GPU, or for
/// `simpledrm` on the standard VGA in a VESA mode.
fn boot<'t>(test: &'t Test, image: &Image, name: &str) -> Result<Vm<'t>> {
    let settings = test.settings();
    let append = if name == "simpledrm" {
        format!("{} vga=0x318", super::ESCAPE_APPEND)
    } else {
        super::ESCAPE_APPEND.to_owned()
    };
    let mut machine = Machine::boot(image, &append).with_disks([super::disk(settings, 0)?]);
    if name == "virtio" {
        machine
            .extra
            .extend(["-vga", "none", "-device", "virtio-vga"].map(String::from));
    }
    let mut vm = Vm::start(test, name, &machine)?;
    super::escape_countdown(&mut vm)?;
    vm.shell_ready()?;
    Ok(vm)
}

/// The number of colours in a screenshot taken now, kept as
/// `<boot>-<what>.ppm`.
fn screen(test: &Test, vm: &Vm<'_>, monitor: &Monitor, file: &str) -> Result<Option<usize>> {
    let path = test.dir().join(format!("{file}.ppm"));
    (monitor.dump)(vm, &path)?;
    Ok(fs::read(&path).ok().and_then(|ppm| screen_colours(&ppm)))
}

/// Runs the demo on the display of `vm` and checks it, from its DRM driver
/// to the kernel console drawing again.
pub(super) fn check_menu(
    test: &Test,
    vm: &mut Vm<'_>,
    name: &str,
    driver: &str,
    monitor: &Monitor,
) -> Result<()> {
    let (_, found) = vm.sh(DRIVER)?;
    test.check(
        found.contains(driver),
        &format!("{name}: card0 is driven by {driver} ({})", found.trim()),
    );
    vm.must(START)?;
    let (_, text) = vm.sh(MENU)?;
    test.check(
        text.contains("Boot Environments") && text.contains("tank/ROOT"),
        &format!("{name}: the menu draws its main view"),
    );
    let drawn = screen(test, vm, monitor, &format!("{name}-menu"))?;
    test.check(
        drawn.is_some_and(|count| count >= MENU_COLOURS),
        &format!("{name}: the display shows it in colour ({drawn:?} colours)"),
    );
    let (_, before) = vm.sh(SELECTED)?;
    (monitor.key)(vm, "down")?;
    let (_, after) = vm.sh(MOVED)?;
    test.check(
        !before.trim().is_empty() && before.trim() != after.trim(),
        &format!("{name}: Down from the keyboard moves the selection"),
    );
    (monitor.key)(vm, "ret")?;
    let (_, log) = vm.sh(ENDED)?;
    test.note(log.trim());
    test.check(
        log.contains("menu-demo:") && log.contains("Booting"),
        &format!("{name}: Enter boots, and the demo ends"),
    );
    vm.must(BAR)?;
    let console = screen(test, vm, monitor, &format!("{name}-console"))?;
    test.check(
        console.is_some_and(|count| count >= CONSOLE_COLOURS),
        &format!("{name}: the kernel console draws again ({console:?} colours)"),
    );
    Ok(())
}

#[cfg(test)]
mod tests;
