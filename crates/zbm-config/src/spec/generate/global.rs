//! The `Global` section of `config.yaml`: how `generate-zbm` builds images.

use crate::spec::{Doc, GenerateSection, OptionSpec, Reader, Source, Value};

/// `Global.ManageImages`: whether generate-zbm does anything.
pub static GLOBAL_MANAGE_IMAGES: OptionSpec = OptionSpec {
    name: "Global.ManageImages",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Global),
    value: Value::Bool,
    default: Some("false"),
    summary: "Whether generate-zbm does anything",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**ManageImages**",
        body: GLOBAL_MANAGE_IMAGES_DOC,
    },
    deprecation: None,
    examples: &["ManageImages: true"],
};

const GLOBAL_MANAGE_IMAGES_DOC: &str = r"
This must be set to *true* before **generate-zbm** will attempt to perform any action (e.g., image
creation or pruning old files).
";

/// `Global.InitCPIO`: builds images with mkinitcpio instead of dracut.
pub static GLOBAL_INIT_CPIO: OptionSpec = OptionSpec {
    name: "Global.InitCPIO",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Global),
    value: Value::Bool,
    default: Some("false"),
    summary: "Builds images with mkinitcpio instead of dracut",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**InitCPIO**",
        body: GLOBAL_INIT_CPIO_DOC,
    },
    deprecation: None,
    examples: &["InitCPIO: true"],
};

const GLOBAL_INIT_CPIO_DOC: &str = r"
Set to *true* to use **mkinitcpio** instead of **dracut** to create ZFSBootMenu images.
";

/// `Global.DracutConfDir`: the dracut configuration of the images.
pub static GLOBAL_DRACUT_CONF_DIR: OptionSpec = OptionSpec {
    name: "Global.DracutConfDir",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Global),
    value: Value::Text,
    default: None,
    summary: "The dracut configuration of the images",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**DracutConfDir**",
        body: GLOBAL_DRACUT_CONF_DIR_DOC,
    },
    deprecation: None,
    examples: &["DracutConfDir: /etc/zfsbootmenu/dracut.conf.d"],
};

const GLOBAL_DRACUT_CONF_DIR_DOC: &str = r"
The path of the dracut configuration directory for ZFSBootMenu. This **MUST NOT** be the same
location as the system *dracut.conf.d*, as the configuration files there interfere with the
creation of the ZFSBootMenu initramfs. If unspecified, a default value of
*/etc/zfsbootmenu/dracut.conf.d* is assumed. This value is ignored when *Global.InitCPIO* is
*true*.
";

/// `Global.InitCPIOConfig`: the mkinitcpio configuration of the images.
pub static GLOBAL_INIT_CPIO_CONFIG: OptionSpec = OptionSpec {
    name: "Global.InitCPIOConfig",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Global),
    value: Value::Text,
    default: None,
    summary: "The mkinitcpio configuration of the images",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**InitCPIOConfig**",
        body: GLOBAL_INIT_CPIO_CONFIG_DOC,
    },
    deprecation: None,
    examples: &["InitCPIOConfig: /etc/zfsbootmenu/mkinitcpio.conf"],
};

const GLOBAL_INIT_CPIO_CONFIG_DOC: &str = r"
The path to a mkinitcpio configuration file for ZFSBootMenu. The *zfsbootmenu* hook will be
forcefully added when **generate-zbm** invokes **mkinitcpio** using a command-line argument and
does not need to be specified in the *HOOKS* array in the configuration file. This value is
ignored when *Global.InitCPIO* is not *true*.
";

/// `Global.InitCPIOHookDirs`: mkinitcpio hook directories.
pub static GLOBAL_INIT_CPIO_HOOK_DIRS: OptionSpec = OptionSpec {
    name: "Global.InitCPIOHookDirs",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Global),
    value: Value::Text,
    default: None,
    summary: "mkinitcpio hook directories",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**InitCPIOHookDirs**",
        body: GLOBAL_INIT_CPIO_HOOK_DIRS_DOC,
    },
    deprecation: None,
    examples: &["InitCPIOHookDirs: /etc/zfsbootmenu/initcpio"],
};

const GLOBAL_INIT_CPIO_HOOK_DIRS_DOC: &str = r"
A single path or an array of paths to **mkinitcpio** hook directories. When specifying a custom
directory for the *zfsbootmenu* hook, it is generally required to also specify the default
location as well. This option is ignored when *Global.InitCPIO* is not *true*.
";

/// `Global.BootMountPoint`: where the EFI system partition is mounted.
pub static GLOBAL_BOOT_MOUNT_POINT: OptionSpec = OptionSpec {
    name: "Global.BootMountPoint",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Global),
    value: Value::Text,
    default: None,
    summary: "Where the EFI system partition is mounted",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**BootMountPoint**",
        body: GLOBAL_BOOT_MOUNT_POINT_DOC,
    },
    deprecation: None,
    examples: &["BootMountPoint: /boot/efi"],
};

const GLOBAL_BOOT_MOUNT_POINT_DOC: &str = r"
In general, this should be the location of your EFI System Partition. **generate-zbm** will ensure
that this is mounted when images are created and, if **generate-zbm** does the mounting, will
unmount this filesystem on exit. When this parameter is not specified, **generate-zbm** will not
verify or attempt to mount any filesystems.
";

/// `Global.Version`: the version in the names of versioned images.
pub static GLOBAL_VERSION: OptionSpec = OptionSpec {
    name: "Global.Version",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Global),
    value: Value::Text,
    default: None,
    summary: "The version in the names of versioned images",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**Version**",
        body: GLOBAL_VERSION_DOC,
    },
    deprecation: None,
    examples: &[r#"Version: "%{current}""#],
};

const GLOBAL_VERSION_DOC: &str = r"
A specific ZFSBootMenu version string to use in versioned output images. In the string, the value
*%{current}* will be replaced with the release version of ZFSBootMenu. The default value is simply
*%{current}*.
";

/// `Global.DracutFlags`: more arguments for dracut.
pub static GLOBAL_DRACUT_FLAGS: OptionSpec = OptionSpec {
    name: "Global.DracutFlags",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Global),
    value: Value::Text,
    default: None,
    summary: "More arguments for dracut",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**DracutFlags**",
        body: GLOBAL_DRACUT_FLAGS_DOC,
    },
    deprecation: None,
    examples: &[r#"DracutFlags: ["--no-early-microcode"]"#],
};

const GLOBAL_DRACUT_FLAGS_DOC: &str = r"
An array of additional arguments that will be passed to **dracut** when generating an initramfs.
This option is ignored when *Global.InitCPIO* is *true*.
";

/// `Global.InitCPIOFlags`: more arguments for mkinitcpio.
pub static GLOBAL_INIT_CPIO_FLAGS: OptionSpec = OptionSpec {
    name: "Global.InitCPIOFlags",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Global),
    value: Value::Text,
    default: None,
    summary: "More arguments for mkinitcpio",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**InitCPIOFlags**",
        body: GLOBAL_INIT_CPIO_FLAGS_DOC,
    },
    deprecation: None,
    examples: &[r#"InitCPIOFlags: ["-v"]"#],
};

const GLOBAL_INIT_CPIO_FLAGS_DOC: &str = r"
An array of additional arguments that will be passed to **mkinitcpio** when generating an
initramfs. This option is ignored when *Global.InitCPIO* is not *true*.
";

/// `Global.PreHooksDir`: programs to run before images are made.
pub static GLOBAL_PRE_HOOKS_DIR: OptionSpec = OptionSpec {
    name: "Global.PreHooksDir",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Global),
    value: Value::Text,
    default: None,
    summary: "Programs to run before images are made",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**PreHooksDir**",
        body: GLOBAL_PRE_HOOKS_DIR_DOC,
    },
    deprecation: None,
    examples: &["PreHooksDir: /etc/zfsbootmenu/generate-zbm.pre.d"],
};

const GLOBAL_PRE_HOOKS_DIR_DOC: &str = r"
The path of the directory containing executables that should be executed after *BootMountPoint*
has been mounted. Files in this directory should be **+x**, and are executed in the order returned
by a shell glob. The exit code of each hook is ignored.
";

/// `Global.PostHooksDir`: programs to run after images are made.
pub static GLOBAL_POST_HOOKS_DIR: OptionSpec = OptionSpec {
    name: "Global.PostHooksDir",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Global),
    value: Value::Text,
    default: None,
    summary: "Programs to run after images are made",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**PostHooksDir**",
        body: GLOBAL_POST_HOOKS_DIR_DOC,
    },
    deprecation: None,
    examples: &["PostHooksDir: /etc/zfsbootmenu/generate-zbm.post.d"],
};

const GLOBAL_POST_HOOKS_DIR_DOC: &str = r"
The path of the directory containing executables that should be executed after all images have
been created and any file pruning has taken place. Files in this directory should be **+x**, and
are executed in the order returned by a shell glob. The exit code of each hook is ignored.
";

/// `Global.ConsoleFont`: the console font family of the images.
pub static GLOBAL_CONSOLE_FONT: OptionSpec = OptionSpec {
    name: "Global.ConsoleFont",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Global),
    value: Value::Choice(&["terminus", "spleen"]),
    default: Some("terminus"),
    summary: "The console font family of the images: terminus or spleen",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**ConsoleFont**",
        body: GLOBAL_CONSOLE_FONT_DOC,
    },
    deprecation: None,
    examples: &["ConsoleFont: spleen"],
};

const GLOBAL_CONSOLE_FONT_DOC: &str = r"
The console fonts of the images: *terminus*, the default, or *spleen*. **generate-zbm** passes the
choice to **dracut** or **mkinitcpio** as the build option **zfsbootmenu_console_font** (see
**zfsbootmenu(7)**), unless their configuration sets it. Only images with the Rust runtime load
Spleen; images with the legacy runtime keep Terminus. Any other value stops **generate-zbm** with
an error before it builds an image.
";
