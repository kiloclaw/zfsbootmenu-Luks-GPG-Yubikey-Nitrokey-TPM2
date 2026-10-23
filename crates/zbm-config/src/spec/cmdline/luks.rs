//! Options of the kernel command line for LUKS volumes, documented in
//! `zfsbootmenu(7)` and `zfsbootmenu-luks(7)`.

use crate::spec::{Doc, OptionSpec, Reader, Source, Value};

/// The options, in the order of the manual page.
pub static ALL: &[&OptionSpec] = &[&LUKS, &LUKS_KEY, &LUKS_TOKENS, &LUKS_TOKEN_WAIT];

/// `zbm.luks`: which LUKS volumes are unlocked: off, auto, or a device list.
pub static LUKS: OptionSpec = OptionSpec {
    name: "zbm.luks",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "Which LUKS volumes are unlocked: off, auto, or a device list",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.luks=<off|auto|device,device,...>**",
        body: LUKS_DOC,
    },
    deprecation: None,
    examples: &[
        "zbm.luks=off",
        "zbm.luks=auto",
        "zbm.luks=PARTLABEL=zfs0,PARTLABEL=zfs1",
    ],
};

const LUKS_DOC: &str = r"
Control the unlocking of LUKS (dm-crypt) volumes that hold ZFS vdevs, which happens after
early-setup hooks and before pools are imported.

*off*

  Do not unlock any LUKS volumes.

*auto*

  Try every LUKS volume on the system, closing again any whose contents are recognized as
  something other than a ZFS vdev. This is the default when the image contains no LUKS crypttab.

*device,device,...*

  Unlock only the listed devices. Each device may be a path below **/dev**, a typed indicator of
  the form **UUID=**, **PARTUUID=**, **LABEL=**, **PARTLABEL=**, **ID=** or **PATH=**, or a bare
  LUKS UUID.

When this option is absent, the crypttab embedded with **zfsbootmenu_luks_crypttab** is used if it
exists; otherwise *auto* applies. A passphrase that unlocks one volume is tried on the others
before prompting again, and an empty passphrase skips a volume. See the *ZFS Pools on LUKS
Volumes* guide for details.
";

/// `zbm.luks_key`: A key file for LUKS volumes that have none.
pub static LUKS_KEY: OptionSpec = OptionSpec {
    name: "zbm.luks_key",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "A key file for LUKS volumes that have none",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.luks_key=<path>[:<device>]**",
        body: LUKS_KEY_DOC,
    },
    deprecation: None,
    examples: &["zbm.luks_key=/keys/zroot.key:LABEL=ZBMKEY"],
};

const LUKS_KEY_DOC: &str = r"
A key file used to unlock any LUKS volume that has no key file of its own. With **:<device>**
(specified as for **zbm.luks**), the device is mounted read-only and **<path>** is read relative
to its root. ZFSBootMenu falls back to a passphrase prompt if the key is unavailable or does not
work. Key files named *\*.gpg*, *\*.asc* or *\*.pgp* are decrypted with GnuPG first, using a
passphrase or an OpenPGP smart card.
";

/// `zbm.luks_tokens`: whether LUKS2 tokens (TPM2, FIDO2, clevis) are used.
pub static LUKS_TOKENS: OptionSpec = OptionSpec {
    name: "zbm.luks_tokens",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Bool,
    default: Some("1"),
    summary: "Whether LUKS2 tokens (TPM2, FIDO2, clevis) are used",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.luks_tokens=0**",
        body: LUKS_TOKENS_DOC,
    },
    deprecation: None,
    examples: &["zbm.luks_tokens=0"],
};

const LUKS_TOKENS_DOC: &str = r"
Do not use LUKS2 tokens. By default, ZFSBootMenu unlocks volumes with TPM2 and FIDO2 tokens
enrolled by **systemd-cryptenroll** and with clevis tokens before asking for a passphrase.
";

/// `zbm.luks_token_wait`: seconds to wait for a security key to be plugged in.
pub static LUKS_TOKEN_WAIT: OptionSpec = OptionSpec {
    name: "zbm.luks_token_wait",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Integer,
    default: Some("10"),
    summary: "Seconds to wait for a security key to be plugged in",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.luks_token_wait=<seconds>**",
        body: LUKS_TOKEN_WAIT_DOC,
    },
    deprecation: None,
    examples: &["zbm.luks_token_wait=30"],
};

const LUKS_TOKEN_WAIT_DOC: &str = r"
How long to wait for a FIDO2 security key, OpenPGP smart card or YubiKey to be plugged in before
moving on to other unlock methods (default: 10). Pressing any key skips the wait.

See :doc:`zfsbootmenu-luks(7) </man/zfsbootmenu-luks.7>` for a complete description of LUKS
support.
";
