//! Options of the kernel command line for the console: its font,
//! documented in `zfsbootmenu(7)` under `zbm.autosize`, and the console on
//! the display, under `zbm.console`.

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

/// `zbm.console`: which console the runtime runs on: the kernel's, or its
/// own on the display (ROADMAP.md #19).
pub static DISPLAY_CONSOLE: OptionSpec = OptionSpec {
    name: "zbm.console",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Choice(&["kernel", "drm"]),
    default: Some("kernel"),
    summary: "Which console the runtime runs on: the kernel's, or its own on the display",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.console=drm**",
        body: DISPLAY_CONSOLE_DOC,
    },
    deprecation: None,
    examples: &["zbm.console=drm", "zbm.console=kernel"],
};

const DISPLAY_CONSOLE_DOC: &str = r#"
In images built with the Rust runtime (*zfsbootmenu_runtime=rust*), run ZFSBootMenu and its
recovery shell on a console of its own, which draws on the display through the kernel's DRM drivers
(*simpledrm* on the firmware's framebuffer is enough) with the image's console fonts, at the size
**zbm.autosize** chooses, and reads the keyboards itself. It shows 256 colours and true colour, and
keeps nothing that scrolls off the screen. Before booting, it hands the display and the keyboards
back to the kernel. When the control terminal is not a virtual console, when the console cannot
use the display, or when it fails, ZFSBootMenu goes on on the kernel console. Keys are read from
the keyboards present when it starts. The image needs the kernel's *evdev* module
and the display's DRM driver, for instance with *add_drivers+=" evdev virtio_gpu "* in its dracut
configuration. The default, **zbm.console=kernel**, keeps the kernel console.

**zbm.console_font=psf**

  The console draws with the image's TrueType font, DejaVu Sans Mono, which covers Latin, Greek and
  Cyrillic, in cells as high as the console font **zbm.autosize** chooses. With **psf**, it draws
  with that console font itself, as the kernel console would.

**zbm.keyboard_layout=de**

  The console translates keys with the first of the image's keyboard layouts
  (*zfsbootmenu_keyboard_layouts*), dead keys included, or with the one this option names. A layout
  the image lacks leaves the first, and an image without layouts reads keys on the US layout.
  **zbm console layout** *name* switches layouts while the console runs, and Super+Space goes to the
  image's next layout.

**zbm.numlock=on**

  The console turns Num Lock on (*on*) or off (*off*) when it starts; otherwise it starts as the
  keyboards' light shows it. Scroll Lock holds the programs' output, as on the kernel console, until
  it is pressed again.

**zbm.keyboard_repeat=<delay>,<rate>**

  A key held repeats after *delay* milliseconds, *rate* times a second, on every keyboard; otherwise
  the keyboards keep the kernel's repeat.

**zbm.keyboard_compose=menu**

  The compose key: **ralt** (right Alt), **rwin** (the right logo key), **menu**, **rctrl** (right
  Ctrl) or **caps** (Caps Lock). It starts the sequences of the compose rules of the image's
  locale, as Compose a e for æ, and types nothing itself; without this option there is none.

**zbm.keyboard_layouts=046d:c31c=de,1a2c:0e24=fr**

  Layouts of their own for USB keyboards, each named by its vendor and product as **lsusb** shows
  them. Such a keyboard keeps its own Caps Lock, Num Lock and lights; the others type with the
  layout of **zbm.keyboard_layout**, which Super+Space changes.

**zbm.console_scrollback=2000**

  The console keeps this many lines that scrolled off the screen, which Shift+PgUp and Shift+PgDn
  show; 0 keeps none. While a passphrase or PIN prompt is open, it keeps none, and the lines it had
  are overwritten. The menu tells the console about its prompts; a program that reads a line
  without echoing it, such as **cryptsetup** or **zluks**, is taken for one. Typing shows the screen
  again.

  Ctrl+Shift+Space starts vi mode: the keys move a cursor over the screen and the scrollback as in
  vi (h, j, k, l, w, b, e, 0, $, g, G, H, M, L, Ctrl+U, Ctrl+D, PgUp, PgDn), and none reach the
  programs; Escape or q ends it. Ctrl+Shift+F and Ctrl+Shift+B, or / and ? in vi mode, search down
  and up for the text typed, shown on the last row, and Enter keeps the match; n and N find the next
  match and the one before. Ctrl+Shift+S saves the scrollback in
  ``/zfsbootmenu/console-scrollback.txt``, which **zbm report** adds to its report. None of these
  keys is the console's at a secret prompt or in a full-screen program.

**zbm.console_palette=linux**

  The colour scheme: **linux**, the Linux console's grey on black; **high-contrast**, white on black
  with bright colours; **solarized-dark**; **solarized-light**; or **tango**.

**zbm.console_large=on**

  Large text in high contrast: cells twice as high as they would be, or half again as high when that
  leaves fewer than 40 columns, in the **high-contrast** scheme unless **zbm.console_palette** names
  another.

**zbm.console_status=on**

  A status line under the programs' screen: the title they set, the keyboard layout, the locks that
  are on, the time in UTC and the battery's charge on a laptop.

**zbm.console_bell=visual**

  What the bell does: the screen's border flashes (**visual**), the PC speaker sounds (**audible**),
  **both**, or nothing (**none**). The speaker needs the **pcspkr** module in the image.

**zbm.console_mode=1024x768**

  The console shows its display in a mode of this size when the display has one, as **video=**
  chooses the kernel console's; the mode the display shows already otherwise.

**zbm.console_blank=10**

  After this many minutes without a key, the screen goes black and the display's power off, as
  **setterm -blank** and **-powerdown** do; a key turns it on again, and reaches the programs. The
  screen does not blank by default.

Print Screen saves the screen's text and image in ``/zfsbootmenu/screenshot-N.txt`` and
``screenshot-N.png``, which **zbm report** adds to its report; there is none at a secret prompt.

Ctrl+Alt+= and Ctrl+Alt+- make the text larger and smaller while the console runs, and Ctrl+Alt+0
gives it its first size again; the programs are told the new size. **zbm console font** says which
font the console draws with, and **zbm console font** *pixels*, **+**, **-** or **default** asks for
a size.

Ctrl+Shift+U, then a code point in hexadecimal and Space or Enter, types any character; Escape
cancels it. While a dead key or a code point waits for its next key, the console shows it at the
cursor, except at a prompt that does not echo, such as a passphrase prompt.
"#;

/// `zbm.console_font`: which fonts the console on the display draws with.
pub static DISPLAY_CONSOLE_FONT: OptionSpec = OptionSpec {
    name: "zbm.console_font",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Choice(&["ttf", "psf"]),
    default: Some("ttf"),
    summary: "The fonts of the console on the display: TrueType or console fonts",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.console"),
    deprecation: None,
    examples: &["zbm.console_font=psf"],
};

/// `zbm.keyboard_layout`: the keyboard layout of the console on the
/// display.
pub static KEYBOARD_LAYOUT: OptionSpec = OptionSpec {
    name: "zbm.keyboard_layout",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "The keyboard layout of the console on the display",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.console"),
    deprecation: None,
    examples: &[
        "zbm.keyboard_layout=de",
        "zbm.keyboard_layout=de(nodeadkeys)",
    ],
};

/// `zbm.numlock`: Num Lock when the console on the display starts.
pub static NUM_LOCK: OptionSpec = OptionSpec {
    name: "zbm.numlock",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Choice(&["on", "off"]),
    default: None,
    summary: "Num Lock when the console on the display starts: on or off",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.console"),
    deprecation: None,
    examples: &["zbm.numlock=on"],
};

/// `zbm.keyboard_repeat`: the key repeat of the console on the display.
pub static KEYBOARD_REPEAT: OptionSpec = OptionSpec {
    name: "zbm.keyboard_repeat",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "The key repeat of the console on the display: delay (ms) and rate a second",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.console"),
    deprecation: None,
    examples: &["zbm.keyboard_repeat=250,30"],
};

/// `zbm.console_scrollback`: the lines of history of the console on the
/// display.
pub static CONSOLE_SCROLLBACK: OptionSpec = OptionSpec {
    name: "zbm.console_scrollback",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Integer,
    default: Some("2000"),
    summary: "The lines of history the console on the display keeps for Shift+PgUp",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.console"),
    deprecation: None,
    examples: &["zbm.console_scrollback=0", "zbm.console_scrollback=10000"],
};

/// The most lines of history `zbm.console_scrollback` may ask for.
pub const MAX_SCROLLBACK: usize = 100_000;

/// The colour schemes of the console on the display.
pub const PALETTES: &[&str] = &[
    "linux",
    "high-contrast",
    "solarized-dark",
    "solarized-light",
    "tango",
];

/// `zbm.console_palette`: the colour scheme of the console on the display.
pub static CONSOLE_PALETTE: OptionSpec = OptionSpec {
    name: "zbm.console_palette",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Choice(PALETTES),
    default: Some("linux"),
    summary: "The colour scheme of the console on the display",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.console"),
    deprecation: None,
    examples: &["zbm.console_palette=solarized-dark"],
};

/// `zbm.console_large`: large text and high contrast on the console on
/// the display.
pub static CONSOLE_LARGE: OptionSpec = OptionSpec {
    name: "zbm.console_large",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Choice(&["on", "off"]),
    default: Some("off"),
    summary: "Large text in high contrast on the console on the display",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.console"),
    deprecation: None,
    examples: &["zbm.console_large=on"],
};

/// `zbm.console_status`: the status line of the console on the display.
pub static CONSOLE_STATUS: OptionSpec = OptionSpec {
    name: "zbm.console_status",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Choice(&["on", "off"]),
    default: Some("off"),
    summary: "A status line under the console on the display",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.console"),
    deprecation: None,
    examples: &["zbm.console_status=on"],
};

/// The keys that can be the compose key.
pub const COMPOSE_KEYS: &[&str] = &["ralt", "rwin", "menu", "rctrl", "caps"];

/// `zbm.keyboard_compose`: the compose key of the console on the display.
pub static KEYBOARD_COMPOSE: OptionSpec = OptionSpec {
    name: "zbm.keyboard_compose",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Choice(COMPOSE_KEYS),
    default: None,
    summary: "The compose key of the console on the display",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.console"),
    deprecation: None,
    examples: &["zbm.keyboard_compose=menu"],
};

/// `zbm.keyboard_layouts`: layouts of their own for some keyboards.
pub static KEYBOARD_LAYOUTS: OptionSpec = OptionSpec {
    name: "zbm.keyboard_layouts",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "Layouts of their own for USB keyboards, by vendor and product",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.console"),
    deprecation: None,
    examples: &["zbm.keyboard_layouts=046d:c31c=de,1a2c:0e24=fr"],
};

/// Whether `name` is a keyboard layout's name as the console on the
/// display takes it, in `zbm.keyboard_layouts`: a layout and perhaps a
/// variant in parentheses (`de`, `de(nodeadkeys)`), each of ASCII letters,
/// digits, `_` and `-`.
///
/// ```
/// use zbm_config::spec::cmdline::is_layout_name;
///
/// assert!(is_layout_name("de(nodeadkeys)"));
/// assert!(!is_layout_name("de("));
/// ```
pub fn is_layout_name(name: &str) -> bool {
    let is_word = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    };
    match name.split_once('(') {
        Some((layout, rest)) => is_word(layout) && rest.strip_suffix(')').is_some_and(is_word),
        None => is_word(name),
    }
}

/// What the bell of the console on the display can do.
pub const BELLS: &[&str] = &["none", "visual", "audible", "both"];

/// `zbm.console_bell`: the bell of the console on the display.
pub static CONSOLE_BELL: OptionSpec = OptionSpec {
    name: "zbm.console_bell",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Choice(BELLS),
    default: Some("visual"),
    summary: "What the bell of the console on the display does",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.console"),
    deprecation: None,
    examples: &["zbm.console_bell=audible"],
};

/// `zbm.console_mode`: the size of the display's mode, as `1024x768`.
pub static CONSOLE_MODE: OptionSpec = OptionSpec {
    name: "zbm.console_mode",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "The size of the mode the console on the display shows",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.console"),
    deprecation: None,
    examples: &["zbm.console_mode=1024x768"],
};

/// `zbm.console_blank`: the minutes without a key after which the console
/// on the display blanks.
pub static CONSOLE_BLANK: OptionSpec = OptionSpec {
    name: "zbm.console_blank",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Integer,
    default: None,
    summary: "The minutes without a key after which the console on the display blanks",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.console"),
    deprecation: None,
    examples: &["zbm.console_blank=10"],
};

/// The width and height `value` (`1024x768`) names, when both are numbers
/// greater than 0.
///
/// ```
/// use zbm_config::spec::cmdline::console::mode_size;
///
/// assert_eq!(mode_size("1024x768"), Some((1024, 768)));
/// assert_eq!(mode_size("1024"), None);
/// assert_eq!(mode_size("0x768"), None);
/// ```
pub fn mode_size(value: &str) -> Option<(u16, u16)> {
    let (width, height) = value.trim().split_once('x')?;
    let (width, height): (u16, u16) = (width.parse().ok()?, height.parse().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

/// The most minutes `zbm.console_blank` takes: a day.
pub const MAX_BLANK_MINUTES: u32 = 24 * 60;

/// `zbm.lines`: the number of lines of the console.
pub static LINES: OptionSpec = OptionSpec {
    name: "zbm.lines",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Integer,
    default: None,
    summary: "The number of lines of the console",
    reader: Reader::Rust,
    doc: Doc::NotInManual("undocumented; it sets the size of the console when the kernel cannot"),
    deprecation: None,
    examples: &["zbm.lines=40"],
};

/// `zbm.columns`: the number of columns of the console.
pub static COLUMNS: OptionSpec = OptionSpec {
    name: "zbm.columns",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Integer,
    default: None,
    summary: "The number of columns of the console",
    reader: Reader::Rust,
    doc: Doc::NotInManual("undocumented; it sets the size of the console when the kernel cannot"),
    deprecation: None,
    examples: &["zbm.columns=120"],
};

/// `zbm.console_mirror`: where the console on the display writes the text
/// of its screen, for the VM tests; only builds with the `console-mirror`
/// feature take it, as the copy holds what prompts show.
pub static CONSOLE_MIRROR: OptionSpec = OptionSpec {
    name: "zbm.console_mirror",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "Where the console on the display writes the text of its screen",
    reader: Reader::Rust,
    doc: Doc::NotInManual("for the VM tests' builds, which read the screen on a serial port"),
    deprecation: None,
    examples: &["zbm.console_mirror=/dev/ttyS1"],
};
