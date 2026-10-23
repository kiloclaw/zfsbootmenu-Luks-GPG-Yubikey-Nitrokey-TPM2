//! `runtime-rust-autosize`: the console font of the Rust image follows the
//! display (`zbm.autosize`, HiDPI). The machine has a virtio GPU with a
//! 3840×2400 display; the image adds its driver, which ZFSBootMenu images
//! leave out, so that the display's EDID reaches `/sys/class/drm`.
//!
//! - At boot the control terminal is the serial console, so the
//!   early-setup hook runs `zbm console-autosize` and leaves the font alone.
//! - With the EDID, which QEMU gives 100 dpi, `zbm console-autosize
//!   --console /dev/tty1` loads the 20-pixel font: 384×120.
//! - Without it (`edid=off`), the legacy rule: the largest font that leaves
//!   110 columns, 32 pixels: 240×75.
//! - With `zbm.autosize=48`, the 24-pixel font doubled: 160×50.

use crate::error::Result;
use crate::expect::regex;
use crate::images::{Image, ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

/// What one run of `zbm console-autosize` on `/dev/tty1` printed and left.
const RUN: &str = "/libexec/zbm console-autosize --console /dev/tty1; \
    echo \"AUTOSIZE rc=$? size=$(stty -F /dev/tty1 size) \
    $(dmesg | grep 'font set to' | tail -n 1 | sed 's/.*font set to //')\"";

/// The density the run found, from the kernel log.
const DENSITY: &str = "dmesg | grep -o 'display density [a-z0-9]*' | tail -n 1";

/// The skip at boot, from the kernel log.
const SKIPPED: &str =
    "dmesg | grep -c 'console autosize skipped: the control terminal is not a virtual console'";

/// What a run did: its exit status, the console's size, the font kept.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Report {
    rc: u32,
    columns: u32,
    rows: u32,
    font: String,
    doubled: bool,
}

/// The report in the output of [`RUN`].
pub(super) fn parse_report(output: &str) -> Option<Report> {
    let pattern =
        regex(r"AUTOSIZE rc=(\d+) size=(\d+) (\d+) \S*/([^/,\s]+)(, doubled)?, /dev/tty1 is")
            .ok()?;
    let found = pattern.captures(output)?;
    let number = |index: usize| found.get(index)?.as_str().parse().ok();
    Some(Report {
        rc: number(1)?,
        rows: number(2)?,
        columns: number(3)?,
        font: found.get(4)?.as_str().to_owned(),
        doubled: found.get(5).is_some(),
    })
}

/// The report a run should give.
fn expected(font: &str, doubled: bool, columns: u32, rows: u32) -> Report {
    Report {
        rc: 0,
        columns,
        rows,
        font: font.to_owned(),
        doubled,
    }
}

/// Boots the image with and without the EDID and checks the fonts.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let spec = ImageSpec {
        runtime: Some(ZbmRuntime::Rust),
        conf: "add_drivers+=\" virtio_gpu \"".into(),
        ..ImageSpec::new("auto-gpu")
    };
    let image = zbm_image(settings, &spec)?;

    let mut vm = boot(test, &image, "edid", true)?;
    let (_, skipped) = vm.sh(SKIPPED)?;
    test.check(
        skipped.trim().parse::<u32>().unwrap_or(0) > 0,
        "the hook leaves the serial console alone",
    );
    check_density(
        test,
        &mut vm,
        "display density 100",
        "the EDID gives 100 dpi",
    )?;
    check_run(
        test,
        &mut vm,
        &expected("ter-v20b.psf", false, 384, 120),
        "with the EDID",
    )?;
    vm.kill();

    let mut vm = boot(test, &image, "noedid", false)?;
    check_density(
        test,
        &mut vm,
        "display density unknown",
        "no EDID, no density",
    )?;
    check_run(
        test,
        &mut vm,
        &expected("ter-v32b.psf", false, 240, 75),
        "without the EDID",
    )?;
    vm.must("echo zbm.autosize=48 >> /zfsbootmenu/zbm.cmdline")?;
    check_run(
        test,
        &mut vm,
        &expected("ter-v24b.psf", true, 160, 50),
        "zbm.autosize=48",
    )?;
    vm.kill();
    Ok(())
}

/// Boots `image` with the virtio GPU, its EDID on or off, into the
/// recovery shell.
fn boot<'t>(test: &'t Test, image: &Image, name: &str, edid: bool) -> Result<Vm<'t>> {
    let settings = test.settings();
    let mut machine =
        Machine::boot(image, super::ESCAPE_APPEND).with_disks([super::disk(settings, 0)?]);
    let edid = if edid { "on" } else { "off" };
    machine.extra.extend([
        "-vga".into(),
        "none".into(),
        "-device".into(),
        format!("virtio-vga,edid={edid},xres=3840,yres=2400"),
    ]);
    let mut vm = Vm::start(test, name, &machine)?;
    super::escape_countdown(&mut vm)?;
    vm.shell_ready()?;
    Ok(vm)
}

/// Checks the density a run of `zbm console-autosize` logs.
fn check_density(test: &Test, vm: &mut Vm<'_>, density: &str, what: &str) -> Result<()> {
    vm.sh(RUN)?;
    let (_, found) = vm.sh(DENSITY)?;
    test.check(
        found.trim() == density,
        &format!("{what} ({})", found.trim()),
    );
    Ok(())
}

/// Runs `zbm console-autosize` and checks what it kept.
fn check_run(test: &Test, vm: &mut Vm<'_>, want: &Report, what: &str) -> Result<()> {
    let (_, output) = vm.sh(RUN)?;
    let report = parse_report(&output);
    test.check(
        report.as_ref() == Some(want),
        &format!(
            "{what}: {}{} at {}x{} ({})",
            want.font,
            if want.doubled { " doubled" } else { "" },
            want.columns,
            want.rows,
            output.trim()
        ),
    );
    Ok(())
}

#[cfg(test)]
mod tests;
