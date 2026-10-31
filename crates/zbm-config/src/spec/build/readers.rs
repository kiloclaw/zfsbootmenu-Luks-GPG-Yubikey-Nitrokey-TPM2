//! The build options of screen readers (ROADMAP.md UI8, ADR 0011).

use super::super::{Doc, OptionSpec, Reader, Source, Value};

/// `zfsbootmenu_brltty`: `brltty` in the image, for a Braille display.
pub static BRLTTY: OptionSpec = OptionSpec {
    name: "zfsbootmenu_brltty",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Bool,
    default: Some("no"),
    summary: "brltty in the image, for a Braille display",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_brltty=yes**",
        body: BRLTTY_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_brltty=yes"],
};

const BRLTTY_DOC: &str = r"
Puts the build host's **brltty** into the image, with its configuration (*/etc/brltty.conf*), its
drivers and tables, and the USB serial drivers Braille displays use, and starts it before anything
is asked. **brltty** reads the kernel console; in images with the *rust* runtime, the console on the
display (**zbm.console=drm**) writes its text where **brltty** reads it, the latest of what the menu
announces on its last row. Set the display's driver and device in */etc/brltty.conf* on the build
host. **brltty** then parses what the display sends, as root, before the menu: leave it out of
images whose users need none. The build fails when the host has no **brltty**.
";

/// `zfsbootmenu_speakup`: the `speakup` synthesiser of the image.
pub static SPEAKUP: OptionSpec = OptionSpec {
    name: "zfsbootmenu_speakup",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Text,
    default: None,
    summary: "The speakup synthesiser to load, for speech",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_speakup=<synth>**",
        body: SPEAKUP_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_speakup=soft", "zfsbootmenu_speakup=dectlk"],
};

const SPEAKUP_DOC: &str = r"
Puts the kernel's **speakup** screen reader into the image with the module of the
synthesiser **<synth>** (*speakup_<synth>*), and loads it before anything is asked; with *soft*,
**espeakup** too, when the build host has it. **speakup** speaks the kernel console, so keep it
(**zbm.console=kernel**, the default); with it loaded, images with the *rust* runtime show the
menu as lines (see **zbm.menu**). The build fails when the kernel has no such module.
";
