//! Properties of pools and datasets that ZFSBootMenu reads, documented in
//! `zfsbootmenu(7)`.

use super::{Doc, OptionSpec, Reader, Source, Value};

/// The options, in the order of the manual page.
pub static ALL: &[&OptionSpec] = &[
    &BOOTFS,
    &READONLY,
    &KERNEL,
    &COMMANDLINE,
    &ACTIVE,
    &ROOTPREFIX,
    &KEYSOURCE,
    &DEVICETREE,
];

/// `bootfs`: the default boot environment of the pool.
pub static BOOTFS: OptionSpec = OptionSpec {
    name: "bootfs",
    aliases: &[],
    source: Source::PoolProperty,
    value: Value::Text,
    default: None,
    summary: "The default boot environment of the pool",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**bootfs**",
        body: BOOTFS_DOC,
    },
    deprecation: None,
    examples: &["bootfs=zroot/ROOT/void"],
};

const BOOTFS_DOC: &str = r#"
A dataset that will be considered the default boot environment if the pool is the first to be
imported by ZFSBootMenu.

.. note::

  This must be set for automatic booting to function. When no **bootfs** property is detected,
  ZFSBootMenu will always display a selection menu.
"#;

/// `org.zfsbootmenu:readonly`.
pub static READONLY: OptionSpec = OptionSpec {
    name: "org.zfsbootmenu:readonly",
    aliases: &[],
    source: Source::PoolProperty,
    value: Value::Text,
    default: None,
    summary: "Refuses operations that need the pool imported read-write",
    reader: Reader::Shell("the legacy menu"),
    doc: Doc::Entry {
        heading: "**org.zfsbootmenu:readonly**",
        body: READONLY_DOC,
    },
    deprecation: None,
    examples: &["org.zfsbootmenu:readonly=on"],
};

const READONLY_DOC: &str = r#"
When set to any value other than *-* or *off*, ZFSBootMenu operations which require the pool to be
imported read-write will fail.
"#;

/// `org.zfsbootmenu:kernel`: the kernel to boot, by name or version.
pub static KERNEL: OptionSpec = OptionSpec {
    name: "org.zfsbootmenu:kernel",
    aliases: &[],
    source: Source::DatasetProperty,
    value: Value::Text,
    default: None,
    summary: "The kernel to boot, by name or version",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**org.zfsbootmenu:kernel**",
        body: KERNEL_DOC,
    },
    deprecation: None,
    examples: &["org.zfsbootmenu:kernel=5.4"],
};

const KERNEL_DOC: &str = r#"
An identifier used to select which kernel to boot among all kernels found in the */boot* directory
of the selected boot environment. This can be a partial kernel name (e.g., *5.4*) or a full
filename (e.g., *vmlinuz-5.7.11_1*).

If the identifier does not match any kernels, the latest kernel will be chosen as a fallback.
"#;

/// `org.zfsbootmenu:commandline`.
pub static COMMANDLINE: OptionSpec = OptionSpec {
    name: "org.zfsbootmenu:commandline",
    aliases: &[],
    source: Source::DatasetProperty,
    value: Value::Text,
    default: None,
    summary: "The kernel command line of the boot environment",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**org.zfsbootmenu:commandline**",
        body: COMMANDLINE_DOC,
    },
    deprecation: None,
    examples: &[r#"org.zfsbootmenu:commandline="quiet %{parent}""#],
};

const COMMANDLINE_DOC: &str = r#"
A list of command-line arguments to be passed to any kernel in this environment that does not have
an associated per-kernel argument file. Refer to **Passing Kernel Command Lines to Boot
Environments** for details.
"#;

/// `org.zfsbootmenu:active`: shows (on) or hides (off) the boot environment.
pub static ACTIVE: OptionSpec = OptionSpec {
    name: "org.zfsbootmenu:active",
    aliases: &[],
    source: Source::DatasetProperty,
    value: Value::Choice(&["on", "off"]),
    default: None,
    summary: "Shows (on) or hides (off) the boot environment",
    reader: Reader::Shell("the legacy menu"),
    doc: Doc::Entry {
        heading: "**org.zfsbootmenu:active**",
        body: ACTIVE_DOC,
    },
    deprecation: None,
    examples: &["org.zfsbootmenu:active=off"],
};

const ACTIVE_DOC: &str = r#"
This controls whether boot environments appear in or are hidden from ZFSBootMenu.

**off**

  For boot environments with *mountpoint=/*, set **org.zfsbootmenu:active=off** to **HIDE** the
  environment.

**on**

  For boot environments with *mountpoint=legacy*, set **org.zfsbootmenu:active=on** to **SHOW**
  the environment.

By default, ZFSBootMenu only shows boot environments with the property *mountpoint=/*.
"#;

/// `org.zfsbootmenu:rootprefix`.
pub static ROOTPREFIX: OptionSpec = OptionSpec {
    name: "org.zfsbootmenu:rootprefix",
    aliases: &[],
    source: Source::DatasetProperty,
    value: Value::Text,
    default: None,
    summary: "What precedes the root dataset on the kernel command line",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**org.zfsbootmenu:rootprefix**",
        body: ROOTPREFIX_DOC,
    },
    deprecation: None,
    examples: &["org.zfsbootmenu:rootprefix=root=ZFS="],
};

const ROOTPREFIX_DOC: &str = r#"
This specifies the prefix added to the ZFS filesystem provided as the root filesystem on the
kernel command line. For example, the command-line argument *root=zfs:zroot/ROOT/void* has root
prefix *root=zfs:*.

The default prefix is *root=zfs:* for most boot environments. Environments that appear to be Arch
Linux will use *zfs=* by default, while those that appear to be Gentoo or Alpine will use a
default of *root=ZFS=*. The root prefix is generally determined by the initramfs generator, and
the default is selected to match the expectation of the preferred initramfs generator on each
distribution.

Set this property to override the value determined from inspecting the boot environment.
"#;

/// `org.zfsbootmenu:keysource`.
pub static KEYSOURCE: OptionSpec = OptionSpec {
    name: "org.zfsbootmenu:keysource",
    aliases: &[],
    source: Source::DatasetProperty,
    value: Value::Text,
    default: None,
    summary: "A file system to mount for the key files of the boot environment",
    reader: Reader::Shell("the legacy menu"),
    doc: Doc::Entry {
        heading: "**org.zfsbootmenu:keysource=<filesystem>**",
        body: KEYSOURCE_DOC,
    },
    deprecation: None,
    examples: &["org.zfsbootmenu:keysource=zroot/keystore"],
};

const KEYSOURCE_DOC: &str = r#"
If specified, this provides the name of the ZFS filesystem from which keys for a particular boot
environment will be sourced.

Normally, when ZFSBootMenu attempts to load encryption keys for a boot environment, it will
attempt to look for a key file at the path specified by the *keylocation* property on the
*encryptionroot* for that boot environment. If that file does not exist, and
*keyformat=passphrase* is set for the *encryptionroot* (or *keylocation=prompt*), ZFSBootMenu will
prompt for a passphrase to unlock the boot environment. These passphrases entered are not cached
by default.

When **org.zfsbootmenu:keysource** is a mountable ZFS filesystem, before prompting for a
passphrase when *keylocation* is not set to *prompt*, ZFSBootMenu will attempt to mount
**<filesystem>** (unlocking that, if necessary) and search for the key file within
**<filesystem>**. When **<filesystem>** specifies a *mountpoint* property that is not *none* or
*legacy*, the specified mount point will be stripped (if possible) from the beginning of any
*keylocation* property to attempt to identify a key at the point where it would normally be
mounted. If no file exists at the stripped path (or the *mountpoint* specifies *none* or
*legacy*), keys will be sought at the full path of *keylocation* relative to **<filesystem>**. If
a key is found at either location, it will be copied to the initramfs. The copy in the initramfs
will be used to decrypt the original boot environment. Copied keys are retained until ZFSBootMenu
boots an environment, so a single password prompt can be sufficient to unlock several pools with
the same *keysource* or prevent prompts from reappearing when the pool must be exported and
reimported (for example, to alter boot parameters from within ZFSBootMenu).
"#;

/// `org.zfsbootmenu:devicetree`: A device tree to load with the kernel.
pub static DEVICETREE: OptionSpec = OptionSpec {
    name: "org.zfsbootmenu:devicetree",
    aliases: &[],
    source: Source::DatasetProperty,
    value: Value::Text,
    default: None,
    summary: "A device tree to load with the kernel",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**org.zfsbootmenu:devicetree=<devicetree file definition>**",
        body: DEVICETREE_DOC,
    },
    deprecation: None,
    examples: &["org.zfsbootmenu:devicetree=rockchip/rk3399-rockpro64.dtb"],
};

const DEVICETREE_DOC: &str = r#"
If specified, this provides the path to a device-tree file that will be loaded when ZFSBootMenu
loads and boots an environment. The path may be relative or absolute.

Absolute paths may contain a template placeholder, *%{kernel}*, that will be replaced with the
version of the kernel that will be booted.

Relative paths are interpreted as children of the path */boot/dtbs/dtbs-%{kernel}/*, a well-known
location used by Void Linux (and possibly other distributions) to store device-tree files for
installed kernels. The placeholder *%{kernel}* in the implicit prefix behaves as the template
placeholder in an absolute path. An explicit template placeholder is not supported in relative
paths.

Whether relative or absolute, the device-tree path is always interpreted within the environment to
be booted.

**/templated/path/to/%{kernel}/platform.dtb**

  The value *%{kernel}* in the property will be replaced with the kernel version that is to be
  launched from the boot environment.

**short/path/platform.dtb**

  This value is appended to */boot/dtbs/dtbs-%{kernel}/*, a well-known path used by Void Linux and
  possibly other distributions.

**/absolute/path/to/platform.dtb**

  This is the absolute path to a device tree file, regardless of the kernel that is to be
  launched.
"#;
