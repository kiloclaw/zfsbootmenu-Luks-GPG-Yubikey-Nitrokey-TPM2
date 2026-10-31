//! Options of the kernel command line of ZFSBootMenu, documented in
//! `zfsbootmenu(7)`; those for LUKS volumes are in [`luks`].

pub mod console;
pub mod graphics;
pub mod logs;
pub mod luks;
pub mod shell;

use super::{Alias, Deprecation, Doc, OptionSpec, Reader, Source, Value};
pub use console::{
    AUTOSIZE, BELLS, COLUMNS, COMPOSE_KEYS, CONSOLE_BELL, CONSOLE_BLANK, CONSOLE_LARGE,
    CONSOLE_MIRROR, CONSOLE_MODE, CONSOLE_PALETTE, CONSOLE_SCROLLBACK, CONSOLE_STATUS,
    DISPLAY_CONSOLE, DISPLAY_CONSOLE_FONT, KEYBOARD_COMPOSE, KEYBOARD_LAYOUT, KEYBOARD_LAYOUTS,
    KEYBOARD_REPEAT, LINES, MAX_BLANK_MINUTES, MAX_SCROLLBACK, NUM_LOCK, PALETTES, VCONSOLE_FONT,
    is_layout_name, mode_size,
};
pub use graphics::{GPU, GPU_UNTESTED};
pub use logs::SAVE_LOGS;
pub use shell::SHELL;

/// The options, in the order of the manual page.
pub static ALL: &[&OptionSpec] = &[
    &LOGLEVEL,
    &CONSOLE,
    &SPL_HOSTID,
    &PREFER,
    &RETRY_DELAY,
    &IMPORT_POLICY,
    &FORCE_IMPORT,
    &SET_HOSTID,
    &SORT_KEY,
    &TIMEOUT,
    &SHOW,
    &SKIP,
    &LINES,
    &COLUMNS,
    &HOOKROOT,
    &KCL_OVERRIDE,
    &SKIP_HOOKS,
    &AUTOSIZE,
    &VCONSOLE_FONT,
    &DISPLAY_CONSOLE,
    &DISPLAY_CONSOLE_FONT,
    &KEYBOARD_LAYOUT,
    &NUM_LOCK,
    &KEYBOARD_REPEAT,
    &CONSOLE_SCROLLBACK,
    &CONSOLE_PALETTE,
    &CONSOLE_LARGE,
    &CONSOLE_STATUS,
    &CONSOLE_BELL,
    &CONSOLE_MODE,
    &CONSOLE_BLANK,
    &KEYBOARD_COMPOSE,
    &KEYBOARD_LAYOUTS,
    &CONSOLE_MIRROR,
    &GPU,
    &GPU_UNTESTED,
    &logs::SAVE_LOGS,
    &SHELL,
    &WAIT_FOR,
    &ROOT,
];

/// `loglevel`: the kernel log level of ZFSBootMenu's messages; at least 4.
pub static LOGLEVEL: OptionSpec = OptionSpec {
    name: "loglevel",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Integer,
    default: Some("4"),
    summary: "The kernel log level of ZFSBootMenu's messages; at least 4",
    reader: Reader::Rust,
    doc: Doc::NotInManual("a parameter of the kernel itself"),
    deprecation: None,
    examples: &["loglevel=7"],
};

/// `console`: the console of the menu: the device of the last console=.
pub static CONSOLE: OptionSpec = OptionSpec {
    name: "console",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: Some("tty1"),
    summary: "The console of the menu: the device of the last console=",
    reader: Reader::Rust,
    doc: Doc::NotInManual("a parameter of the kernel itself"),
    deprecation: None,
    examples: &["console=ttyS0,115200n8"],
};

/// `spl.spl_hostid`: the host ID to import pools with.
pub static SPL_HOSTID: OptionSpec = OptionSpec {
    name: "spl.spl_hostid",
    aliases: &[Alias {
        name: "spl_hostid",
        deprecation: None,
    }],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "The host ID to import pools with",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**spl_hostid=<hostid>**",
        body: SPL_HOSTID_DOC,
    },
    deprecation: None,
    examples: &["spl_hostid=0x00bab10c", "spl.spl_hostid=0xdeadbeef"],
};

const SPL_HOSTID_DOC: &str = r"
When creating an initramfs or UEFI bundle, the */etc/hostid* from the system is copied into the
target. If this image will be used on another system with a different hostid, replace **<hostid>**
with the desired hostid, as an eight-digit hexadecimal number, to override the value contained
within the image.
";

/// `zbm.prefer`: the pool to import first, or the boot environment to prefer.
pub static PREFER: OptionSpec = OptionSpec {
    name: "zbm.prefer",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "The pool to import first, or the boot environment to prefer",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.prefer**",
        body: PREFER_DOC,
    },
    deprecation: None,
    examples: &[
        "zbm.prefer=zroot",
        "zbm.prefer=zroot!",
        "zbm.prefer=zroot/ROOT/void!!",
    ],
};

const PREFER_DOC: &str = r"
ZFSBootMenu attempts to import all detected pools by default, with the *bootfs* property on the
first pool (sorted alphabetically) used to select the default boot environment. Providing a *pool*
name for this parameter will override the pool import order. Providing a *dataset* for this
parameter will override both the pool import order and the default boot environment.


**zbm.prefer=<pool|dataset>**

  The simplest form attempts to import **<pool>** or the pool component of a **<dataset>** before
  any other pool. If a *dataset* is provided, that will be used as the boot environment if it
  exists.

**zbm.prefer=<pool|dataset>!**

  If a literal *!* has been appended to the pool name, ZFSBootMenu will insist on successfully
  importing the named pool before attempting to import any others.

**zbm.prefer=<pool|dataset>!!**

  If a literal *!!* has been appended to the pool name, ZFSBootMenu will insist on successfully
  importing the named pool and no others.
";

/// `zbm.retry_delay`.
pub static RETRY_DELAY: OptionSpec = OptionSpec {
    name: "zbm.retry_delay",
    aliases: &[Alias {
        name: "zbm.import_delay",
        deprecation: Some(Deprecation {
            heading: "**zbm.import_delay**",
            instead: "use **zbm.retry_delay**",
        }),
    }],
    source: Source::KernelCommandLine,
    value: Value::Integer,
    default: Some("5"),
    summary: "Seconds between attempts to import pools and find devices",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.retry_delay=<time>**",
        body: RETRY_DELAY_DOC,
    },
    deprecation: None,
    examples: &["zbm.retry_delay=10"],
};

const RETRY_DELAY_DOC: &str = r"
This option determines the interval between repeated attempts of required steps. When **<time>**
is unspecified or is anything other than a positive integer, a default value of 5 seconds will be
used. Should ZFSBootMenu fail to successfully import any pool, it will repeat import attempts
indefinitely until at least one pool can be imported or the user chooses to drop to a recovery
shell. Additionally, should any required devices be configured via **zbm.wait_for**, device checks
will repeat on this interval.
";

/// `zbm.import_policy`.
pub static IMPORT_POLICY: OptionSpec = OptionSpec {
    name: "zbm.import_policy",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Choice(&["hostid", "strict", "force"]),
    default: Some("hostid"),
    summary: "How pools are imported: by host ID matching, strictly, or forced",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.import_policy**",
        body: IMPORT_POLICY_DOC,
    },
    deprecation: None,
    examples: &["zbm.import_policy=strict", "zbm.import_policy=force"],
};

const IMPORT_POLICY_DOC: &str = r"
This option controls how the pool import process should take place.

**zbm.import_policy=hostid**

  Set this option to allow run-time reconfiguration of the SPL hostid. If a pool is preferred via
  **zbm.prefer** and the pool can not be imported with a preconfigured hostid, the system will
  attempt to adopt the hostid of the system that last imported the pool. If a preferred pool is
  not set and no pools can be imported using a preconfigured hostid, the system will adopt the
  hostid of the first otherwise-importable pool. After adopting a detected hostid, ZFSBootMenu
  will subsequently attempt to import as many pools as possible. This is the default import
  policy.

**zbm.import_policy=strict**

  Set this option to only import pools that match the SPL hostid configured in ZFSBootMenu. If
  none can be imported, an emergency shell will be invoked. The *strict* policy is consistent with
  the behavior of earlier versions of ZFSBootMenu.

**zbm.import_policy=force**

  Set this option to attempt to force pool imports. When set, this invokes *zpool import -f* in
  place of the regular *zpool import* command, which will attempt to import a pool that's
  potentially in use on another system. Use this option with caution!
";

/// `zbm.force_import`: forces pool imports.
pub static FORCE_IMPORT: OptionSpec = OptionSpec {
    name: "zbm.force_import",
    aliases: &[Alias {
        name: "force_import",
        deprecation: Some(Deprecation {
            heading: "**force_import=1**",
            instead: "use **zbm.import_policy=force**",
        }),
    }],
    source: Source::KernelCommandLine,
    value: Value::Bool,
    default: Some("0"),
    summary: "Forces pool imports",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.import_policy"),
    deprecation: Some(Deprecation {
        heading: "**zbm.force_import=1**",
        instead: "use **zbm.import_policy=force**",
    }),
    examples: &["zbm.force_import=1"],
};

/// `zbm.set_hostid`.
pub static SET_HOSTID: OptionSpec = OptionSpec {
    name: "zbm.set_hostid",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Bool,
    default: Some("1"),
    summary: "Whether boot environments get the host ID their pool was imported with",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.set_hostid**",
        body: SET_HOSTID_DOC,
    },
    deprecation: None,
    examples: &["zbm.set_hostid=0"],
};

const SET_HOSTID_DOC: &str = r"
Setting this option will cause ZFSBootMenu to set the *spl.spl_hostid* command-line parameter for
the selected boot environment to the hostid used to import its pool. The SPL kernel module will
use this value as the hostid of the booted environment regardless of the contents of
*/etc/hostid*. As a special case, if the hostid to be set is zero, ZFSBootMenu will instead set
*spl_hostid=00000000*, which should be used by dracut-based initramfs images to write an all-zero
*/etc/hostid* in the initramfs prior to importing the boot pool. This option is on by default.

.. note::

  Setting *spl.spl_hostid* to a non-zero value on the kernel commandline will make the ZFS kernel
  modules **ignore** any value set in */etc/hostid*. To restore standard ZFS behavior on a running
  system, execute

  .. code-block::

    echo 0 > /sys/module/spl/parameters/spl_hostid
";

/// `zbm.sort_key`.
pub static SORT_KEY: OptionSpec = OptionSpec {
    name: "zbm.sort_key",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Choice(&["name", "creation", "used"]),
    default: Some("name"),
    summary: "The property the boot environment and snapshot lists are sorted by",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.sort_key**",
        body: SORT_KEY_DOC,
    },
    deprecation: None,
    examples: &["zbm.sort_key=creation"],
};

const SORT_KEY_DOC: &str = r"
This option accepts a ZFS property name by which the boot environment and snapshot lists will be
sorted.

**zbm.sort_key=name**

  Sort the lists by *name*. This is the default sorting method.

**zbm.sort_key=creation**

  Sort the lists by *creation* date.

**zbm.sort_key=used**

  Sort the lists by size *used*.
";

/// `zbm.timeout`.
pub static TIMEOUT: OptionSpec = OptionSpec {
    name: "zbm.timeout",
    aliases: &[Alias {
        name: "timeout",
        deprecation: Some(Deprecation {
            heading: "**timeout**",
            instead: "use **zbm.timeout**",
        }),
    }],
    source: Source::KernelCommandLine,
    value: Value::Integer,
    default: Some("10"),
    summary: "Seconds before the default boot environment boots; -1 shows the menu",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.timeout**",
        body: TIMEOUT_DOC,
    },
    deprecation: None,
    examples: &["zbm.timeout=5", "zbm.timeout=-1", "zbm.timeout=0"],
};

const TIMEOUT_DOC: &str = r"
This option accepts numeric values that control whether and when the boot-environment menu should
be displayed.

**zbm.timeout=0** | **zbm.skip**

  When possible, bypass the menu and immediately boot a configured *bootfs* pool property.

**zbm.timeout=-1** | **zbm.show**

  Rather than present a countdown timer for automatic selection, immediately display the
  boot-environment menu.

**zbm.timeout=<positive integer>**

  Display a countdown timer for the specified number of seconds before booting the configured
  *bootfs* boot environment.
";

/// `zbm.show`: shows the menu without a countdown (zbm.timeout=-1).
pub static SHOW: OptionSpec = OptionSpec {
    name: "zbm.show",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Bool,
    default: Some("0"),
    summary: "Shows the menu without a countdown (zbm.timeout=-1)",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.timeout"),
    deprecation: None,
    examples: &["zbm.show"],
};

/// `zbm.skip`: boots the default boot environment at once (zbm.timeout=0).
pub static SKIP: OptionSpec = OptionSpec {
    name: "zbm.skip",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Bool,
    default: Some("0"),
    summary: "Boots the default boot environment at once (zbm.timeout=0)",
    reader: Reader::Rust,
    doc: Doc::Within("zbm.timeout"),
    deprecation: None,
    examples: &["zbm.skip"],
};

/// `zbm.hookroot`: A file system with more hooks, as device//path.
pub static HOOKROOT: OptionSpec = OptionSpec {
    name: "zbm.hookroot",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "A file system with more hooks, as device//path",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.hookroot=<hookspec>**",
        body: HOOKROOT_DOC,
    },
    deprecation: None,
    examples: &["zbm.hookroot=UUID=DEAD-BEEF//EFI/zfsbootmenu/hooks"],
};

const HOOKROOT_DOC: &str = r#"
Tell ZFSBootMenu to attempt to read any early-setup, setup or teardown hooks from the path
specified by *hookspec* in addition to any included directly in the image.

The *hookspec* parameter takes the form

.. code-block::

  device//path

where *device* is either a regular device node (e.g., */dev/sda*) or other partition identifier
recognized by :manpage:`mount(8)` (e.g., *LABEL=<label>* o *UUID=<uuid>*). The *path* component
following *//* represents the location of a directory with respect to the root of the filesystem
on *device*. For example, if a partition with a UUID of *DEAD-BEEF* is mounted at */boot/efi* on a
running system and the hook root should refer to the path

.. code-block::

  /boot/efi/EFI/zfsbootmenu/hooks,

the corresponding hook specification should be

.. code-block::

  zbm.hookroot=UUID=DEAD-BEEF//EFI/zfsbootmenu/hooks

on the ZFSBootMenu command line. Note that any kernel modules necessary to mount the specified
filesystem must be present in the ZFSBootMenu image. (For example, mounting a FAT32 filesystem may
require that *vfat.ko*, *fat.ko*, *nls_cp437.ko* and *nls_iso8859_1.ko* be added to the image.)

Within the hook root, create subdirectories *early-setup.d*, *setup.d*, *load-key.d*, *boot-sel.d*
or *teardown.d* to hold hooks for the respective stages of hook execution. ZFSBootMenu will mount
the device named by the hook specification, look for the individual hook directories, and copy any
files found therein into its own memory-backed root filesystem. The copy is not recursive and
further subdirectorie are ignored. Note that, because ZFSBootMenu copies these scripts into its
standard hook paths at each boot, it is possible to "mask" a script explicitly included in the
ZFSBootMenu image by including an external hook script with the same name in the appropriate
directory.

In images with the Rust runtime, on a machine with a TPM, the hooks copied from the hook root are
measured into PCR 9 before any hook runs. A LUKS2 TPM2 token bound to PCR 9 then unseals only with
the hooks it was enrolled with; see :doc:`zfsbootmenu-luks(7) </man/zfsbootmenu-luks.7>`.
"#;

/// `zbm.kcl_override`.
pub static KCL_OVERRIDE: OptionSpec = OptionSpec {
    name: "zbm.kcl_override",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "A kernel command line for every boot environment, replacing theirs",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: r#"**zbm.kcl_override="boot environment KCL"**"#,
        body: KCL_OVERRIDE_DOC,
    },
    deprecation: None,
    examples: &[r#"zbm.kcl_override="some alternate set='of arguments'""#],
};

const KCL_OVERRIDE_DOC: &str = r#"
Override the kernel command line passed in to all boot environments. Double quotes must be used to
encapsulate the value of this argument. Arguments that need spaces should be enclosed with single
quotes. *root* is always removed. *spl_hostid* and *spl.spl_hostid* are removed if the
default-enabled option *zbm.set_hostid* is set.

.. code-block::

  zbm.kcl_override="some alternate set='of arguments'"
"#;

/// `zbm.skip_hooks`: hooks not to run, by file name.
pub static SKIP_HOOKS: OptionSpec = OptionSpec {
    name: "zbm.skip_hooks",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "Hooks not to run, by file name",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.skip_hooks=<hooklist>**",
        body: SKIP_HOOKS_DOC,
    },
    deprecation: None,
    examples: &["zbm.skip_hooks=90-xhci-unbind.sh"],
};

const SKIP_HOOKS_DOC: &str = r"
Skip execution of any early-setup, setup, load-key, boot-selection or teardown hooks with file
names matching any entry in the comma-separated list *hooklist*. Only base names of hooks (*i.e.*,
with any other path component removed) are matched against the *hooklist*.

**NOTE**: The *hooklist* argument **MUST NOT** contain spaces and **MUST NOT** be enclosed in
quotes.
";

/// `zbm.wait_for`: devices to wait for before importing pools.
pub static WAIT_FOR: OptionSpec = OptionSpec {
    name: "zbm.wait_for",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "Devices to wait for before importing pools",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.wait_for=device,device,...**",
        body: WAIT_FOR_DOC,
    },
    deprecation: None,
    examples: &[
        "zbm.wait_for=/dev/disk/by-id/wwn-0x500a07510f1b2c3d",
        "zbm.wait_for=UUID=1234-5678,LABEL=zfs0",
    ],
};

const WAIT_FOR_DOC: &str = r"
Ensure that one or more devices are present before starting the pool import process. Devices may
be specified as full paths to device nodes (*e.g.*, **/dev/sda** or
**/dev/disk/by-id/wwn-0x500a07510ee65912**) or, for convenience, as a typed indicator of the form
**TYPE=VALUE**, which will be expanded internally as

  **/dev/disk/by-TYPE/VALUE**

The use of full device paths other than descendants of **/dev/disk/** is fragile and should be
avoided. The delay interval between device checks can be controlled by **zbm.retry_delay**.
";

/// `root`: the pool to import first, as `root=zfsbootmenu:POOL=<pool>`.
pub static ROOT: OptionSpec = OptionSpec {
    name: "root",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "The pool to import first, in the form root=zfsbootmenu:POOL=<pool>",
    reader: Reader::Other("nothing: the form is no longer read"),
    doc: Doc::Within("zbm.prefer"),
    deprecation: Some(Deprecation {
        heading: "**root=zfsbootmenu:POOL=<pool>**",
        instead: "use **zbm.prefer**",
    }),
    examples: &["root=zfsbootmenu:POOL=zroot"],
};
