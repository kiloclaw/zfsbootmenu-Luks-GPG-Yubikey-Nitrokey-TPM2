//! The fonts of images: the console fonts, and the TrueType faces and
//! fallback fonts of the console on the display.

use super::*;

#[test]
fn spleen_is_the_console_font_of_rust_images_that_ask_for_it() {
    let mut parity = Parity::new(
        env!("CARGO_PKG_NAME"),
        "image",
        "spleen_is_the_console_font_of_rust_images_that_ask_for_it",
    );
    let settings = [
        ("zfsbootmenu_zbm", ZBM),
        ("zfsbootmenu_console_font", "spleen"),
    ];

    let mut rust_settings = settings.to_vec();
    rust_settings.push(("zfsbootmenu_runtime", "rust"));
    let rust = dracut_install(&mut parity, &rust_settings);
    let fonts: Vec<(&str, usize)> = rust.fonts.iter().map(|(n, g)| (n.as_str(), *g)).collect();
    // The licence is no font: 0 glyphs.
    assert_eq!(
        fonts,
        [
            ("DejaVuSansMono-Bold.ttf", 0),
            ("DejaVuSansMono-BoldOblique.ttf", 0),
            ("DejaVuSansMono-Oblique.ttf", 0),
            ("DejaVuSansMono.ttf", 0),
            ("LICENSE.dejavu", 0),
            ("LICENSE.spleen", 0),
            ("spleen-12x24.psf", 256),
            ("spleen-16x32.psf", 256),
            ("spleen-32x64.psf", 256),
            ("spleen-8x16.psf", 256),
        ]
    );
    assert_eq!(rust.warnings, Vec::<String>::new());
    let licence = format!("{FONTS}/LICENSE.spleen");
    assert!(rust.exists(&licence), "{:?}", rust.fonts);

    // The regular face alone, and fallback fonts in the order given; one
    // that is missing is left out. Tests run in the crate's directory.
    let mut fallback = rust_settings.clone();
    fallback.push(("zfsbootmenu_console_faces", "regular"));
    let fonts = "../../legacy/zfsbootmenu/fonts/dejavu/DejaVuSansMono-Bold.ttf /nonexistent/a.ttf";
    fallback.push(("zfsbootmenu_console_fallback_fonts", fonts));
    let image = dracut_install(&mut parity, &fallback);
    let ttf: Vec<&String> = image
        .fonts
        .keys()
        .filter(|name| {
            std::path::Path::new(name)
                .extension()
                .is_some_and(|extension| extension == "ttf")
        })
        .collect();
    assert_eq!(ttf, ["DejaVuSansMono.ttf"]);
    assert!(image.exists(&format!("{FONTS}/fallback/01-DejaVuSansMono-Bold.ttf")));
    assert_eq!(
        image.warnings,
        ["fallback font /nonexistent/a.ttf not found; leaving it out"]
    );

    // The legacy runtime loads Terminus by name: it keeps them, and says so.
    let legacy = dracut_install(&mut parity, &settings);
    assert_eq!(legacy.fonts.len(), 6, "{:?}", legacy.fonts);
    assert!(legacy.fonts.keys().all(|name| name.starts_with("ter-v")));
    assert_eq!(
        legacy.warnings,
        ["Spleen console fonts need zfsbootmenu_runtime=rust; installing Terminus"]
    );

    // Another family is Terminus too.
    rust_settings[1] = ("zfsbootmenu_console_font", "comic");
    let other = dracut_install(&mut parity, &rust_settings);
    let psf = other.fonts.keys().filter(|name| {
        std::path::Path::new(name)
            .extension()
            .is_some_and(|extension| extension == "psf")
    });
    assert!(psf.clone().count() == 6 && psf.clone().all(|name| name.starts_with("ter-v")));
    assert_eq!(
        other.warnings,
        ["unknown zfsbootmenu_console_font 'comic'; installing Terminus"]
    );
}
