//! The build options of the fonts of the console on the display.

use super::super::{Doc, OptionSpec, Reader, Source, Value};

/// `zfsbootmenu_console_font`: the console font family of the image.
pub static CONSOLE_FONT: OptionSpec = OptionSpec {
    name: "zfsbootmenu_console_font",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Choice(&["terminus", "spleen"]),
    default: Some("terminus"),
    summary: "The console font family of the image: terminus or spleen",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_console_font=terminus**",
        body: CONSOLE_FONT_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_console_font=spleen"],
};

const CONSOLE_FONT_DOC: &str = r"
Select the console fonts of the image: *terminus*, the default, or *spleen*, Spleen 2.2.0 in cells
of 8x16, 12x24, 16x32 and 32x64, with its licence as
*/usr/share/zfsbootmenu/fonts/LICENSE.spleen*. Only the *rust* runtime loads Spleen: an image with
the *legacy* runtime warns and keeps Terminus, as it does for any other value. **generate-zbm**
sets this variable from *Global.ConsoleFont* when its configuration has that key.
";

/// `zfsbootmenu_console_faces`: the faces of the TrueType font in images
/// with the Rust runtime.
pub static CONSOLE_FACES: OptionSpec = OptionSpec {
    name: "zfsbootmenu_console_faces",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Choice(&["all", "regular"]),
    default: Some("all"),
    summary: "The faces of the console's TrueType font: all, or regular alone",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_console_faces=all**",
        body: CONSOLE_FACES_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_console_faces=regular"],
};

const CONSOLE_FACES_DOC: &str = r"
Select the faces of DejaVu Sans Mono that images with the *rust* runtime include for the console on
the display (**zbm.console=drm**): *all*, the default, adds the bold, oblique and bold oblique faces
to the regular one, so that the console draws bold and italic text as programs ask; *regular* leaves
them out, which makes the image about 840 KB smaller, and the console draws all text upright, bold
text bright.
";

/// `zfsbootmenu_console_fallback_fonts`: fonts for the characters the
/// console's font lacks.
pub static CONSOLE_FALLBACK_FONTS: OptionSpec = OptionSpec {
    name: "zfsbootmenu_console_fallback_fonts",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Text,
    default: None,
    summary: "TrueType or OpenType fonts for the characters the console's font lacks",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_console_fallback_fonts=\"<path> ...\"**",
        body: CONSOLE_FALLBACK_FONTS_DOC,
    },
    deprecation: None,
    examples: &[r#"zfsbootmenu_console_fallback_fonts="/usr/share/fonts/noto/NotoSansCJK.ttc""#],
};

const CONSOLE_FALLBACK_FONTS_DOC: &str = r"
List TrueType or OpenType fonts of the build host, separated by spaces, that the console on the
display of images with the *rust* runtime draws the characters with that DejaVu Sans Mono lacks,
such as Chinese, Japanese or Korean ones: the console tries them in the order given, and draws a
replacement character only when none has the character. They are installed in
*/usr/share/zfsbootmenu/fonts/fallback/*; a font that is missing is left out with a warning.
";
