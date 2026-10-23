use std::fmt::Write as _;

use super::*;
use crate::args::{Args, CmdlineDir};
use zbm_testkit::legacy::{Bash, LOG_STUBS, log_lines};
use zbm_testkit::parity::Parity;
use zbm_testkit::{Rng, TempDir, parity};

/// The legacy LUKS library, loaded as the unit suites load it, with messages
/// on standard error as `LEVEL: text`.
fn legacy_luks() -> Bash {
    Bash::new()
        .code(LOG_STUBS)
        .code("_ZFSBOOTMENU_CORE=1")
        .code(r#"LUKS_KEYS_LIB="$LEGACY/zfsbootmenu/lib/zfsbootmenu-luks-keys.sh""#)
        .source("zfsbootmenu/lib/zfsbootmenu-kcl.sh")
        .source("zfsbootmenu/lib/zfsbootmenu-luks.sh")
}

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
            .map(|message| message.stderr_line() + "\n")
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
        let _ = writeln!(
            text,
            "[{}] [{}] [{}] [{}]",
            entry.name, entry.spec, entry.keyfile, entry.options
        );
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
    let files = zbm_platform::files::RealFiles;
    let base = dir.join("base");
    let arg =
        CmdlineDir::new(&files, &base).lookup(&crate::spec::cmdline::luks::LUKS, &mut Vec::new());
    let (mode, messages) = load_config(arg.as_deref(), text.as_deref(), &path);
    assert_eq!(
        show_mode(&mode).replace(&path, "$CRYPTTAB"),
        shown,
        "{case}"
    );
    let rust: String = messages
        .iter()
        .map(|message| message.stderr_line() + "\n")
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
