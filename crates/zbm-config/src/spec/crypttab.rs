//! Options of crypttab entries, documented in `zfsbootmenu-crypttab(5)`.

use super::{Alias, Doc, OptionSpec, Reader, Source, Value};

/// The options, in the order of the manual page.
pub static ALL: &[&OptionSpec] = &[
    &DISCARD,
    &READONLY,
    &TRIES,
    &NOAUTO,
    &NOFAIL,
    &KEYSCRIPT,
    &HEADER,
    &KEYFILE_OFFSET,
    &KEYFILE_SIZE,
    &KEY_SLOT,
    &GPG,
    &NO_TOKENS,
    &YKCHALRESP,
    &YKCHALRESP_HASH,
    &YKCHALRESP_CONCAT,
    &NO_READ_WORKQUEUE,
    &NO_WRITE_WORKQUEUE,
    &SAME_CPU_CRYPT,
    &SUBMIT_FROM_CRYPT_CPUS,
    &IGNORED,
];

/// `discard`: passes TRIM requests through to the device.
pub static DISCARD: OptionSpec = OptionSpec {
    name: "discard",
    aliases: &[Alias {
        name: "allow-discards",
        deprecation: None,
    }],
    source: Source::Crypttab,
    value: Value::Flag,
    default: None,
    summary: "Passes TRIM requests through to the device",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**discard**, **allow-discards**",
        body: DISCARD_DOC,
    },
    deprecation: None,
    examples: &["discard", "allow-discards"],
};

const DISCARD_DOC: &str = r#"
Pass TRIM requests through to the device (**cryptsetup --allow-discards**).
"#;

/// `readonly`: opens the volume read-only.
pub static READONLY: OptionSpec = OptionSpec {
    name: "readonly",
    aliases: &[Alias {
        name: "read-only",
        deprecation: None,
    }],
    source: Source::Crypttab,
    value: Value::Flag,
    default: None,
    summary: "Opens the volume read-only",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**readonly**, **read-only**",
        body: READONLY_DOC,
    },
    deprecation: None,
    examples: &["readonly", "read-only"],
};

const READONLY_DOC: &str = r#"
Open the mapping read-only.
"#;

/// `tries`: passphrase attempts per round; 0 for no limit.
pub static TRIES: OptionSpec = OptionSpec {
    name: "tries",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Integer,
    default: Some("3"),
    summary: "Passphrase attempts per round; 0 for no limit",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: r#"**tries=**\ *n*"#,
        body: TRIES_DOC,
    },
    deprecation: None,
    examples: &["tries=5", "tries=0"],
};

const TRIES_DOC: &str = r#"
Passphrase attempts per round before ZFSBootMenu offers to try again or skip the volume. The
default is 3; **0** means unlimited. An invalid value is ignored with a warning.
"#;

/// `noauto`: not unlocked at start-up, only with zluks.
pub static NOAUTO: OptionSpec = OptionSpec {
    name: "noauto",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Flag,
    default: None,
    summary: "Not unlocked at start-up, only with zluks",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**noauto**",
        body: NOAUTO_DOC,
    },
    deprecation: None,
    examples: &["noauto"],
};

const NOAUTO_DOC: &str = r#"
Do not unlock the volume at start-up. It can still be opened with **zluks open**.
"#;

/// `nofail`: A missing device is no reason to wait.
pub static NOFAIL: OptionSpec = OptionSpec {
    name: "nofail",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Flag,
    default: None,
    summary: "A missing device is no reason to wait",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**nofail**",
        body: NOFAIL_DOC,
    },
    deprecation: None,
    examples: &["nofail"],
};

const NOFAIL_DOC: &str = r#"
Do not wait or prompt when the device is missing.
"#;

/// `keyscript`: A program that prints the key.
pub static KEYSCRIPT: OptionSpec = OptionSpec {
    name: "keyscript",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Text,
    default: None,
    summary: "A program that prints the key",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: r#"**keyscript=**\ *path*"#,
        body: KEYSCRIPT_DOC,
    },
    deprecation: None,
    examples: &[
        "keyscript=/etc/zfsbootmenu/getkey",
        "keyscript=decrypt_gnupg",
    ],
};

const KEYSCRIPT_DOC: &str = r#"
Run *path* and use its standard output as the key. A name without a slash refers to
*/lib/cryptsetup/scripts/*\ *name*. The script receives the *key* field as its first argument and
the environment variables **CRYPTTAB_NAME**, **CRYPTTAB_SOURCE**, **CRYPTTAB_KEY**,
**CRYPTTAB_OPTIONS** and **CRYPTTAB_TRIED**. Key scripts named in the crypttab are installed in
the image with their library dependencies.
"#;

/// `header`: A detached LUKS header.
pub static HEADER: OptionSpec = OptionSpec {
    name: "header",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Text,
    default: None,
    summary: "A detached LUKS header",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: r#"**header=**\ *path*"#,
        body: HEADER_DOC,
    },
    deprecation: None,
    examples: &["header=/etc/zfsbootmenu/zroot.hdr"],
};

const HEADER_DOC: &str = r#"
Use a detached LUKS header.
"#;

/// `keyfile-offset`: bytes of the key file to skip.
pub static KEYFILE_OFFSET: OptionSpec = OptionSpec {
    name: "keyfile-offset",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Integer,
    default: None,
    summary: "Bytes of the key file to skip",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: r#"**keyfile-offset=**\ *n*"#,
        body: KEYFILE_OFFSET_DOC,
    },
    deprecation: None,
    examples: &["keyfile-offset=512"],
};

const KEYFILE_OFFSET_DOC: &str = r#"
Skip *n* bytes at the start of the key file.
"#;

/// `keyfile-size`: bytes of the key file to read.
pub static KEYFILE_SIZE: OptionSpec = OptionSpec {
    name: "keyfile-size",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Integer,
    default: None,
    summary: "Bytes of the key file to read",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: r#"**keyfile-size=**\ *n*"#,
        body: KEYFILE_SIZE_DOC,
    },
    deprecation: None,
    examples: &["keyfile-size=64"],
};

const KEYFILE_SIZE_DOC: &str = r#"
Read at most *n* bytes of the key file.
"#;

/// `key-slot`: the key slot to try.
pub static KEY_SLOT: OptionSpec = OptionSpec {
    name: "key-slot",
    aliases: &[Alias {
        name: "keyslot",
        deprecation: None,
    }],
    source: Source::Crypttab,
    value: Value::Integer,
    default: None,
    summary: "The key slot to try",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: r#"**key-slot=**\ *n*, **keyslot=**\ *n*"#,
        body: KEY_SLOT_DOC,
    },
    deprecation: None,
    examples: &["key-slot=1", "keyslot=2"],
};

const KEY_SLOT_DOC: &str = r#"
Try only key slot *n*.

The values of **keyfile-offset**, **keyfile-size** and **key-slot** must be non-negative integers.
Other values are ignored with a warning, because they would make every attempt fail, the
passphrase included.
"#;

/// `gpg`: the key file is GnuPG-encrypted, whatever its name.
pub static GPG: OptionSpec = OptionSpec {
    name: "gpg",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Flag,
    default: None,
    summary: "The key file is GnuPG-encrypted, whatever its name",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**gpg**",
        body: GPG_DOC,
    },
    deprecation: None,
    examples: &["gpg"],
};

const GPG_DOC: &str = r#"
The key file is GnuPG-encrypted, whatever its name.
"#;

/// `no-tokens`: LUKS2 tokens are not tried.
pub static NO_TOKENS: OptionSpec = OptionSpec {
    name: "no-tokens",
    aliases: &[
        Alias {
            name: "notoken",
            deprecation: None,
        },
        Alias {
            name: "no-token",
            deprecation: None,
        },
    ],
    source: Source::Crypttab,
    value: Value::Flag,
    default: None,
    summary: "LUKS2 tokens are not tried",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**no-tokens**, **notoken**, **no-token**",
        body: NO_TOKENS_DOC,
    },
    deprecation: None,
    examples: &["no-tokens", "notoken"],
};

const NO_TOKENS_DOC: &str = r#"
Do not try the volume's LUKS2 tokens (TPM2, FIDO2, clevis).
"#;

/// `ykchalresp`: yubiKey challenge-response, with the slot (2 by default).
pub static YKCHALRESP: OptionSpec = OptionSpec {
    name: "ykchalresp",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::FlagOrChoice(&["1", "2"]),
    default: None,
    summary: "YubiKey challenge-response, with the slot (2 by default)",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: r#"**ykchalresp**, **ykchalresp=**\ *slot*"#,
        body: YKCHALRESP_DOC,
    },
    deprecation: None,
    examples: &["ykchalresp", "ykchalresp=1"],
};

const YKCHALRESP_DOC: &str = r#"
Unlock with YubiKey HMAC-SHA1 challenge-response, using *slot* 1 or 2 (default 2). The typed
passphrase is the challenge and the response is the LUKS passphrase, as with **yubikey-luks**.
"#;

/// `ykchalresp-hash`: the challenge is the SHA-256 of the passphrase.
pub static YKCHALRESP_HASH: OptionSpec = OptionSpec {
    name: "ykchalresp-hash",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Flag,
    default: None,
    summary: "The challenge is the SHA-256 of the passphrase",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**ykchalresp-hash**",
        body: YKCHALRESP_HASH_DOC,
    },
    deprecation: None,
    examples: &["ykchalresp-hash"],
};

const YKCHALRESP_HASH_DOC: &str = r#"
Send the SHA-256 of the passphrase, in hexadecimal, as the challenge (**yubikey-luks** *HASH=1*).
Implies **ykchalresp**.
"#;

/// `ykchalresp-concat`.
pub static YKCHALRESP_CONCAT: OptionSpec = OptionSpec {
    name: "ykchalresp-concat",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Flag,
    default: None,
    summary: "The passphrase is the challenge followed by the response",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**ykchalresp-concat**",
        body: YKCHALRESP_CONCAT_DOC,
    },
    deprecation: None,
    examples: &["ykchalresp-concat"],
};

const YKCHALRESP_CONCAT_DOC: &str = r#"
Use the challenge followed by the response as the LUKS passphrase (**yubikey-luks**
*CONCATENATE=1*). Implies **ykchalresp**.
"#;

/// `no-read-workqueue`: A dm-crypt performance flag.
pub static NO_READ_WORKQUEUE: OptionSpec = OptionSpec {
    name: "no-read-workqueue",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Flag,
    default: None,
    summary: "A dm-crypt performance flag",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: concat!(
            "**no-read-workqueue**, **no-write-workqueue**, **same-cpu-crypt**, ",
            "**submit-from-crypt-cpus**"
        ),
        body: NO_READ_WORKQUEUE_DOC,
    },
    deprecation: None,
    examples: &["no-read-workqueue"],
};

const NO_READ_WORKQUEUE_DOC: &str = r#"
dm-crypt performance flags, passed to **cryptsetup** as **--perf-**\ *flag*.
"#;

/// `no-write-workqueue`: A dm-crypt performance flag.
pub static NO_WRITE_WORKQUEUE: OptionSpec = OptionSpec {
    name: "no-write-workqueue",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Flag,
    default: None,
    summary: "A dm-crypt performance flag",
    reader: Reader::Rust,
    doc: Doc::Within("no-read-workqueue"),
    deprecation: None,
    examples: &["no-write-workqueue"],
};

/// `same-cpu-crypt`: A dm-crypt performance flag.
pub static SAME_CPU_CRYPT: OptionSpec = OptionSpec {
    name: "same-cpu-crypt",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Flag,
    default: None,
    summary: "A dm-crypt performance flag",
    reader: Reader::Rust,
    doc: Doc::Within("no-read-workqueue"),
    deprecation: None,
    examples: &["same-cpu-crypt"],
};

/// `submit-from-crypt-cpus`: A dm-crypt performance flag.
pub static SUBMIT_FROM_CRYPT_CPUS: OptionSpec = OptionSpec {
    name: "submit-from-crypt-cpus",
    aliases: &[],
    source: Source::Crypttab,
    value: Value::Flag,
    default: None,
    summary: "A dm-crypt performance flag",
    reader: Reader::Rust,
    doc: Doc::Within("no-read-workqueue"),
    deprecation: None,
    examples: &["submit-from-crypt-cpus"],
};

/// `luks`: options of other crypttab readers, accepted and ignored.
pub static IGNORED: OptionSpec = OptionSpec {
    name: "luks",
    aliases: &[
        Alias {
            name: "luks1",
            deprecation: None,
        },
        Alias {
            name: "luks2",
            deprecation: None,
        },
        Alias {
            name: "initramfs",
            deprecation: None,
        },
        Alias {
            name: "x-initrd.attach",
            deprecation: None,
        },
        Alias {
            name: "_netdev",
            deprecation: None,
        },
        Alias {
            name: "noearly",
            deprecation: None,
        },
        Alias {
            name: "force",
            deprecation: None,
        },
    ],
    source: Source::Crypttab,
    value: Value::Flag,
    default: None,
    summary: "Options of other crypttab readers, accepted and ignored",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: concat!(
            "**luks**, **luks1**, **luks2**, **initramfs**, **x-initrd.attach**, **_netdev**, ",
            "**noearly**, **force**"
        ),
        body: IGNORED_DOC,
    },
    deprecation: None,
    examples: &["luks", "initramfs", "_netdev"],
};

const IGNORED_DOC: &str = r#"
Accepted for compatibility with other crypttab files and ignored.
"#;
