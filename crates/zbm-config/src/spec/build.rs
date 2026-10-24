//! Options of the dracut module and the mkinitcpio hook that build
//! ZFSBootMenu images, documented in `zfsbootmenu(7)`.

use super::{Deprecation, Doc, OptionSpec, Reader, Source, Value};

/// The options, in the order of the manual page.
pub static ALL: &[&OptionSpec] = &[
    &MODULE_ROOT,
    &HOOK_ROOT,
    &RUNTIME,
    &ZBM,
    &CONSOLE_FONT,
    &KEYBOARD_LAYOUTS,
    &SKIP_GCC_S,
    &LUKS,
    &LUKS_CRYPTTAB,
    &LUKS_TOKENS,
    &LUKS_TOKEN_PLUGINS,
    &LUKS_CLEVIS,
    &LUKS_GPG,
    &LUKS_GPG_KEYS,
    &LUKS_GPG_PUBKEYS,
    &LUKS_PCSCD,
    &LUKS_YUBIKEY,
    &MISER,
    &EARLY_SETUP,
    &SETUP,
    &TEARDOWN,
];

/// `zfsbootmenu_module_root`: where the core components of the image are.
pub static MODULE_ROOT: OptionSpec = OptionSpec {
    name: "zfsbootmenu_module_root",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Text,
    default: Some("/usr/share/zfsbootmenu"),
    summary: "Where the core components of the image are",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_module_root=<path>**",
        body: MODULE_ROOT_DOC,
    },
    deprecation: None,
    examples: &[r#"zfsbootmenu_module_root="/usr/local/share/zfsbootmenu""#],
};

const MODULE_ROOT_DOC: &str = r"
Set this variable to override the default **<path>** where the ZFSBootMenu module expects to find
core components that must be installed in the created image. When unspecified, a default of
*/usr/share/zfsbootmenu* is assumed.
";

/// `zfsbootmenu_hook_root`: where the user hooks for the image are.
pub static HOOK_ROOT: OptionSpec = OptionSpec {
    name: "zfsbootmenu_hook_root",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Text,
    default: Some("/etc/zfsbootmenu/hooks"),
    summary: "Where the user hooks for the image are",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_hook_root=<path>**",
        body: HOOK_ROOT_DOC,
    },
    deprecation: None,
    examples: &[r#"zfsbootmenu_hook_root="/etc/zfsbootmenu/hooks""#],
};

const HOOK_ROOT_DOC: &str = r"
Set this variable to override the default **<path>** where the ZFSBootMenu module expects to find
optional user hooks that will be installed in the created image. When unspecified, a default of
*/etc/zfsbootmenu/hooks* is assumed.
";

/// `zfsbootmenu_runtime`: the runtime of the image: legacy (shell) or rust.
pub static RUNTIME: OptionSpec = OptionSpec {
    name: "zfsbootmenu_runtime",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Choice(&["legacy", "rust"]),
    default: Some("legacy"),
    summary: "The runtime of the image: legacy (shell) or rust",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_runtime=legacy**",
        body: RUNTIME_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_runtime=rust"],
};

const RUNTIME_DOC: &str = r"
Select the runtime that the image boots with: *legacy*, the shell implementation, which is the
default, or *rust*, the Rust runtime that is being written to replace it. The Rust runtime needs
**zbm** (see **zfsbootmenu_zbm**). So far it unlocks LUKS volumes, with every key method and
**zluks**, imports pools, with every import option, and boots the default boot environment, as the
legacy runtime does with **zbm.skip**, loading the key of its encryption root with the
**load-key.d** hooks, a key file in the image or the passphrase prompt; where a machine needs the
menu, it logs that it cannot show it yet and opens the recovery shell.
It keeps secrets in memory that is locked, left out of core dumps and, where the kernel allows it
(*memfd_secret*), out of the kernel's own map of memory; boot it with *init_on_free=1* (in
**Kernel.CommandLine** of **generate-zbm**) so that the kernel also zeroes the memory it frees,
which **zbm report** shows. Any other value stops the image build with an error.
";

/// `zfsbootmenu_zbm`: the zbm program of the Rust runtime.
pub static ZBM: OptionSpec = OptionSpec {
    name: "zfsbootmenu_zbm",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Text,
    default: Some("/usr/libexec/zfsbootmenu/zbm"),
    summary: "The zbm program of the Rust runtime",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_zbm=<path>**",
        body: ZBM_DOC,
    },
    deprecation: None,
    examples: &[r#"zfsbootmenu_zbm="/usr/local/libexec/zfsbootmenu/zbm""#],
};

const ZBM_DOC: &str = r"
Set this variable to override the default **<path>** of **zbm**, the program of the Rust runtime,
which is installed in the image as */libexec/zbm*. When unspecified, a default of
*/usr/libexec/zfsbootmenu/zbm* is assumed, which is where **make install** puts it. The *legacy*
runtime does not use **zbm**; when it is not found, the image is built without it. The *rust*
runtime cannot be built without it.
";

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

/// `zfsbootmenu_keyboard_layouts`: the keyboard layouts of the console on
/// the display.
pub static KEYBOARD_LAYOUTS: OptionSpec = OptionSpec {
    name: "zfsbootmenu_keyboard_layouts",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Text,
    default: None,
    summary: "The keyboard layouts of the console on the display, its own first",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_keyboard_layouts=\"de us\"**",
        body: KEYBOARD_LAYOUTS_DOC,
    },
    deprecation: None,
    examples: &[
        "zfsbootmenu_keyboard_layouts=\"de us\"",
        "zfsbootmenu_keyboard_layouts=\"de(nodeadkeys) fr\"",
    ],
};

const KEYBOARD_LAYOUTS_DOC: &str = r"
In images with the *rust* runtime, the keyboard layouts of the console on the display
(**zbm.console=drm**), as XKB names separated by spaces or commas: a layout, such as *de*, or a
layout and its variant, such as *de(nodeadkeys)*. The first is the console's own unless
**zbm.keyboard_layout** chooses another. **zbm keyboard-layouts** compiles each from the XKB data of
the machine that builds the image (*/usr/share/X11/xkb*, from *xkeyboard-config*) into
*/usr/share/zfsbootmenu/layouts*, about 70 KB each, with the dead keys of the *en_US.UTF-8* compose
rules (*/usr/share/X11/locale*, from *libX11*); the build warns about layouts it does not know and
leaves them out. When unspecified, the image gets the layout of the build host's
*/etc/vconsole.conf* (*XKBLAYOUT*), then *us*, *de*, *fr* and *gb*.
";

/// `zfsbootmenu_skip_gcc_s`: leaves `libgcc_s.so` out of the image.
pub static SKIP_GCC_S: OptionSpec = OptionSpec {
    name: "zfsbootmenu_skip_gcc_s",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Bool,
    default: None,
    summary: "Leaves libgcc_s.so out of the image",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_skip_gcc_s=yes**",
        body: SKIP_GCC_S_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_skip_gcc_s=yes"],
};

const SKIP_GCC_S_DOC: &str = r"
The ZFSBootMenu module attempts to detect and install a copy of the library **libgcc_s.so** in its
initramfs image on glibc systems. Because several executables may have latent dependencies on this
library via a **dlopen** call in glibc itself, a failure to detect and install the library will
cause initramfs generation to fail. If the host system has no dependencies on **libgcc_s.so**, set
**zfsbootmenu_skip_gcc_s=yes** to avoid this failure. Alternatively, if **libgcc_s.so** is present
in an undetected location, set this option and configure dracut or mkinitcpio to explicitly
install the library.
";

/// `zfsbootmenu_luks`: whether the image can unlock LUKS volumes.
pub static LUKS: OptionSpec = OptionSpec {
    name: "zfsbootmenu_luks",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Bool,
    default: Some("yes"),
    summary: "Whether the image can unlock LUKS volumes",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_luks=no**",
        body: LUKS_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_luks=no"],
};

const LUKS_DOC: &str = r"
Omit support for ZFS pools on LUKS volumes. By default, when **cryptsetup** is installed on the
build host, the image includes **cryptsetup**, **dmsetup**, device-mapper udev rules and the
dm-crypt and common kernel crypto modules.
";

/// `zfsbootmenu_luks_crypttab`: the crypttab to embed in the image.
pub static LUKS_CRYPTTAB: OptionSpec = OptionSpec {
    name: "zfsbootmenu_luks_crypttab",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Text,
    default: Some("/etc/zfsbootmenu/crypttab"),
    summary: "The crypttab to embed in the image",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_luks_crypttab=<path>**",
        body: LUKS_CRYPTTAB_DOC,
    },
    deprecation: None,
    examples: &[r#"zfsbootmenu_luks_crypttab="/etc/zfsbootmenu/crypttab""#],
};

const LUKS_CRYPTTAB_DOC: &str = r"
A crypttab(5)-style file that lists the LUKS volumes to unlock, with optional key files and
options (default: */etc/zfsbootmenu/crypttab*). It is embedded in the image as
*/etc/zfsbootmenu/crypttab*, together with any **keyscript=** programs it references. Key files
are never embedded. The file is ignored for release builds.
";

/// `zfsbootmenu_luks_tokens`: whether the image supports LUKS2 tokens.
pub static LUKS_TOKENS: OptionSpec = OptionSpec {
    name: "zfsbootmenu_luks_tokens",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Bool,
    default: Some("yes"),
    summary: "Whether the image supports LUKS2 tokens",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_luks_tokens=no**",
        body: LUKS_TOKENS_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_luks_tokens=no"],
};

const LUKS_TOKENS_DOC: &str = r"
Omit support for LUKS2 tokens (TPM2, FIDO2 and clevis). By default, the systemd **systemd-tpm2**
and **systemd-fido2** cryptsetup plugins, **tpm2-tools**, **openssl**, **fido2-assert**,
**fido2-token** and the clevis tools are installed when present on the build host, together with
TPM, USB and HID drivers.
";

/// `zfsbootmenu_luks_token_plugins`.
pub static LUKS_TOKEN_PLUGINS: OptionSpec = OptionSpec {
    name: "zfsbootmenu_luks_token_plugins",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Bool,
    default: Some("yes"),
    summary: "Whether the image has systemd's cryptsetup token plugins",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_luks_token_plugins=no**",
        body: LUKS_TOKEN_PLUGINS_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_luks_token_plugins=no"],
};

const LUKS_TOKEN_PLUGINS_DOC: &str = r"
Omit the systemd cryptsetup token plugins; ZFSBootMenu then unlocks systemd TPM2 and FIDO2 tokens
with its own implementation.
";

/// `zfsbootmenu_luks_clevis`: whether the image has clevis.
pub static LUKS_CLEVIS: OptionSpec = OptionSpec {
    name: "zfsbootmenu_luks_clevis",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Bool,
    default: Some("yes"),
    summary: "Whether the image has clevis",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_luks_clevis=no**",
        body: LUKS_CLEVIS_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_luks_clevis=no"],
};

const LUKS_CLEVIS_DOC: &str = r"
Omit clevis even when it is installed.
";

/// `zfsbootmenu_luks_gpg`: whether the image has GnuPG.
pub static LUKS_GPG: OptionSpec = OptionSpec {
    name: "zfsbootmenu_luks_gpg",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Bool,
    default: Some("yes"),
    summary: "Whether the image has GnuPG",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_luks_gpg=no**",
        body: LUKS_GPG_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_luks_gpg=no"],
};

const LUKS_GPG_DOC: &str = r"
Omit GnuPG. By default, **gpg**, **gpg-agent** and **scdaemon** are installed when present, so
that GnuPG-encrypted LUKS key files can be decrypted with a passphrase or an OpenPGP smart card
(YubiKey, Nitrokey, ...). GnuPG-encrypted key files listed in the crypttab are embedded in the
image.
";

/// `zfsbootmenu_luks_gpg_keys`.
pub static LUKS_GPG_KEYS: OptionSpec = OptionSpec {
    name: "zfsbootmenu_luks_gpg_keys",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Text,
    default: None,
    summary: "OpenPGP public keys to copy from the host's keyring",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: r#"**zfsbootmenu_luks_gpg_keys="<key id> ..."**"#,
        body: LUKS_GPG_KEYS_DOC,
    },
    deprecation: None,
    examples: &[r#"zfsbootmenu_luks_gpg_keys="0x1234ABCD""#],
};

const LUKS_GPG_KEYS_DOC: &str = r"
OpenPGP public keys to export from the build host's keyring into the image.
";

/// `zfsbootmenu_luks_gpg_pubkeys`: files of OpenPGP public keys to import.
pub static LUKS_GPG_PUBKEYS: OptionSpec = OptionSpec {
    name: "zfsbootmenu_luks_gpg_pubkeys",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Text,
    default: None,
    summary: "Files of OpenPGP public keys to import",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: r#"**zfsbootmenu_luks_gpg_pubkeys="<file> ..."**"#,
        body: LUKS_GPG_PUBKEYS_DOC,
    },
    deprecation: None,
    examples: &[r#"zfsbootmenu_luks_gpg_pubkeys="/etc/zfsbootmenu/keys.asc""#],
};

const LUKS_GPG_PUBKEYS_DOC: &str = r"
Files with OpenPGP public keys to import into the image.
";

/// `zfsbootmenu_luks_pcscd`: whether the image has pcscd.
pub static LUKS_PCSCD: OptionSpec = OptionSpec {
    name: "zfsbootmenu_luks_pcscd",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Bool,
    default: Some("no"),
    summary: "Whether the image has pcscd",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_luks_pcscd=yes**",
        body: LUKS_PCSCD_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_luks_pcscd=yes"],
};

const LUKS_PCSCD_DOC: &str = r"
Include **pcscd** and its card reader drivers, for smart card readers that GnuPG's built-in CCID
driver does not support.
";

/// `zfsbootmenu_luks_yubikey`: whether the image has ykchalresp.
pub static LUKS_YUBIKEY: OptionSpec = OptionSpec {
    name: "zfsbootmenu_luks_yubikey",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Bool,
    default: Some("yes"),
    summary: "Whether the image has ykchalresp",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_luks_yubikey=no**",
        body: LUKS_YUBIKEY_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_luks_yubikey=no"],
};

const LUKS_YUBIKEY_DOC: &str = r"
Omit **ykchalresp**, used for YubiKey challenge-response unlocking.
";

/// `zfsbootmenu_miser`: keeps busybox's utilities instead of the system's.
pub static MISER: OptionSpec = OptionSpec {
    name: "zfsbootmenu_miser",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Bool,
    default: Some("no"),
    summary: "Keeps busybox's utilities instead of the system's",
    reader: Reader::Shell("the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_miser=yes** (mkinitcpio only)",
        body: MISER_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_miser=yes"],
};

const MISER_DOC: &str = r"
By default, **mkinitcpio** uses busybox to populate initramfs images. However, the *zfsbootmenu*
hook will install system versions of several utilities that it requires to operate. On most
systems, these versions will be provided by util-linux rather than busybox. To prefer busybox for
these utilities when possible, set **zfsbootmenu_miser=yes**. Synonyms for *yes* are *1*, *y* or
*on*, without regard to letter case.
";

/// `zfsbootmenu_early_setup`: early-setup hooks to install.
pub static EARLY_SETUP: OptionSpec = OptionSpec {
    name: "zfsbootmenu_early_setup",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Text,
    default: None,
    summary: "Early-setup hooks to install",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Within("zfsbootmenu_hook_root"),
    deprecation: Some(Deprecation {
        heading: "**zfsbootmenu_early_setup=<executable-list>**",
        instead: concat!(
            "place early-setup hooks in the directory ",
            "*${zfsbootmenu_hook_root}/early-setup.d*"
        ),
    }),
    examples: &[r#"zfsbootmenu_early_setup="/etc/zfsbootmenu/early.sh""#],
};

/// `zfsbootmenu_setup`: setup hooks to install.
pub static SETUP: OptionSpec = OptionSpec {
    name: "zfsbootmenu_setup",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Text,
    default: None,
    summary: "Setup hooks to install",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Within("zfsbootmenu_hook_root"),
    deprecation: Some(Deprecation {
        heading: "**zfsbootmenu_setup=<executable-list>**",
        instead: "place setup hooks in the directory *${zfsbootmenu_hook_root}/setup.d*",
    }),
    examples: &[r#"zfsbootmenu_setup="/etc/zfsbootmenu/setup.sh""#],
};

/// `zfsbootmenu_teardown`: teardown hooks to install.
pub static TEARDOWN: OptionSpec = OptionSpec {
    name: "zfsbootmenu_teardown",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Text,
    default: None,
    summary: "Teardown hooks to install",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Within("zfsbootmenu_hook_root"),
    deprecation: Some(Deprecation {
        heading: "**zfsbootmenu_teardown=<executable-list>**",
        instead: "place teardown hooks in the directory *${zfsbootmenu_hook_root}/teardown.d*",
    }),
    examples: &[r#"zfsbootmenu_teardown="/etc/zfsbootmenu/teardown.sh""#],
};
