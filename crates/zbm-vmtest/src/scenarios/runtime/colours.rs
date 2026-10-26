//! `runtime-rust-colours`: the console fonts of the Rust image leave the
//! console its 16 colours (ROADMAP.md #20). A font of more than 256 glyphs
//! takes a bit of each character's attribute for its glyph: on a VGA text
//! console that is the bit of the bright foreground colours, which the menu
//! uses for titles, warnings and secondary text.
//!
//! The image adds the `virtio_gpu` driver and, as a control, the whole
//! 512-glyph Terminus 8×14 at [`WHOLE`]. For each font, the test draws a
//! bar of the 16 foreground colours on `/dev/tty1`, the 8 normal ones and
//! the 8 bright ones that bold gives, as the theme draws them, takes a
//! screenshot from QEMU's monitor and counts the colours of the bar's row:
//!
//! - on QEMU's standard VGA, the VGA text console (`vgacon`): the image's
//!   256-glyph Terminus 8×14 must show 16 colours, and the whole font 8,
//!   which shows that the test sees the difference;
//! - on a virtio GPU, the framebuffer console (`fbcon`): the image's font
//!   must show 16; what the whole font shows is noted, a measure.

use std::fs;
use std::ops::Range;
use std::path::Path;

use crate::error::{Error, Result};
use crate::images::{Image, ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

/// The image's Terminus 8×14, a 256-glyph subset; VGA text mode draws
/// fonts 8 pixels wide only.
const SUBSET: &str = "/usr/share/zfsbootmenu/fonts/ter-v14b.psf";

/// The whole Terminus 8×14 of 512 glyphs, added to the image.
const WHOLE: &str = "/build/ter-v14b-512.psf";

/// The height of the fonts' cells, in pixels.
const CELL_HEIGHT: usize = 14;

/// The line of the screen the bar is drawn on, from 1.
const BAR_LINE: usize = 10;

/// The colours of the bar with a font that keeps them all: black, which is
/// also the background, and 15 others.
pub(super) const ALL_COLOURS: usize = 16;

/// The colours of the bar on a VGA text console with a 512-glyph font:
/// bold no longer gives the bright colours.
pub(super) const VGA_512_COLOURS: usize = 8;

/// Draws the bar on `/dev/tty1`: four full blocks in each colour, the
/// normal ones (`ESC [0;3Nm`) and the bright ones (`ESC [1;3Nm`), on a
/// cleared screen without its cursor, then waits for the screen to be
/// updated.
pub(super) const BAR: &str = "b=$(printf '\\342\\226\\210%.0s' 1 2 3 4); \
    { printf '\\e[?25l\\e[H\\e[2J\\e[10;1H'; \
    for w in 0 1; do for c in 0 1 2 3 4 5 6 7; do printf '\\e[%s;3%sm%s' $w $c \"$b\"; done; done; \
    printf '\\e[0m'; } > /dev/tty1; sleep 1";

/// The console drivers bound to the virtual consoles.
const CONSOLES: &str = "cat /sys/class/vtconsole/vtcon*/name";

/// Boots the image on both consoles and counts the colours of each font.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let image = image(test)?;
    for (name, gpu, driver) in [
        ("vga", false, "VGA+"),
        ("fbcon", true, "frame buffer device"),
    ] {
        let mut vm = boot(test, &image, name, gpu)?;
        let (_, consoles) = vm.sh(CONSOLES)?;
        test.check(
            consoles.contains(driver),
            &format!(
                "{name}: tty1 is on the {driver} console ({})",
                consoles.trim()
            ),
        );
        let subset = draw(test, &mut vm, name, SUBSET, &screendump)?;
        check_colours(
            test,
            subset,
            ALL_COLOURS,
            &format!("{name}: the image's font"),
        );
        let whole = draw(test, &mut vm, name, WHOLE, &screendump)?;
        if gpu {
            test.note(&format!(
                "{name}: the whole 512-glyph font shows {}",
                describe(whole)
            ));
        } else {
            check_colours(
                test,
                whole,
                VGA_512_COLOURS,
                &format!("{name}: the whole font"),
            );
        }
        vm.kill();
    }
    Ok(())
}

/// The Rust image with the virtio GPU driver and the whole font.
fn image(test: &Test) -> Result<Image> {
    let settings = test.settings();
    let source = settings.repo.join("legacy/zfsbootmenu/fonts/ter-v14b.psf");
    let font = fs::read(&source).map_err(Error::io(format!("reading {}", source.display())))?;
    zbm_image(settings, &spec(font))
}

/// The image of the test, which adds the whole font `font` at [`WHOLE`]:
/// the build directory, where it is copied, is `/build` in the build
/// container.
pub(super) fn spec(font: Vec<u8>) -> ImageSpec {
    let name = WHOLE.strip_prefix("/build/").unwrap_or(WHOLE);
    ImageSpec {
        runtime: Some(ZbmRuntime::Rust),
        conf: format!("add_drivers+=\" virtio_gpu \"\ninstall_items+=\" {WHOLE} \""),
        files: vec![(name.into(), font)],
        ..ImageSpec::new("colours")
    }
}

/// Boots `image` into the recovery shell, on QEMU's standard VGA or, with
/// `gpu`, on a virtio GPU.
fn boot<'t>(test: &'t Test, image: &Image, name: &str, gpu: bool) -> Result<Vm<'t>> {
    let settings = test.settings();
    let mut machine =
        Machine::boot(image, super::ESCAPE_APPEND).with_disks([super::disk(settings, 0)?]);
    if gpu {
        machine.extra.extend([
            "-vga".into(),
            "none".into(),
            "-device".into(),
            "virtio-vga".into(),
        ]);
    }
    let mut vm = Vm::start(test, name, &machine)?;
    super::escape_to_shell(&mut vm)?;
    Ok(vm)
}

/// Writes a screenshot of the machine to `path` (a PPM image).
pub(super) fn screendump(vm: &Vm<'_>, path: &Path) -> Result<()> {
    let reply = vm.monitor(&format!("screendump {}", path.display()))?;
    if !reply.is_empty() {
        vm.test().show(&format!("monitor: screendump: {reply}"));
    }
    Ok(())
}

/// Loads `font` on `/dev/tty1`, draws the bar and returns the number of
/// colours of its line in the screenshot `dump` takes, which is kept as
/// `<boot>-<font>.ppm`; `None` when the font cannot be loaded or the
/// screenshot read.
pub(super) fn draw(
    test: &Test,
    vm: &mut Vm<'_>,
    boot: &str,
    font: &str,
    dump: &dyn Fn(&Vm<'_>, &Path) -> Result<()>,
) -> Result<Option<usize>> {
    let (rc, _) = vm.sh(&format!("setfont -C /dev/tty1 {font}"))?;
    if rc != 0 {
        test.check(false, &format!("{boot}: setfont loads {font}"));
        return Ok(None);
    }
    vm.must(BAR)?;
    let name = Path::new(font)
        .file_stem()
        .map_or("font".into(), |stem| stem.to_string_lossy().into_owned());
    let path = test.dir().join(format!("{boot}-{name}.ppm"));
    dump(vm, &path)?;
    let start = (BAR_LINE - 1) * CELL_HEIGHT;
    Ok(fs::read(&path)
        .ok()
        .and_then(|ppm| colours(&ppm, start..start + CELL_HEIGHT)))
}

/// Checks that the bar shows `want` colours.
pub(super) fn check_colours(test: &Test, found: Option<usize>, want: usize, what: &str) {
    test.check(
        found == Some(want),
        &format!("{what}: {want} colours ({})", describe(found)),
    );
}

/// What a count of colours says.
fn describe(found: Option<usize>) -> String {
    found.map_or("no screenshot".into(), |count| format!("{count} colours"))
}

/// The width, the height and the pixels of a binary PPM image (`P6`) of
/// 8-bit samples, as QEMU's `screendump` writes it.
pub(super) fn parse_ppm(ppm: &[u8]) -> Option<(usize, usize, &[u8])> {
    let mut rest = ppm.strip_prefix(b"P6")?;
    let mut numbers = [0_usize; 3];
    for number in &mut numbers {
        rest = skip_space(rest);
        let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
        *number = std::str::from_utf8(rest.get(..digits)?)
            .ok()?
            .parse()
            .ok()?;
        rest = rest.get(digits..)?;
    }
    let [width, height, maximum] = numbers;
    // One white space character ends the header.
    let pixels = rest.get(1..)?;
    (maximum > 0 && maximum <= 255).then_some((width, height, pixels))
}

/// The number of colours in the pixel rows `rows` of a PPM image; `None`
/// when it is not one, or has none of the rows.
pub(super) fn colours(ppm: &[u8], rows: Range<usize>) -> Option<usize> {
    let (width, height, pixels) = parse_ppm(ppm)?;
    if rows.end > height || rows.is_empty() {
        return None;
    }
    let row = width.checked_mul(3)?;
    let bytes = pixels.get(rows.start.checked_mul(row)?..rows.end.checked_mul(row)?)?;
    let mut seen: Vec<&[u8]> = bytes.chunks_exact(3).collect();
    seen.sort_unstable();
    seen.dedup();
    Some(seen.len())
}

/// The number of colours of a whole PPM image.
pub(super) fn screen_colours(ppm: &[u8]) -> Option<usize> {
    let (_, height, _) = parse_ppm(ppm)?;
    colours(ppm, 0..height)
}

/// `bytes` from its first character that is neither white space nor in a
/// comment.
fn skip_space(mut bytes: &[u8]) -> &[u8] {
    loop {
        match bytes.first() {
            Some(byte) if byte.is_ascii_whitespace() => bytes = &bytes[1..],
            Some(b'#') => {
                let end = bytes
                    .iter()
                    .position(|&byte| byte == b'\n')
                    .unwrap_or(bytes.len());
                bytes = &bytes[end..];
            }
            _ => return bytes,
        }
    }
}

#[cfg(test)]
mod tests;
