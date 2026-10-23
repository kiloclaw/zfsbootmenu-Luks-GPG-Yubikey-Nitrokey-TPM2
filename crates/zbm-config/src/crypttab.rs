//! Which volumes to unlock, and how: the crypttab, its options, and the
//! command line option `zbm.luks=<off|auto|device,device,...>`.
//!
//! The volumes come, in order of precedence, from `zbm.luks`, from a
//! crypttab(5)-style file in the image, or from discovering every LUKS
//! volume ([`load_config`]). Each crypttab entry has options in the crypttab
//! syntax, which [`Options::parse`] reads by their specifications in the
//! [table of options](crate::spec::crypttab): an option the table does not
//! have is ignored and logged.

use zbm_core::shell::test_integer;
use zbm_trace::Message;

use crate::spec::{self, Source};

/// The number of passphrase attempts per round when the options name none
/// (`LUKS_DEFAULT_TRIES`).
pub const DEFAULT_TRIES: u64 = 3;

/// Where the image keeps its crypttab (`ZBM_CRYPTTAB`).
pub const CRYPTTAB: &str = "/etc/zfsbootmenu/crypttab";

/// Where key scripts named without a path are (Debian's stock scripts).
const KEYSCRIPTS: &str = "/lib/cryptsetup/scripts/";

/// YubiKey challenge-response settings of a volume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YubiKey {
    /// The slot that answers the challenge, 1 or 2.
    pub slot: u8,
    /// Whether the challenge is the SHA-256 of the passphrase.
    pub hash: bool,
    /// Whether the LUKS passphrase is the challenge followed by the response.
    pub concat: bool,
}

/// The options of a volume, as the legacy runtime keeps them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Options {
    /// Arguments for `cryptsetup open` (`_luks_args`).
    pub args: Vec<String>,
    /// Passphrase attempts per round; 0 means without limit.
    pub tries: u64,
    /// Not unlocked at start-up (`noauto`).
    pub noauto: bool,
    /// A missing device is no reason to wait (`nofail`).
    pub nofail: bool,
    /// The program that prints the key (`keyscript=`).
    pub keyscript: Option<String>,
    /// A detached LUKS header (`header=`).
    pub header: Option<String>,
    /// The key file is GnuPG-encrypted whatever its name (`gpg`).
    pub gpg: bool,
    /// LUKS2 tokens are not tried (`no-tokens`).
    pub notoken: bool,
    /// YubiKey challenge-response (`ykchalresp`).
    pub yubikey: Option<YubiKey>,
}

impl Options {
    /// The options of a volume without any, with `tries` attempts.
    pub fn new(tries: u64) -> Options {
        Options {
            args: Vec::new(),
            tries,
            noauto: false,
            nofail: false,
            keyscript: None,
            header: None,
            gpg: false,
            notoken: false,
            yubikey: None,
        }
    }

    /// Parses a comma-separated crypttab option list, as the legacy runtime
    /// does, with `tries` attempts unless the list names another number. The
    /// messages are the warnings about invalid values and the debugging
    /// output about options ZFSBootMenu does not support.
    pub fn parse(text: &str, tries: u64) -> (Options, Vec<Message>) {
        let mut options = Options::new(tries);
        let mut messages = Vec::new();
        if text.is_empty() || text == "-" {
            return (options, messages);
        }
        for option in text.split(',') {
            options.apply(option, &mut messages);
        }
        (options, messages)
    }

    fn yubikey(&mut self) -> &mut YubiKey {
        self.yubikey.get_or_insert(YubiKey {
            slot: 2,
            hash: false,
            concat: false,
        })
    }

    fn apply(&mut self, option: &str, messages: &mut Vec<Message>) {
        let invalid = |messages: &mut Vec<Message>| {
            messages.push(Message::warning(format!(
                "ignoring invalid LUKS option '{option}'"
            )));
        };
        let (key, value) = match option.split_once('=') {
            Some((key, value)) => (key, Some(value)),
            None => (option, None),
        };
        if key.is_empty() && value.is_none() {
            return;
        }
        // Options are matched by the name of their specification; aliases
        // and the options themselves are in the table.
        let name = spec::find(Source::Crypttab, key).map_or("", |spec| spec.name);
        match (name, value) {
            ("discard", None) => self.args.push("--allow-discards".to_owned()),
            ("readonly", None) => self.args.push("--readonly".to_owned()),
            ("tries", Some(value)) => match test_integer(value) {
                // `[ "${value}" -ge 0 ]`
                Some(tries) if tries >= 0 => self.tries = tries.unsigned_abs(),
                _ => invalid(messages),
            },
            ("noauto", None) => self.noauto = true,
            ("nofail", None) => self.nofail = true,
            ("keyscript", Some(value)) => {
                // Like Debian, bare names refer to the stock cryptsetup scripts.
                let path = if value.starts_with('/') {
                    value.to_owned()
                } else {
                    format!("{KEYSCRIPTS}{value}")
                };
                self.keyscript = Some(path);
            }
            ("header", Some(value)) => {
                self.header = Some(value.to_owned());
                self.args.extend(["--header".to_owned(), value.to_owned()]);
            }
            ("keyfile-offset" | "keyfile-size" | "key-slot", Some(value)) => {
                // A bad value would make every attempt fail, the passphrase
                // included.
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    invalid(messages);
                } else {
                    self.args.extend([format!("--{name}"), value.to_owned()]);
                }
            }
            ("gpg", None) => self.gpg = true,
            ("no-tokens", None) => self.notoken = true,
            ("ykchalresp", None) => {
                self.yubikey();
            }
            ("ykchalresp", Some(value)) => {
                let yubikey = self.yubikey();
                match value {
                    "1" => yubikey.slot = 1,
                    "2" => yubikey.slot = 2,
                    _ => messages.push(Message::warning(format!(
                        "ignoring invalid YubiKey slot in '{option}'"
                    ))),
                }
            }
            ("ykchalresp-hash", None) => self.yubikey().hash = true,
            ("ykchalresp-concat", None) => self.yubikey().concat = true,
            ("no-read-workqueue", None) => self.args.push("--perf-no_read_workqueue".to_owned()),
            ("no-write-workqueue", None) => {
                self.args.push("--perf-no_write_workqueue".to_owned());
            }
            ("same-cpu-crypt", None) => self.args.push("--perf-same_cpu_crypt".to_owned()),
            ("submit-from-crypt-cpus", None) => {
                self.args.push("--perf-submit_from_crypt_cpus".to_owned());
            }
            ("luks", None) => {
                // Meaningful to other crypttab consumers, not to ZFSBootMenu.
            }
            _ => messages.push(Message::debug(format!(
                "ignoring unsupported LUKS option '{option}'"
            ))),
        }
    }
}

/// A volume to unlock.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// The mapping name; empty for the default, `luks-<UUID>`.
    pub name: String,
    /// The device specification, which `zbm_luks::device::resolve_device`
    /// resolves.
    pub spec: String,
    /// The key file specification, `path[:keydevice]`; empty for none.
    pub keyfile: String,
    /// The crypttab options.
    pub options: String,
}

impl Entry {
    /// A volume named on the command line: a device and nothing else.
    pub fn device(spec: &str) -> Entry {
        Entry {
            name: String::new(),
            spec: spec.to_owned(),
            keyfile: String::new(),
            options: String::new(),
        }
    }
}

/// Where the list of volumes comes from (`_luks_mode`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Mode {
    /// `zbm.luks=off`: nothing is unlocked.
    Disabled,
    /// Every LUKS volume that may hold ZFS.
    Auto,
    /// The entries of the crypttab.
    Crypttab(Vec<Entry>),
    /// The devices `zbm.luks` lists.
    CommandLine(Vec<Entry>),
}

impl Mode {
    /// The name the legacy code logs.
    pub fn name(&self) -> &'static str {
        match self {
            Mode::Disabled => "disabled",
            Mode::Auto => "auto",
            Mode::Crypttab(_) => "crypttab",
            Mode::CommandLine(_) => "cmdline",
        }
    }

    /// The configured volumes; none in automatic mode.
    pub fn entries(&self) -> &[Entry] {
        match self {
            Mode::Crypttab(entries) | Mode::CommandLine(entries) => entries,
            Mode::Disabled | Mode::Auto => &[],
        }
    }
}

/// The fields of a crypttab line, as `read -r name device keyfile options _`
/// splits it: at runs of spaces and tabs, with the fifth field and the rest
/// left out.
fn crypttab_fields(line: &str) -> [&str; 4] {
    let mut fields = line.split([' ', '\t']).filter(|field| !field.is_empty());
    [(); 4].map(|()| fields.next().unwrap_or_default())
}

/// Parses a crypttab as the legacy runtime reads it; `path` names it in
/// warnings.
pub fn parse_crypttab(text: &str, path: &str) -> (Vec<Entry>, Vec<Message>) {
    let mut entries = Vec::new();
    let mut messages = Vec::new();
    // The last line may lack a newline; files edited on Windows end in CRLF.
    let text = text.strip_suffix('\n').unwrap_or(text);
    if text.is_empty() {
        return (entries, messages);
    }
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let [name, device, keyfile, options] = crypttab_fields(line);
        if name.is_empty() || name.starts_with('#') {
            continue;
        }
        if device.is_empty() {
            messages.push(Message::warning(format!(
                "ignoring malformed entry '{name}' in {path}"
            )));
            continue;
        }
        let keyfile = match keyfile {
            "-" | "none" => "",
            keyfile => keyfile,
        };
        entries.push(Entry {
            name: name.to_owned(),
            spec: device.to_owned(),
            keyfile: keyfile.to_owned(),
            options: options.to_owned(),
        });
    }
    (entries, messages)
}

/// Decides which volumes to unlock, as the legacy runtime does, from the
/// value of `zbm.luks` (if the command line has it) and the crypttab at
/// `path` (if it can be read).
pub fn load_config(
    zbm_luks: Option<&str>,
    crypttab: Option<&str>,
    path: &str,
) -> (Mode, Vec<Message>) {
    let mut messages = Vec::new();
    let mode = if let Some(arg) = zbm_luks {
        match arg.to_lowercase().as_str() {
            "off" | "no" | "none" | "0" | "false" | "disable" | "disabled" => {
                messages.push(Message::info("LUKS unlocking disabled on the command line"));
                return (Mode::Disabled, messages);
            }
            "" | "auto" | "on" | "yes" | "1" | "true" => Mode::Auto,
            _ => Mode::CommandLine(
                arg.split(',')
                    .filter(|spec| !spec.is_empty())
                    .map(Entry::device)
                    .collect(),
            ),
        }
    } else if let Some(text) = crypttab {
        let (entries, warnings) = parse_crypttab(text, path);
        messages.extend(warnings);
        if entries.is_empty() {
            Mode::Auto
        } else {
            Mode::Crypttab(entries)
        }
    } else {
        Mode::Auto
    };
    messages.push(Message::debug(format!(
        "LUKS configuration mode: {}",
        mode.name()
    )));
    (mode, messages)
}

#[cfg(test)]
mod tests;
