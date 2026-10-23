use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{Step, checks, cleanup, scripted, settings};

/// A PPM image `width` pixels wide whose row `y` is in the colour
/// `colour(x, y)` at `x`, with a comment in its header.
fn ppm(width: usize, height: usize, colour: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
    let mut bytes = format!("P6\n# QEMU\n{width} {height}\n255\n").into_bytes();
    for y in 0..height {
        for x in 0..width {
            bytes.extend_from_slice(&colour(x, y));
        }
    }
    bytes
}

/// The bar of `count` colours on line [`BAR_LINE`], on a black screen with
/// some white text above it.
fn screen(count: usize) -> Vec<u8> {
    let bar = (BAR_LINE - 1) * CELL_HEIGHT..BAR_LINE * CELL_HEIGHT;
    ppm(64, 30 * CELL_HEIGHT, |x, y| {
        if bar.contains(&y) {
            let shade = u8::try_from(x * count / 64 * 16).unwrap();
            [shade, shade, 0]
        } else if y < CELL_HEIGHT {
            [255, 255, 255]
        } else {
            [0, 0, 0]
        }
    })
}

#[test]
fn the_colours_of_the_bar_line_are_counted() {
    let rows = (BAR_LINE - 1) * CELL_HEIGHT..BAR_LINE * CELL_HEIGHT;

    assert_eq!(colours(&screen(16), rows.clone()), Some(16));
    assert_eq!(colours(&screen(8), rows.clone()), Some(8));
    // The text above the bar is left out.
    assert_eq!(colours(&screen(1), rows), Some(1));
    assert_eq!(colours(&screen(16), 0..CELL_HEIGHT), Some(1));
    // Black, white and the bar's 16, which include black.
    assert_eq!(screen_colours(&screen(16)), Some(17));
    assert_eq!(screen_colours(b"P3"), None);
}

#[test]
fn what_is_not_a_whole_ppm_image_has_no_colours() {
    let image = screen(16);

    assert_eq!(colours(b"P5\n1 1\n255\n\0", 0..1), None);
    assert_eq!(colours(b"P6\n1 1\n65535\n\0\0\0\0\0\0", 0..1), None);
    assert_eq!(colours(b"P6\n1 1\n0\n\0\0\0", 0..1), None);
    assert_eq!(colours(b"P6\n1 x\n255\n", 0..1), None);
    assert_eq!(colours(&image, 0..10_000), None);
    assert_eq!(colours(&image, 3..3), None);
    assert_eq!(
        colours(&image[..image.len() - 1], 0..30 * CELL_HEIGHT),
        None
    );
}

#[test]
fn the_bar_is_drawn_on_its_line_in_16_colours() {
    assert!(BAR.contains(&format!("[{BAR_LINE};1H")));
    assert!(BAR.contains("for w in 0 1; do for c in 0 1 2 3 4 5 6 7"));
    assert!(BAR.ends_with("> /dev/tty1; sleep 1"));
}

#[test]
fn a_font_is_loaded_drawn_and_counted_from_the_screenshot() {
    let settings = settings("runtime-colours-draw");
    let test = Test::new(&settings, "runtime-rust-colours").unwrap();
    let leak = |text: String| -> &'static str { Box::leak(text.into_boxed_str()) };
    let script = vec![
        Step::Hear(leak(format!("setfont -C /dev/tty1 {SUBSET}; echo RC=$?"))),
        Step::Say("RC=0\r\n"),
        Step::Hear(leak(format!("{BAR}; echo RC=$?"))),
        Step::Say("RC=0\r\n"),
        Step::Hear(leak(format!("setfont -C /dev/tty1 {WHOLE}; echo RC=$?"))),
        Step::Say("RC=0\r\n"),
        Step::Hear(leak(format!("{BAR}; echo RC=$?"))),
        Step::Say("RC=0\r\n"),
        Step::Hear(leak(format!("setfont -C /dev/tty1 {WHOLE}; echo RC=$?"))),
        Step::Say("setfont: KDFONTOP: Invalid argument\r\nRC=1\r\n"),
    ];
    let (mut vm, guest) = scripted(&test, script);
    let sixteen = |_: &Vm<'_>, path: &Path| -> Result<()> {
        fs::write(path, screen(16)).map_err(Error::io("writing the screenshot"))
    };
    let nothing = |_: &Vm<'_>, _: &Path| -> Result<()> { Ok(()) };

    let subset = draw(&test, &mut vm, "vga", SUBSET, &sixteen).unwrap();
    let whole = draw(&test, &mut vm, "vga", WHOLE, &nothing).unwrap();
    let refused = draw(&test, &mut vm, "vga", WHOLE, &sixteen).unwrap();
    check_colours(&test, subset, ALL_COLOURS, "vga: the image's font");
    check_colours(&test, whole, VGA_512_COLOURS, "vga: the whole font");

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!((subset, whole, refused), (Some(16), None, None));
    assert!(test.dir().join("vga-ter-v14b.ppm").exists());
    assert_eq!(
        checks(&test),
        [
            format!("FAIL - vga: setfont loads {WHOLE}"),
            "ok   - vga: the image's font: 16 colours (16 colours)".to_owned(),
            "FAIL - vga: the whole font: 8 colours (no screenshot)".to_owned(),
        ]
    );
    cleanup(&settings);
}

#[test]
fn the_image_adds_the_gpu_driver_and_the_whole_font() {
    let spec = spec(vec![1, 2, 3]);

    assert_eq!(spec.runtime, Some(ZbmRuntime::Rust));
    assert_eq!(spec.files, [("ter-v14b-512.psf".to_owned(), vec![1, 2, 3])]);
    assert!(
        spec.conf.contains("add_drivers+=\" virtio_gpu \""),
        "{}",
        spec.conf
    );
    assert!(
        spec.conf.contains(&format!("install_items+=\" {WHOLE} \"")),
        "{}",
        spec.conf
    );
}
