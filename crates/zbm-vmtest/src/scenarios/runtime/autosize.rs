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
//! - An image built with `Global.ConsoleFont: spleen` in the configuration
//!   of `generate-zbm`, which sets `zfsbootmenu_console_font`, has the
//!   Spleen fonts and their licence, and no Terminus: with the EDID, 18
//!   pixels make Spleen 8×16, 480×150; `zbm.autosize=64` loads the native
//!   32×64, 120×37, rather than 16×32 doubled.

use std::ops::RangeInclusive;

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

/// The density the EDID gives. QEMU makes the display 100 dpi, but an EDID
/// holds the size in whole millimetres (975 for 3840 pixels), or in whole
/// centimetres when its first descriptor has no size, as when QEMU puts the
/// timing of a large mode in an extension block (97 cm): the guest finds
/// 100 or 101 dpi.
const EDID_DPI: RangeInclusive<u32> = 99..=101;

/// The fonts of the image, and its Spleen licence, on one line.
const FONT_FILES: &str = "(cd /usr/share/zfsbootmenu/fonts && echo *)";

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
        Some(&EDID_DPI),
        "the EDID gives about 100 dpi",
    )?;
    check_run(
        test,
        &mut vm,
        &expected("ter-v20b.psf", false, 384, 120),
        "with the EDID",
    )?;
    vm.kill();

    let mut vm = boot(test, &image, "noedid", false)?;
    check_density(test, &mut vm, None, "no EDID, no density")?;
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
    run_spleen(test)
}

/// Boots an image with the Spleen fonts, with the EDID, and checks them.
fn run_spleen(test: &Test) -> Result<()> {
    let spec = ImageSpec {
        runtime: Some(ZbmRuntime::Rust),
        conf: "add_drivers+=\" virtio_gpu \"".into(),
        console_font: Some("spleen".into()),
        ..ImageSpec::new("auto-spleen")
    };
    let image = zbm_image(test.settings(), &spec)?;
    let mut vm = boot(test, &image, "spleen", true)?;
    check_font_files(test, &mut vm)?;
    check_run(
        test,
        &mut vm,
        &expected("spleen-8x16.psf", false, 480, 150),
        "Spleen with the EDID",
    )?;
    vm.must("echo zbm.autosize=64 >> /zfsbootmenu/zbm.cmdline")?;
    check_run(
        test,
        &mut vm,
        &expected("spleen-32x64.psf", false, 120, 37),
        "Spleen with zbm.autosize=64",
    )?;
    vm.kill();
    Ok(())
}

/// Checks that the image has the Spleen fonts, their licence and nothing
/// else.
fn check_font_files(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let (_, files) = vm.sh(FONT_FILES)?;
    let files = files.trim();
    test.check(
        files == SPLEEN_FILES,
        &format!("the image has the Spleen fonts and their licence ({files})"),
    );
    Ok(())
}

/// What [`FONT_FILES`] lists in the Spleen image.
const SPLEEN_FILES: &str =
    "LICENSE.spleen spleen-12x24.psf spleen-16x32.psf spleen-32x64.psf spleen-8x16.psf";

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
    super::escape_to_shell(&mut vm)?;
    Ok(vm)
}

/// Checks the density a run of `zbm console-autosize` logs: within `want`
/// dots per inch, or unknown when `want` is `None`.
fn check_density(
    test: &Test,
    vm: &mut Vm<'_>,
    want: Option<&RangeInclusive<u32>>,
    what: &str,
) -> Result<()> {
    vm.sh(RUN)?;
    let (_, found) = vm.sh(DENSITY)?;
    test.check(
        density_matches(&found, want),
        &format!("{what} ({})", found.trim()),
    );
    Ok(())
}

/// Whether the logged density `found`, `display density 100`, is within
/// `want`, or `display density unknown` when `want` is `None`.
pub(super) fn density_matches(found: &str, want: Option<&RangeInclusive<u32>>) -> bool {
    match (found.trim().strip_prefix("display density "), want) {
        (Some("unknown"), None) => true,
        (Some(dpi), Some(range)) => dpi.parse().is_ok_and(|dpi| range.contains(&dpi)),
        _ => false,
    }
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
