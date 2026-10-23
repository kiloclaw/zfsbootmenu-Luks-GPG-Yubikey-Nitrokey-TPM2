//! Options of the kernel command line for the console font, documented in
//! `zfsbootmenu(7)` under `zbm.autosize`.

use crate::spec::{Doc, OptionSpec, Reader, Source, Value};

/// `zbm.autosize`: whether the console font is resized to the display, or
/// the font height to use.
pub static AUTOSIZE: OptionSpec = OptionSpec {
    name: "zbm.autosize",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Bool,
    default: Some("1"),
    summary: "Whether the console font is resized to the display, or the height to use",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.autosize**",
        body: AUTOSIZE_DOC,
    },
    deprecation: None,
    examples: &["zbm.autosize=0", "zbm.autosize=48"],
};

const AUTOSIZE_DOC: &str = r"
Enable automatic font resizing of the kernel console to normalize the apparent resolution for both
low resolution and high resolution (HiDPI) displays. This option is enabled by default. It applies
to virtual consoles only, and does nothing when **rd.vconsole.font** names a font or the mkinitcpio
*consolefont* hook loaded one.

Images built with the Rust runtime (*zfsbootmenu_runtime=rust*) read the physical size of the
display from its EDID, when a kernel graphics driver provides it, and choose the font that makes
text about 4.5 millimetres tall while leaving at least 110 columns; fonts doubled by **setfont -d**
reach 64 pixels for the densest panels, where the kernel and **setfont** accept them. Without an
EDID, and in images with the legacy runtime, the largest font up to 32 pixels that leaves at least
110 columns is chosen.

**zbm.autosize=<pixels>**

  With the Rust runtime, use the font closest to this height in pixels, doubled if need be,
  whatever the size of the screen. The legacy runtime takes any number as *on*.
";

/// `rd.vconsole.font`: the console font dracut loads, which automatic
/// sizing leaves alone.
pub static VCONSOLE_FONT: OptionSpec = OptionSpec {
    name: "rd.vconsole.font",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "The console font dracut loads; zbm.autosize then leaves the font alone",
    reader: Reader::Other("dracut's i18n module"),
    doc: Doc::Within("zbm.autosize"),
    deprecation: None,
    examples: &["rd.vconsole.font=ter-v24b"],
};
