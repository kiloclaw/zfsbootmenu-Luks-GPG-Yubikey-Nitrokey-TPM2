//! Which volumes to unlock, and how: `luks_load_config` and
//! `luks_parse_options`.
//!
//! The volumes come, in order of precedence, from the command line option
//! `zbm.luks=<off|auto|device,device,...>`, from a crypttab(5)-style file in
//! the image, or from discovering every LUKS volume. Each crypttab entry has
//! options in the crypttab syntax, which [`Options::parse`] reads.

use zbm_core::shell::test_integer;
use zbm_trace::Message;

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

/// The options of a volume, as `luks_parse_options` sets its variables.
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

    /// Parses a comma-separated crypttab option list, as
    /// `luks_parse_options` does, with `tries` attempts unless the list names
    /// another number. The messages are the warnings about invalid values and
    /// the debugging output about options ZFSBootMenu does not support.
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
        match (key, value) {
            ("", None) => {}
            ("discard" | "allow-discards", None) => self.args.push("--allow-discards".to_owned()),
            ("readonly" | "read-only", None) => self.args.push("--readonly".to_owned()),
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
            ("keyfile-offset" | "keyfile-size" | "key-slot" | "keyslot", Some(value)) => {
                // A bad value would make every attempt fail, the passphrase
                // included.
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    invalid(messages);
                } else {
                    let name = if key == "keyslot" { "key-slot" } else { key };
                    self.args.extend([format!("--{name}"), value.to_owned()]);
                }
            }
            ("gpg", None) => self.gpg = true,
            ("no-tokens" | "notoken" | "no-token", None) => self.notoken = true,
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
            (
                "luks" | "luks1" | "luks2" | "initramfs" | "x-initrd.attach" | "_netdev"
                | "noearly" | "force",
                None,
            ) => {
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
    /// The device specification (see [`crate::device::resolve_device`]).
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

/// Parses a crypttab as `luks_load_config` reads it; `path` names it in
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

/// Decides which volumes to unlock, as `luks_load_config` does, from the
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
mod tests {
    use super::*;
    use crate::testing::legacy_luks;
    use zbm_testkit::legacy::log_lines;
    use zbm_testkit::parity::Parity;
    use zbm_testkit::{Rng, TempDir, parity};

    /// What the legacy variables hold after `luks_parse_options`, one line per
    /// variable, with the arguments separated by `|`.
    const SHOW_OPTIONS: &str = r#"show_options() {
        luks_parse_options "$1"
        local IFS='|'
        printf 'args=%s\n' "${_luks_args[*]}"
        printf '%s\n' "tries=${_luks_tries}" "noauto=${_luks_noauto}" \
          "nofail=${_luks_nofail}" "keyscript=${_luks_keyscript}" \
          "header=${_luks_header}" "gpg=${_luks_gpg}" "notoken=${_luks_notoken}" \
          "yk=${_luks_yk}" "yk_slot=${_luks_yk_slot}" "yk_hash=${_luks_yk_hash}" \
          "yk_concat=${_luks_yk_concat}"
      }"#;

    fn flag(on: bool) -> &'static str {
        if on { "1" } else { "" }
    }

    /// The Rust options in the format of [`SHOW_OPTIONS`]. The legacy code
    /// keeps the number of tries as text, so the comparison uses the value
    /// `test` sees.
    fn show(options: &Options) -> String {
        let yubikey = options.yubikey.as_ref();
        format!(
            "args={}\ntries={}\nnoauto={}\nnofail={}\nkeyscript={}\nheader={}\ngpg={}\n\
             notoken={}\nyk={}\nyk_slot={}\nyk_hash={}\nyk_concat={}\n",
            options.args.join("|"),
            options.tries,
            flag(options.noauto),
            flag(options.nofail),
            options.keyscript.as_deref().unwrap_or_default(),
            options.header.as_deref().unwrap_or_default(),
            flag(options.gpg),
            flag(options.notoken),
            flag(yubikey.is_some()),
            yubikey.map_or(2, |yubikey| yubikey.slot),
            flag(yubikey.is_some_and(|yubikey| yubikey.hash)),
            flag(yubikey.is_some_and(|yubikey| yubikey.concat)),
        )
    }

    /// Normalizes the legacy number of tries to its value.
    fn normalize(legacy: &str) -> String {
        legacy
            .lines()
            .map(|line| match line.strip_prefix("tries=") {
                Some(tries) => format!("tries={}\n", test_integer(tries).unwrap_or(-1)),
                None => format!("{line}\n"),
            })
            .collect()
    }

    fn assert_same_options(parity: &mut Parity, inputs: &[String]) {
        for input in inputs {
            let (shown, logged): (String, String) = parity.legacy(format!("{input:?}"), || {
                let legacy = legacy_luks()
                    .code(SHOW_OPTIONS)
                    .call("show_options", &[input], b"");
                (
                    normalize(&legacy.stdout_text()),
                    log_lines(&legacy.stderr_text()),
                )
            });
            let (options, messages) = Options::parse(input, DEFAULT_TRIES);
            assert_eq!(show(&options), shown, "{input:?}");
            let rust: String = messages
                .iter()
                .map(|message| format!("{}\n", message.stderr_line()))
                .collect();
            assert_eq!(rust, logged, "{input:?}");
        }
    }

    #[test]
    fn options_match_the_legacy_parser() {
        let inputs = [
            "",
            "-",
            "discard",
            "discard,no-read-workqueue,no-write-workqueue,same-cpu-crypt,submit-from-crypt-cpus",
            "allow-discards,readonly,read-only",
            "tries=0",
            "tries=5,tries=abc",
            "tries=08",
            "tries=+7",
            "tries=-1",
            "tries=-0",
            "tries=",
            "tries=99999999999999999999",
            "noauto,nofail",
            "keyscript=decrypt_gnupg",
            "keyscript=/etc/ks,keyscript=",
            "header=/boot/hdr,header=",
            "keyfile-offset=100,keyfile-size=64,key-slot=2,keyslot=03",
            "keyfile-offset=x,keyfile-size=-5,key-slot=,keyslot=1a",
            "gpg,no-tokens",
            "notoken",
            "no-token",
            "ykchalresp",
            "ykchalresp=1",
            "ykchalresp=3,ykchalresp-hash",
            "ykchalresp-concat",
            "ykchalresp=,ykchalresp-hash=1",
            "luks,luks1,luks2,initramfs,x-initrd.attach,_netdev,noearly,force",
            "luks=1,noauto=yes,discard=1,plain,swap,tmp=ext4",
            ",,discard,,",
            ",discard",
            "unknown option,with spaces",
            "keyscript=a=b,header=c=d",
            "\\,discard",
        ];
        let mut parity = parity!("options_match_the_legacy_parser");
        assert_same_options(&mut parity, &inputs.map(str::to_owned));
    }

    #[test]
    fn random_options_match_the_legacy_parser() {
        let mut rng = Rng::from_env(0x10c5);
        let pieces = [
            "discard",
            "tries=",
            "tries=2",
            "tries=x",
            "noauto",
            "nofail",
            "keyscript=",
            "keyscript=k",
            "header=/h",
            "keyfile-size=",
            "keyfile-size=9",
            "key-slot=1",
            "keyslot=",
            "gpg",
            "no-tokens",
            "ykchalresp",
            "ykchalresp=",
            "ykchalresp=1",
            "ykchalresp-hash",
            "ykchalresp-concat",
            "luks",
            "=",
            "-",
            "",
            "x",
            " ",
        ];
        let inputs: Vec<String> = (0..60)
            .map(|_| {
                let count = 1 + rng.below(5);
                (0..count)
                    .map(|_| *rng.choose(&pieces))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect();
        let mut parity = parity!("random_options_match_the_legacy_parser");
        assert_same_options(&mut parity, &inputs);
    }

    #[test]
    fn options_set_the_volume_up() {
        let (options, messages) = Options::parse(
            "discard,header=/hdr,keyslot=1,tries=0,ykchalresp=1,ykchalresp-concat,keyscript=ks",
            DEFAULT_TRIES,
        );
        assert!(messages.is_empty());
        assert_eq!(
            options.args,
            ["--allow-discards", "--header", "/hdr", "--key-slot", "1"]
        );
        assert_eq!(options.tries, 0);
        assert_eq!(options.header.as_deref(), Some("/hdr"));
        assert_eq!(
            options.keyscript.as_deref(),
            Some("/lib/cryptsetup/scripts/ks")
        );
        assert_eq!(
            options.yubikey,
            Some(YubiKey {
                slot: 1,
                hash: false,
                concat: true
            })
        );
        assert_eq!(Options::parse("", 5).0, Options::new(5));
    }

    /// What `luks_load_config` decides, in a line per entry after the mode.
    const SHOW_CONFIG: &str = r#"show_config() {
        luks_load_config; local rc=$? i
        echo "rc=${rc} mode=${_luks_mode}"
        for i in "${!_luks_cfg_spec[@]}"; do
          printf '[%s] [%s] [%s] [%s]\n' "${_luks_cfg_name[i]}" "${_luks_cfg_spec[i]}" \
            "${_luks_cfg_key[i]}" "${_luks_cfg_opts[i]}"
        done
      }"#;

    fn show_mode(mode: &Mode) -> String {
        let rc = u8::from(*mode == Mode::Disabled);
        let name = if *mode == Mode::Disabled {
            ""
        } else {
            mode.name()
        };
        let mut text = format!("rc={rc} mode={name}\n");
        for entry in mode.entries() {
            text.push_str(&format!(
                "[{}] [{}] [{}] [{}]\n",
                entry.name, entry.spec, entry.keyfile, entry.options
            ));
        }
        text
    }

    /// Compares the two for a `zbm.luks` file (its contents as written) and a
    /// crypttab (`None` when there is none).
    fn assert_same_config(parity: &mut Parity, zbm_luks: Option<&str>, crypttab: Option<&[u8]>) {
        // A base directory with the zbm.luks file, and the crypttab.
        let machine = || {
            let dir = TempDir::new("zbm-luks-config");
            dir.write("base/cmdline.d/.keep", "");
            if let Some(value) = zbm_luks {
                dir.write("base/cmdline.d/zbm.luks", value);
            }
            let path = dir.join("crypttab");
            if let Some(text) = crypttab {
                std::fs::write(&path, text).unwrap();
            }
            let path = path.to_string_lossy().into_owned();
            (dir, path)
        };
        let text = crypttab.map(String::from_utf8_lossy);
        let case = format!("{zbm_luks:?} {:?}", text.as_deref());
        let (shown, logged): (String, String) = parity.legacy(case.clone(), || {
            let (dir, path) = machine();
            let legacy = legacy_luks()
                .code(SHOW_CONFIG)
                .env("BASE", &dir.join("base").to_string_lossy())
                .env("ZBM_CRYPTTAB", &path)
                .call("show_config", &[], b"");
            (
                legacy.stdout_text().replace(&path, "$CRYPTTAB"),
                log_lines(&legacy.stderr_text()).replace(&path, "$CRYPTTAB"),
            )
        });

        let (dir, path) = machine();
        let files = zbm_system::files::RealFiles;
        let state = zbm_state::State::new(&files, dir.join("base"));
        let arg = state.zbm_arg("zbm.luks");
        let (mode, messages) = load_config(arg.as_deref(), text.as_deref(), &path);
        assert_eq!(
            show_mode(&mode).replace(&path, "$CRYPTTAB"),
            shown,
            "{case}"
        );
        let rust: String = messages
            .iter()
            .map(|message| format!("{}\n", message.stderr_line()))
            .collect();
        assert_eq!(rust.replace(&path, "$CRYPTTAB"), logged, "{case}");
    }

    #[test]
    fn command_line_modes_match_the_legacy_loader() {
        let mut parity = parity!("command_line_modes_match_the_legacy_loader");
        let tab: &[u8] = b"v /dev/sda none\n";
        for value in [
            "off\n",
            "OFF\n",
            "No\n",
            "none\n",
            "0\n",
            "false\n",
            "disable\n",
            "disabled\n",
            "",
            "\n",
            "auto\n",
            "On\n",
            "yes\n",
            "1\n",
            "TRUE\n",
            "  auto  \n",
        ] {
            assert_same_config(&mut parity, Some(value), Some(tab));
        }
        for value in [
            "UUID=a,PARTLABEL=b\n",
            ",,/dev/sda,,LABEL=x,\n",
            "luks-0f2c8bb6-2d5e-4f87-9a6b-9a7c1fef8d1b\n",
            "UUID=a b\n",
            "first\nsecond\n",
            "offline\n",
        ] {
            assert_same_config(&mut parity, Some(value), Some(tab));
            assert_same_config(&mut parity, Some(value), None);
        }
    }

    #[test]
    fn crypttabs_match_the_legacy_loader() {
        let tabs: [&[u8]; 13] = [
            b"",
            b"\n\n",
            b"# only a comment\n",
            b"v /dev/sda none",
            b"v /dev/sda none discard\r\nw /dev/sdb - noauto\r\n",
            b"\n# comment\n   \n\tv\t\t/dev/sda   none\t discard  \n# trailing\n",
            b"lonely\nv /dev/sda\n",
            b"a UUID=1 /key:LABEL=K tries=2 extra fields\n",
            b"  #indented comment\n#x y z\n",
            b"a b c d\r\r\n",
            b"a\rb c\n",
            b"a b \\ d\\e\n",
            b"\xff\xfe /dev/x\n",
        ];
        let mut parity = parity!("crypttabs_match_the_legacy_loader");
        for tab in tabs {
            assert_same_config(&mut parity, None, Some(tab));
        }
        assert_same_config(&mut parity, None, None);
    }

    #[test]
    fn modes_list_their_entries() {
        let (mode, _) = load_config(None, Some("a /dev/a none ro\n"), CRYPTTAB);
        assert_eq!(mode.name(), "crypttab");
        assert_eq!(
            mode.entries(),
            [Entry {
                name: "a".to_owned(),
                spec: "/dev/a".to_owned(),
                keyfile: String::new(),
                options: "ro".to_owned(),
            }]
        );
        let (mode, messages) = load_config(Some("x,y"), None, CRYPTTAB);
        assert_eq!(mode.entries(), [Entry::device("x"), Entry::device("y")]);
        assert_eq!(
            messages,
            [Message::debug("LUKS configuration mode: cmdline")]
        );
        assert!(
            load_config(Some("off"), None, CRYPTTAB)
                .0
                .entries()
                .is_empty()
        );
    }
}
