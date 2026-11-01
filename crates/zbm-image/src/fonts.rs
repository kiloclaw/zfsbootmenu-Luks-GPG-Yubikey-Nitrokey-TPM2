//! The console's fonts and the keyboard layouts.

use std::path::{Path, PathBuf};

use super::Image;
use crate::keymaps::compile_layouts;
use crate::psf::subsets;

/// Where images keep their fonts, beside dracut's i18n module's, which
/// would replace them.
const FONTS: &str = "/usr/share/zfsbootmenu/fonts";

/// The faces of the TrueType font of the console on the display.
const FACES: [&str; 4] = [
    "DejaVuSansMono.ttf",
    "DejaVuSansMono-Bold.ttf",
    "DejaVuSansMono-Oblique.ttf",
    "DejaVuSansMono-BoldOblique.ttf",
];

/// The directory of the console fonts of the family the settings choose
/// (`zfsbootmenu_console_font`): Terminus, the default, or Spleen, which
/// only the Rust runtime loads, with its licence.
fn family(image: &mut Image, fonts: PathBuf) -> PathBuf {
    let family = image
        .config
        .or("zfsbootmenu_console_font", "terminus")
        .to_owned();
    match family.to_ascii_lowercase().as_str() {
        "terminus" => fonts,
        "spleen" if !image.is_rust() => {
            image
                .plan
                .warn("Spleen console fonts need zfsbootmenu_runtime=rust; installing Terminus");
            fonts
        }
        "spleen" if image.host.is_dir(&fonts.join("spleen")) => {
            let spleen = fonts.join("spleen");
            let licence = spleen.join("LICENSE");
            if image.host.is_file(&licence) {
                image.install(&licence, &Path::new(FONTS).join("LICENSE.spleen"));
            }
            spleen
        }
        "spleen" => {
            image.plan.warn(format!(
                "no Spleen console fonts in {}; installing Terminus",
                fonts.display()
            ));
            fonts
        }
        _ => {
            image.plan.warn(format!(
                "unknown zfsbootmenu_console_font '{family}'; installing Terminus"
            ));
            fonts
        }
    }
}

/// Whether the module root has console fonts.
pub fn has_fonts(image: &Image) -> bool {
    image.host.is_dir(&image.module_root().join("fonts"))
}

/// Whether `name` is a PSF font's (`*.psf`), as the shell's pattern says.
fn is_psf(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension == "psf")
}

/// The console fonts (`*.psf`) of the module root; the Rust runtime's as
/// 256-glyph subsets, which leave the console all 16 colours, or whole
/// when they cannot be made. The Rust runtime's TrueType font for the
/// console on the display: `DejaVuSansMono`, its bold and oblique faces
/// unless `zfsbootmenu_console_faces=regular`, and the fallback fonts of
/// `zfsbootmenu_console_fallback_fonts`, numbered so that the console tries
/// them in order.
pub fn fonts(image: &mut Image) {
    if !has_fonts(image) {
        return;
    }
    let root = image.module_root().join("fonts");
    let fonts = family(image, root.clone());
    let made = if image.is_rust() {
        subsets(image.host.files, &fonts)
            .ok()
            .filter(|made| made.iter().any(|(name, ..)| is_psf(name)))
    } else {
        None
    };
    if let Some(made) = made {
        for (name, bytes, _) in made.into_iter().filter(|(name, ..)| is_psf(name)) {
            image.plan.write(Path::new(FONTS).join(name), 0o644, bytes);
        }
    } else {
        if image.is_rust() {
            image
                .plan
                .warn("unable to make 256-glyph console fonts; installing the whole fonts");
        }
        for name in image.host.names(&fonts) {
            let path = fonts.join(&name);
            if is_psf(&name) && image.host.is_file(&path) {
                image.install(&path, &Path::new(FONTS).join(&name));
            }
        }
    }
    if image.is_rust() {
        faces(image, &root.join("dejavu"));
    }
}

/// The TrueType faces of `dejavu` and the fallback fonts.
fn faces(image: &mut Image, dejavu: &Path) {
    if !image.host.is_file(&dejavu.join(FACES[0])) {
        return;
    }
    let all = image.config.or("zfsbootmenu_console_faces", "all") != "regular";
    let faces = if all { &FACES[..] } else { &FACES[..1] };
    for face in faces {
        let src = dejavu.join(face);
        if image.host.is_file(&src) {
            image.install(&src, &Path::new(FONTS).join(face));
        }
    }
    image.install(
        &dejavu.join("LICENSE"),
        &Path::new(FONTS).join("LICENSE.dejavu"),
    );
    let mut order = 0;
    for font in image.config.words("zfsbootmenu_console_fallback_fonts") {
        let font = PathBuf::from(font);
        if !image.host.is_file(&font) {
            image.plan.warn(format!(
                "fallback font {} not found; leaving it out",
                font.display()
            ));
            continue;
        }
        order += 1;
        let name = font.file_name().unwrap_or_default().to_string_lossy();
        let dest = Path::new(FONTS).join(format!("fallback/{order:02}-{name}"));
        image.install(&font, &dest);
    }
}

/// The Rust runtime's keyboard layouts (`zfsbootmenu_keyboard_layouts`)
/// for the console on the display, compiled from the build host's XKB
/// data; the first is the console's own. Without the option, the layout of
/// the host's `/etc/vconsole.conf` (`XKBLAYOUT`) comes before us, de, fr
/// and gb.
pub fn layouts(image: &mut Image) {
    if !image.is_rust() {
        return;
    }
    let asked = image
        .config
        .get("zfsbootmenu_keyboard_layouts")
        .map_or_else(
            || {
                let vconsole = image
                    .host
                    .read(Path::new("/etc/vconsole.conf"))
                    .unwrap_or_default();
                format!("{} us de fr gb", host_layout(&vconsole))
            },
            str::to_owned,
        );
    let names: Vec<String> = asked
        .split([',', ' ', '\t'])
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    let host = image.host;
    let compiled = compile_layouts(host.files, &host.xkb, &host.compose, &names);
    for line in compiled.failed.iter().chain(&compiled.no_dead_keys) {
        image.plan.warn(format!("keyboard-layouts: {line}"));
    }
    if !compiled.failed.is_empty() {
        image.plan.warn(format!(
            "unable to compile every keyboard layout of '{}'",
            asked.trim()
        ));
    }
    for (name, contents) in compiled.files {
        let dest = Path::new("/usr/share/zfsbootmenu/layouts").join(name);
        image.plan.write(dest, 0o644, contents);
    }
}

/// The first layout of `XKBLAYOUT` in the text of `/etc/vconsole.conf`.
fn host_layout(vconsole: &str) -> &str {
    vconsole
        .lines()
        .find_map(|line| line.strip_prefix("XKBLAYOUT="))
        .map_or("", |value| {
            let value = value.strip_prefix('"').unwrap_or(value);
            value.split([',', '"']).next().unwrap_or_default()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hosts_layout_is_the_first_of_xkblayout() {
        assert_eq!(host_layout("KEYMAP=de\nXKBLAYOUT=\"de,us\"\n"), "de");
        assert_eq!(host_layout("XKBLAYOUT=fr\n"), "fr");
        assert_eq!(host_layout("KEYMAP=us\n"), "");
    }
}
