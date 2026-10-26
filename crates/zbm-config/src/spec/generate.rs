//! Keys of `/etc/zfsbootmenu/config.yaml`, the configuration of
//! `generate-zbm`, documented in `generate-zbm(5)`. The name of each option
//! is its section and key, `Section.Key`.

use super::{Doc, GenerateSection, OptionSpec, Reader, Source, Value};

mod global;

pub use global::{
    GLOBAL_BOOT_MOUNT_POINT, GLOBAL_CONSOLE_FONT, GLOBAL_DRACUT_CONF_DIR, GLOBAL_DRACUT_FLAGS,
    GLOBAL_INIT_CPIO, GLOBAL_INIT_CPIO_CONFIG, GLOBAL_INIT_CPIO_FLAGS, GLOBAL_INIT_CPIO_HOOK_DIRS,
    GLOBAL_MANAGE_IMAGES, GLOBAL_POST_HOOKS_DIR, GLOBAL_PRE_HOOKS_DIR, GLOBAL_VERSION,
};

/// The options, in the order of the manual page.
pub static ALL: &[&OptionSpec] = &[
    &GLOBAL_MANAGE_IMAGES,
    &GLOBAL_INIT_CPIO,
    &GLOBAL_DRACUT_CONF_DIR,
    &GLOBAL_INIT_CPIO_CONFIG,
    &GLOBAL_INIT_CPIO_HOOK_DIRS,
    &GLOBAL_BOOT_MOUNT_POINT,
    &GLOBAL_VERSION,
    &GLOBAL_DRACUT_FLAGS,
    &GLOBAL_INIT_CPIO_FLAGS,
    &GLOBAL_CONSOLE_FONT,
    &GLOBAL_PRE_HOOKS_DIR,
    &GLOBAL_POST_HOOKS_DIR,
    &KERNEL_COMMAND_LINE,
    &KERNEL_PATH,
    &KERNEL_VERSION,
    &KERNEL_PREFIX,
    &KERNEL_GRAPHICS_DRIVERS,
    &COMPONENTS_ENABLED,
    &COMPONENTS_IMAGE_DIR,
    &COMPONENTS_VERSIONS,
    &EFI_ENABLED,
    &EFI_IMAGE_DIR,
    &EFI_VERSIONS,
    &EFI_STUB,
    &EFI_SPLASH_IMAGE,
    &EFI_DEVICE_TREE,
];

/// `Kernel.CommandLine`: the command line of unified EFI images.
pub static KERNEL_COMMAND_LINE: OptionSpec = OptionSpec {
    name: "Kernel.CommandLine",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Kernel),
    value: Value::Text,
    default: None,
    summary: "The command line of unified EFI images",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**CommandLine**",
        body: KERNEL_COMMAND_LINE_DOC,
    },
    deprecation: None,
    examples: &["CommandLine: ro quiet loglevel=0"],
};

const KERNEL_COMMAND_LINE_DOC: &str = r"
If you're making a unified EFI file, this is the command line passed to the boot image.
";

/// `Kernel.Path`: the kernel to build images from.
pub static KERNEL_PATH: OptionSpec = OptionSpec {
    name: "Kernel.Path",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Kernel),
    value: Value::Text,
    default: None,
    summary: "The kernel to build images from",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**Path**",
        body: KERNEL_PATH_DOC,
    },
    deprecation: None,
    examples: &["Path: /boot/vmlinuz-6.6.1"],
};

const KERNEL_PATH_DOC: &str = r"
The full path to a specific kernel to use when making the boot images. If not specified,
**generate-zbm** will try to pick a reasonable kernel.
";

/// `Kernel.Version`: the kernel version to build images with, or a glob.
pub static KERNEL_VERSION: OptionSpec = OptionSpec {
    name: "Kernel.Version",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Kernel),
    value: Value::Text,
    default: None,
    summary: "The kernel version to build images with, or a glob",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**Version**",
        body: KERNEL_VERSION_DOC,
    },
    deprecation: None,
    examples: &[r#"Version: "%{current}""#],
};

const KERNEL_VERSION_DOC: &str = r"
A specific kernel version to use, or a glob used to match possible kernel versions. The value
*%{current}* will be replaced with the output of ``uname -r``. For globs, the highest version
matching the glob will be selected. If not set, **generate-zbm** will try to parse the path of the
selected kernel filename for a version.
";

/// `Kernel.Prefix`: the prefix of the names of the images.
pub static KERNEL_PREFIX: OptionSpec = OptionSpec {
    name: "Kernel.Prefix",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Kernel),
    value: Value::Text,
    default: None,
    summary: "The prefix of the names of the images",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**Prefix**",
        body: KERNEL_PREFIX_DOC,
    },
    deprecation: None,
    examples: &["Prefix: zfsbootmenu"],
};

const KERNEL_PREFIX_DOC: &str = r"
The prefix to use for the names of ZFSBootMenu images. By default, the prefix is extracted from
the input kernel name.

The initramfs images of separate components are named *initramfs* for the prefixes *vmlinuz* and
*vmlinux*, and *initramfs-* followed by the rest of the prefix otherwise: *initramfs-linux-lts* for
*vmlinuz-linux-lts*, *initramfs-zbm* for *zbm*. The images of two prefixes can then share an image
directory.
";

/// `Kernel.GraphicsDrivers`: the graphics drivers of the images and the GPUs
/// they are for.
pub static KERNEL_GRAPHICS_DRIVERS: OptionSpec = OptionSpec {
    name: "Kernel.GraphicsDrivers",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Kernel),
    value: Value::Text,
    default: None,
    summary: "The graphics drivers of the images and the GPUs they are for",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**GraphicsDrivers**",
        body: KERNEL_GRAPHICS_DRIVERS_DOC,
    },
    deprecation: None,
    examples: &[
        "GraphicsDrivers: amdgpu",
        "GraphicsDrivers: amdgpu:gfx12 i915:gen12",
    ],
};

const KERNEL_GRAPHICS_DRIVERS_DOC: &str = r"
Graphics drivers to add to the images, with the firmware of the GPUs they are for, so that the
console runs at the display's own resolution instead of on the framebuffer the firmware left
behind. The drivers are *amdgpu* for AMD GPUs from GCN 3 on, *i915* for Intel graphics up to
Meteor Lake and *xe* for Intel graphics from Lunar Lake on. Only images with the Rust runtime take
them; they load before anything is asked, and **zbm.gpu** on the command line chooses the GPU they
drive, or none (see **zfsbootmenu(7)**). Not finished: the runtime does not yet hand the GPU back
before it boots a kernel, so the booted system may find it unusable (see ROADMAP.md, item 17).

A driver named alone is for the GPUs of the build machine, found by their PCI IDs; for an AMD GPU
that **amdgpu** drives while the image is built, its exact chip is read from the driver. After a
colon, a list of chips or families separated by commas builds for those instead, for an image
meant for another machine or several: *amdgpu:gfx12*, *amdgpu:navi10,gfx9*. The families of
*amdgpu* are *gfx8*, *gfx9*, *gfx10*, *gfx10.3*, *gfx11*, *gfx11.5* and *gfx12* (RDNA 4, such
as the Radeon RX 9060 XT and 9070); those of *i915* are *gen9*, *gen11*, *gen12* and *gen12.7*;
those of *xe* are *xe2* and *xe3*. An unknown name lists the known ones.

**generate-zbm** reports the GPUs it chose and how many firmware files each takes. It takes only
the files the build kernel's driver lists (**modinfo -F firmware**), and stops with an error,
before building, when a driver, chip or family is unknown, when the build machine has no GPU for a
driver named alone, or when the kernel lists no firmware for a chosen GPU. It passes the drivers
and files to **dracut** or **mkinitcpio** as the build options **zfsbootmenu_graphics_drivers**
and **zfsbootmenu_graphics_firmware** (see **zfsbootmenu(7)**), and removes *nomodeset* from
**CommandLine**, which would keep the drivers from starting.
";

/// `Components.Enabled`: whether a separate kernel and initramfs are made.
pub static COMPONENTS_ENABLED: OptionSpec = OptionSpec {
    name: "Components.Enabled",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Components),
    value: Value::Bool,
    default: Some("false"),
    summary: "Whether a separate kernel and initramfs are made",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**Enabled**",
        body: COMPONENTS_ENABLED_DOC,
    },
    deprecation: None,
    examples: &["Enabled: true"],
};

const COMPONENTS_ENABLED_DOC: &str = r"
When *true*, **generate-zbm** will create separate ZFSBootMenu kernel and initramfs images. The
default value is *false*.
";

/// `Components.ImageDir`: where the kernel and initramfs go.
pub static COMPONENTS_IMAGE_DIR: OptionSpec = OptionSpec {
    name: "Components.ImageDir",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Components),
    value: Value::Text,
    default: None,
    summary: "Where the kernel and initramfs go",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**ImageDir**",
        body: COMPONENTS_IMAGE_DIR_DOC,
    },
    deprecation: None,
    examples: &["ImageDir: /boot/efi/EFI/zbm"],
};

const COMPONENTS_IMAGE_DIR_DOC: &str = r"
The destination directory for separate initramfs and kernel images. It must be set when
*Components.Enabled* is *true*.
";

/// `Components.Versions`: whether images are versioned, and how many are kept.
pub static COMPONENTS_VERSIONS: OptionSpec = OptionSpec {
    name: "Components.Versions",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Components),
    value: Value::Text,
    default: None,
    summary: "Whether images are versioned, and how many are kept",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**Versions**",
        body: COMPONENTS_VERSIONS_DOC,
    },
    deprecation: None,
    examples: &["Versions: false", "Versions: 3"],
};

const COMPONENTS_VERSIONS_DOC: &str = r"
When *false* or *0*, image versioning will be disabled; **generate-zbm** will not use its
*Global.Version* parameter to name outputs, and will keep exactly one backup copy of any image it
would overwrite.

When *true* (which behaves as *1*) or any positive integer, **generate-zbm** will append the value
of *Global.Version* to every image it produces, followed by a revision as *_$revision*.
**generate-zbm** will save *Components.Versions* revisions of all images with versions matching
the current value of *Global.Version*. In addition, **generate-zbm** will save the highest
revision of the most recent *Components.Versions* image versions distinct from *Global.Version*.
";

/// `EFI.Enabled`: whether a unified EFI image is made.
pub static EFI_ENABLED: OptionSpec = OptionSpec {
    name: "EFI.Enabled",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Efi),
    value: Value::Bool,
    default: Some("false"),
    summary: "Whether a unified EFI image is made",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**Enabled**",
        body: EFI_ENABLED_DOC,
    },
    deprecation: None,
    examples: &["Enabled: true"],
};

const EFI_ENABLED_DOC: &str = r"
When *true*, **generate-zbm** will create unified UEFI bundles. The default value is *false*.
";

/// `EFI.ImageDir`: where the unified EFI images go.
pub static EFI_IMAGE_DIR: OptionSpec = OptionSpec {
    name: "EFI.ImageDir",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Efi),
    value: Value::Text,
    default: None,
    summary: "Where the unified EFI images go",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**ImageDir**",
        body: EFI_IMAGE_DIR_DOC,
    },
    deprecation: None,
    examples: &["ImageDir: /boot/efi/EFI/zbm"],
};

const EFI_IMAGE_DIR_DOC: &str = r"
The destination directory for unified EFI files. It must be set when *EFI.Enabled* is *true*.
";

/// `EFI.Versions`: whether images are versioned, and how many are kept.
pub static EFI_VERSIONS: OptionSpec = OptionSpec {
    name: "EFI.Versions",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Efi),
    value: Value::Text,
    default: None,
    summary: "Whether images are versioned, and how many are kept",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**Versions**",
        body: EFI_VERSIONS_DOC,
    },
    deprecation: None,
    examples: &["Versions: 2"],
};

const EFI_VERSIONS_DOC: &str = r"
Behaves similarly to *Components.Versions*, but acts on files matching the UEFI bundle naming
scheme.
";

/// `EFI.Stub`: the EFI stub of unified images.
pub static EFI_STUB: OptionSpec = OptionSpec {
    name: "EFI.Stub",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Efi),
    value: Value::Text,
    default: None,
    summary: "The EFI stub of unified images",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**Stub**",
        body: EFI_STUB_DOC,
    },
    deprecation: None,
    examples: &["Stub: /usr/lib/systemd/boot/efi/linuxx64.efi.stub"],
};

const EFI_STUB_DOC: &str = r"
The path to the EFI stub loader used to boot the unified bundle. If not set, a default of
``/usr/lib/systemd/boot/efi/linuxx64.efi.stub`` is assumed.
";

/// `EFI.SplashImage`: A bitmap shown while ZFSBootMenu loads.
pub static EFI_SPLASH_IMAGE: OptionSpec = OptionSpec {
    name: "EFI.SplashImage",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Efi),
    value: Value::Text,
    default: None,
    summary: "A bitmap shown while ZFSBootMenu loads",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**SplashImage**",
        body: EFI_SPLASH_IMAGE_DOC,
    },
    deprecation: None,
    examples: &["SplashImage: /etc/zfsbootmenu/splash.bmp"],
};

const EFI_SPLASH_IMAGE_DOC: &str = r"
The path to a bitmap image file (BMP) to use as a splash image before ZFSBootMenu loads. Only
works if using systemd-boot's EFI stub loader. The ZFSBootMenu logo is available in BMP format at
``/usr/share/examples/zfsbootmenu/splash.bmp``.
";

/// `EFI.DeviceTree`: A device tree to embed in unified images.
pub static EFI_DEVICE_TREE: OptionSpec = OptionSpec {
    name: "EFI.DeviceTree",
    aliases: &[],
    source: Source::GenerateZbm(GenerateSection::Efi),
    value: Value::Text,
    default: None,
    summary: "A device tree to embed in unified images",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**DeviceTree**",
        body: EFI_DEVICE_TREE_DOC,
    },
    deprecation: None,
    examples: &["DeviceTree: rockchip/rk3399-rockpro64.dtb"],
};

const EFI_DEVICE_TREE_DOC: &str = r"
The path to a device-tree file that, when specified, will be embedded in the EFI bundle. The path
may be relative or absolute.

Absolute paths may contain a template placeholder, *%{kernel}*, that will be replaced with the
kernel version that *generate-zbm* builds into the EFI bundle.

Relative paths are interpreted as children of the path */boot/dtbs/dtbs-%{kernel}/*, a well-known
location used by Void Linux (and possibly other distributions) to store device-tree files for
installed kernels. The placeholder *%{kernel}* in the implicit prefix behaves as the template
placeholder in an absolute path. An explicit template placeholder is not supported in relative
paths.
";
