//! The `zbm.*` options and their defaults, as the boot image reads them at
//! start-up.
//!
//! [`ZbmOptions::parse`] does what `zfsbootmenu-parse-commandline.sh` does: it
//! reads the options from the command line, falls back to defaults for missing
//! and invalid values, and records the same log messages. The values keep the
//! text the legacy script exports to `/etc/zfsbootmenu.conf` (see
//! [`ZbmOptions::conf`]): a valid timeout of `+5` stays `+5`, as the shell
//! keeps it.

use zbm_core::shell::{quote, test_integer};
use zbm_log::Message;

use crate::args::ZbmArgs;
use crate::kcl;

/// The settings ZFSBootMenu takes from its command line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZbmOptions {
    /// `loglevel`: the kernel log level of ZFSBootMenu's messages; at least 4.
    pub loglevel: String,
    /// `spl.spl_hostid` or `spl_hostid`: the host ID as eight or more hex digits,
    /// `0`, or empty.
    pub spl_hostid: String,
    /// `console`: the device the menu runs on.
    pub control_term: String,
    /// `zbm.import_policy` (or `zbm.force_import`): `hostid`, `force` or `strict`.
    pub import_policy: String,
    /// `zbm.timeout`, `zbm.show` or `zbm.skip`: seconds before booting the default,
    /// `-1` to always show the menu.
    pub menu_timeout: String,
    /// `zbm.retry_delay` or `zbm.import_delay`: seconds between import and device
    /// retries.
    pub retry_delay: String,
    /// `zbm.lines`: the number of terminal lines, when given.
    pub lines: Option<String>,
    /// `zbm.columns`: the number of terminal columns, when given.
    pub columns: Option<String>,
    /// `zbm.sort_key`: the sort keys of the boot environment list, separated by
    /// `;`.
    pub sort: String,
    /// `zbm.hookroot`: a filesystem holding user hooks, or empty.
    pub hook_root: String,
    /// `zbm.set_hostid`: whether ZFSBootMenu replaces `spl_hostid` in boot
    /// environment command lines.
    pub set_hostid: bool,
    /// `zbm.kcl_override`: the tokenized command line that replaces those of all
    /// boot environments, as written to `${BASE}/cmdline`.
    pub kcl_override: Option<String>,
    /// `zbm.wait_for`: devices to wait for, or empty.
    pub wait_for_devices: String,
    /// `zbm.prefer`: the preferred pool, or empty.
    pub prefer_pool: String,
    /// `zbm.prefer` with `!` (`yes`) or `!!` (`only`): whether the preferred pool
    /// is required, or empty.
    pub require_pool: String,
    /// `zbm.prefer` naming a dataset: the boot environment to prefer, or empty.
    pub prefer_bootfs: String,
}

/// The lowest log level: warnings and errors are always logged.
const MIN_LOGLEVEL: i64 = 4;

/// The sort keys the menu supports, in their default order.
const SORT_KEYS: [&str; 3] = ["name", "creation", "used"];

impl ZbmOptions {
    /// Reads the options from the command line and returns them with the messages
    /// the legacy script logs, in order.
    pub fn parse(args: &ZbmArgs) -> (ZbmOptions, Vec<Message>) {
        let mut log = Vec::new();

        let loglevel = match args.lookup(&["loglevel"], &mut log) {
            Some(level) if test_integer(&level).is_some_and(|level| level >= MIN_LOGLEVEL) => {
                log.push(Message::info(format!(
                    "setting log level from command line: {level}"
                )));
                level
            }
            _ => MIN_LOGLEVEL.to_string(),
        };

        let spl_hostid = match args.lookup(&["spl.spl_hostid", "spl_hostid"], &mut log) {
            Some(value) => parse_spl_hostid(&value).unwrap_or_else(|| {
                log.push(Message::warning(format!(
                    "invalid hostid value {value}, ignoring"
                )));
                String::new()
            }),
            None => String::new(),
        };

        let control_term = match args.lookup(&["console"], &mut log) {
            Some(console) => {
                let device = console
                    .rsplit_once(',')
                    .map_or(console.as_str(), |(d, _)| d);
                let term = format!("/dev/{device}");
                log.push(Message::info(format!(
                    "setting controlling terminal to: {term}"
                )));
                term
            }
            None => {
                let term = "/dev/tty1".to_owned();
                log.push(Message::info(format!(
                    "defaulting controlling terminal to: {term}"
                )));
                term
            }
        };

        let import_policy = if let Some(policy) = args.lookup(&["zbm.import_policy"], &mut log) {
            match policy.as_str() {
                "hostid" => {
                    log.push(Message::info("setting import_policy to hostid matching"));
                    policy
                }
                "force" | "strict" => {
                    log.push(Message::info(format!("setting import_policy to {policy}")));
                    policy
                }
                _ => {
                    log.push(Message::warning(format!(
                        "unknown import policy '{policy}', defaulting to hostid"
                    )));
                    "hostid".to_owned()
                }
            }
        } else if args.flag("0", &["zbm.force_import", "force_import"], &mut log) {
            log.push(Message::info("setting import_policy to force"));
            "force".to_owned()
        } else {
            log.push(Message::info("defaulting import_policy to hostid"));
            "hostid".to_owned()
        };

        let menu_timeout = if let Some(timeout) = args.lookup(&["zbm.timeout", "timeout"], &mut log)
        {
            if test_integer(&timeout).is_some() {
                log.push(Message::info(format!(
                    "setting menu timeout from command line: {timeout}"
                )));
                timeout
            } else {
                log.push(Message::warning(format!(
                    "invalid menu timeout: '{timeout}', defaulting to 10 seconds"
                )));
                "10".to_owned()
            }
        } else if args.flag("0", &["zbm.show"], &mut log) {
            log.push(Message::info("forcing display of menu"));
            "-1".to_owned()
        } else if args.flag("0", &["zbm.skip"], &mut log) {
            log.push(Message::info("skipping display of menu"));
            "0".to_owned()
        } else {
            log.push(Message::info("defaulting menu timeout to 10"));
            "10".to_owned()
        };

        let retry_delay = match args.lookup(&["zbm.retry_delay", "zbm.import_delay"], &mut log) {
            // `[ "${zbm_retry_delay:-0}" -gt 0 ]`
            Some(delay)
                if test_integer(if delay.is_empty() { "0" } else { &delay })
                    .is_some_and(|delay| delay > 0) =>
            {
                log.push(Message::info(format!(
                    "import/waitfor retry delay is {delay} seconds"
                )));
                delay
            }
            _ => "5".to_owned(),
        };

        let lines = positive_or_default(args, "zbm.lines", "25", "lines", &mut log);
        let columns = positive_or_default(args, "zbm.columns", "80", "columns", &mut log);

        let sort = match args.lookup(&["zbm.sort_key"], &mut log) {
            Some(key) => {
                let first = SORT_KEYS
                    .iter()
                    .find(|&&valid| valid == key)
                    .copied()
                    .unwrap_or("name");
                let mut order = vec![first];
                order.extend(SORT_KEYS.iter().filter(|&&other| other != first));
                let sort = order.join(";");
                log.push(Message::info(format!("setting sort key order to {sort}")));
                sort
            }
            None => {
                let sort = SORT_KEYS.join(";");
                log.push(Message::info(format!(
                    "defaulting sort key order to {sort}"
                )));
                sort
            }
        };

        let hook_root = match args.lookup(&["zbm.hookroot"], &mut log) {
            Some(root) => {
                log.push(Message::info(format!("setting user hook root to {root}")));
                root
            }
            None => String::new(),
        };

        let set_hostid = args.flag("1", &["zbm.set_hostid"], &mut log);
        log.push(Message::info(if set_hostid {
            "enabling automatic replacement of spl_hostid"
        } else {
            "disabling automatic replacement of spl_hostid"
        }));

        let kcl_override = args.lookup(&["zbm.kcl_override"], &mut log).map(|value| {
            // Remove one leading and one trailing quote, then up-convert single quotes to
            // double quotes.
            let value = value.strip_prefix('"').unwrap_or(&value);
            let value = value.strip_suffix('"').unwrap_or(value).replace('\'', "\"");
            let mut removals = vec!["root"];
            if set_hostid {
                removals.extend(["spl_hostid", "spl.spl_hostid"]);
            }
            let text = kcl::suppress(&kcl::tokenize(&format!("{value}\n")), &removals);
            log.push(Message::info(format!(
                "overriding all BE KCLs with: '{}'",
                kcl::assemble(&text)
            )));
            text
        });

        let wait_for_devices = match args.lookup(&["zbm.wait_for"], &mut log) {
            Some(devices) => {
                log.push(Message::info(format!("system will wait for {devices}")));
                devices
            }
            None => String::new(),
        };

        let (mut prefer_pool, mut require_pool, mut prefer_bootfs) =
            (String::new(), String::new(), String::new());
        if let Some(prefer) = args.lookup(&["zbm.prefer"], &mut log) {
            let prefer = if let Some(pool) = prefer.strip_suffix("!!") {
                require_pool = "only".to_owned();
                log.push(Message::info(format!(
                    "will only attempt to import {}",
                    pool_of(pool)
                )));
                pool.to_owned()
            } else if let Some(pool) = prefer.strip_suffix('!') {
                require_pool = "yes".to_owned();
                log.push(Message::info(format!("requiring pool {}", pool_of(pool))));
                pool.to_owned()
            } else {
                prefer
            };
            prefer_pool = pool_of(&prefer).to_owned();
            if prefer_pool != prefer {
                prefer_bootfs = prefer;
            }
        }

        let options = ZbmOptions {
            loglevel,
            spl_hostid,
            control_term,
            import_policy,
            menu_timeout,
            retry_delay,
            lines,
            columns,
            sort,
            hook_root,
            set_hostid,
            kcl_override,
            wait_for_devices,
            prefer_pool,
            require_pool,
            prefer_bootfs,
        };
        (options, log)
    }

    /// The options `/etc/zfsbootmenu.conf` holds, as the start-up wrote them
    /// with [`ZbmOptions::conf`]. The file has no `zbm.lines`, `zbm.columns` or
    /// `zbm.kcl_override` (which goes to `${BASE}/cmdline`), so they are
    /// `None`; a missing variable is empty, as an unset shell variable is.
    pub fn from_conf(conf: &crate::conf::Conf) -> ZbmOptions {
        let get = |name: &str| conf.get(name).unwrap_or_default().to_owned();
        ZbmOptions {
            loglevel: get("loglevel"),
            spl_hostid: get("spl_hostid"),
            control_term: get("control_term"),
            import_policy: get("import_policy"),
            menu_timeout: get("menu_timeout"),
            retry_delay: get("zbm_retry_delay"),
            lines: None,
            columns: None,
            sort: get("zbm_sort"),
            hook_root: get("zbm_hook_root"),
            // `[ "${zbm_set_hostid}" -eq 1 ]`
            set_hostid: test_integer(&get("zbm_set_hostid")) == Some(1),
            kcl_override: None,
            wait_for_devices: get("zbm_wait_for_devices"),
            prefer_pool: get("zbm_prefer_pool"),
            require_pool: get("zbm_require_pool"),
            prefer_bootfs: get("zbm_prefer_bootfs"),
        }
    }

    /// The lines `zfsbootmenu-preinit.sh` appends to `/etc/zfsbootmenu.conf`.
    /// Values are written between single quotes, with each single quote in them
    /// written as `'\''`, as `zbm_conf_quote` writes them, so that sourcing the
    /// file (or [`crate::conf::Conf::parse`]) gives the values back.
    pub fn conf(&self, base: &str) -> String {
        let set_hostid = if self.set_hostid { "1" } else { "0" };
        let mut conf = String::from("# BEGIN additions by zfsbootmenu-preinit.sh\n");
        for (name, value) in [
            ("BASE", base),
            ("spl_hostid", &self.spl_hostid),
            ("import_policy", &self.import_policy),
            ("menu_timeout", &self.menu_timeout),
            ("loglevel", &self.loglevel),
            ("zbm_prefer_pool", &self.prefer_pool),
            ("zbm_require_pool", &self.require_pool),
            ("zbm_prefer_bootfs", &self.prefer_bootfs),
        ] {
            conf.push_str(&format!("export {name}={}\n", quote(value)));
        }
        conf.push_str("export default_hostid=00bab10c\n");
        for (name, value) in [
            ("zbm_sort", self.sort.as_str()),
            ("zbm_set_hostid", set_hostid),
            ("zbm_retry_delay", &self.retry_delay),
            ("zbm_hook_root", &self.hook_root),
            ("zbm_wait_for_devices", &self.wait_for_devices),
            ("control_term", &self.control_term),
        ] {
            conf.push_str(&format!("export {name}={}\n", quote(value)));
        }
        conf.push_str("# END additions by zfsbootmenu-preinit.sh\n");
        conf
    }
}

/// The pool part of a pool or dataset name.
fn pool_of(name: &str) -> &str {
    name.split_once('/').map_or(name, |(pool, _)| pool)
}

/// `zbm.lines` and `zbm.columns`: a positive integer, or `default` with a
/// warning when the value is not one.
fn positive_or_default(
    args: &ZbmArgs,
    name: &str,
    default: &str,
    unit: &str,
    log: &mut Vec<Message>,
) -> Option<String> {
    args.lookup(&[name], log).map(|value| {
        if test_integer(&value).is_some_and(|number| number > 0) {
            value
        } else {
            log.push(Message::warning(format!(
                "invalid {name}: '{value}', defaulting to {default} {unit}"
            )));
            default.to_owned()
        }
    })
}

/// The host ID the legacy script derives from `spl_hostid`, or `None` when the
/// value is invalid.
///
/// A value that is a non-zero decimal number (`(( 10#value ))`) is printed with
/// `printf "%08x"`, which reads a leading 0 as octal; otherwise a non-zero hex
/// number, with or without `0x`, keeps its first eight digits; a zero in either
/// form becomes `0`. Anything else is invalid; the legacy script checks for
/// plain digits before it uses Bash arithmetic, which would evaluate
/// expressions such as `5-3`.
pub fn parse_spl_hostid(value: &str) -> Option<String> {
    if base_constant(value, 10).is_some_and(|number| number != 0) {
        return Some(printf_hex(value));
    }
    let hex = value.strip_prefix("0x").unwrap_or(value);
    if base_constant(hex, 16).is_some_and(|number| number != 0) {
        let digits: String = hex.chars().take(8).collect();
        return Some(printf_hex(&format!("0x{digits}")));
    }
    if test_integer(hex) == Some(0) {
        return Some("0".to_owned());
    }
    None
}

/// The value of the Bash constant `base#digits`, with the wrap-around of Bash
/// arithmetic, or `None` when a digit is not valid in the base or there are
/// none.
fn base_constant(digits: &str, base: u32) -> Option<u64> {
    if digits.is_empty() {
        return None;
    }
    digits.chars().try_fold(0_u64, |value, digit| {
        let digit = digit.to_digit(base)?;
        Some(
            value
                .wrapping_mul(u64::from(base))
                .wrapping_add(u64::from(digit)),
        )
    })
}

/// What Bash's `printf "%08x" "$value"` prints for a number in C syntax (`0x`
/// hex, leading `0` octal, otherwise decimal), using the valid prefix of an
/// invalid number and the largest value on overflow, as `strtoumax` does.
fn printf_hex(value: &str) -> String {
    let text = value.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let (negative, text) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (base, digits) = if let Some(hex) = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .filter(|hex| hex.starts_with(|c: char| c.is_ascii_hexdigit()))
    {
        (16, hex)
    } else if text.starts_with('0') {
        (8, text)
    } else {
        (10, text)
    };
    let mut number: u64 = 0;
    let mut overflow = false;
    for digit in digits.chars().map_while(|c| c.to_digit(base)) {
        match number
            .checked_mul(u64::from(base))
            .and_then(|n| n.checked_add(u64::from(digit)))
        {
            Some(n) => number = n,
            None => overflow = true,
        }
    }
    if overflow {
        number = u64::MAX;
    }
    if negative {
        number = number.wrapping_neg();
    }
    format!("{number:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testkit::legacy::{Bash, LOG_STUBS, log_lines, path};
    use zbm_testkit::{Rng, TempDir, parity};

    fn parse(cmdline: &str) -> (ZbmOptions, Vec<Message>) {
        ZbmOptions::parse(&ZbmArgs::from_cmdline(cmdline))
    }

    #[test]
    fn defaults() {
        let (options, log) = parse("");
        assert_eq!(options.loglevel, "4");
        assert_eq!(options.spl_hostid, "");
        assert_eq!(options.control_term, "/dev/tty1");
        assert_eq!(options.import_policy, "hostid");
        assert_eq!(options.menu_timeout, "10");
        assert_eq!(options.retry_delay, "5");
        assert_eq!(options.lines, None);
        assert_eq!(options.sort, "name;creation;used");
        assert!(options.set_hostid);
        assert_eq!(options.kcl_override, None);
        assert_eq!(options.prefer_pool, "");
        assert_eq!(log.len(), 5);
    }

    #[test]
    fn explicit_values() {
        let (options, _) = parse(
            "loglevel=7 console=ttyS0,115200n8 zbm.import_policy=strict zbm.timeout=+3 \
             zbm.retry_delay=2 zbm.lines=40 zbm.columns=0 zbm.sort_key=used \
             zbm.hookroot=pool/hooks zbm.set_hostid=0 zbm.wait_for=/dev/sda \
             zbm.prefer=pool/ROOT/be!",
        );
        assert_eq!(options.loglevel, "7");
        assert_eq!(options.control_term, "/dev/ttyS0");
        assert_eq!(options.import_policy, "strict");
        assert_eq!(options.menu_timeout, "+3");
        assert_eq!(options.retry_delay, "2");
        assert_eq!(options.lines.as_deref(), Some("40"));
        assert_eq!(options.columns.as_deref(), Some("80"));
        assert_eq!(options.sort, "used;name;creation");
        assert_eq!(options.hook_root, "pool/hooks");
        assert!(!options.set_hostid);
        assert_eq!(options.wait_for_devices, "/dev/sda");
        assert_eq!(options.prefer_pool, "pool");
        assert_eq!(options.require_pool, "yes");
        assert_eq!(options.prefer_bootfs, "pool/ROOT/be");
    }

    #[test]
    fn host_ids() {
        assert_eq!(parse_spl_hostid("123").as_deref(), Some("0000007b"));
        assert_eq!(parse_spl_hostid("0123").as_deref(), Some("00000053"));
        assert_eq!(parse_spl_hostid("089").as_deref(), Some("00000000"));
        assert_eq!(parse_spl_hostid("deadbeef").as_deref(), Some("deadbeef"));
        assert_eq!(parse_spl_hostid("0x00bab10c").as_deref(), Some("00bab10c"));
        assert_eq!(
            parse_spl_hostid("0xDEADBEEF99").as_deref(),
            Some("deadbeef")
        );
        assert_eq!(parse_spl_hostid("0").as_deref(), Some("0"));
        assert_eq!(parse_spl_hostid("0x0").as_deref(), Some("0"));
        assert_eq!(parse_spl_hostid("4294967296").as_deref(), Some("100000000"));
        assert_eq!(parse_spl_hostid("0X12"), None);
        assert_eq!(parse_spl_hostid("xyz"), None);
        assert_eq!(parse_spl_hostid(""), None);
    }

    #[test]
    fn kcl_override_drops_root_and_host_ids() {
        let (options, log) = parse("zbm.kcl_override=\"quiet root=zfs:x spl_hostid=1 'a b' c\"");
        assert_eq!(options.kcl_override.as_deref(), Some("quiet\n\"a b\"\nc\n"));
        assert!(log.contains(&Message::info(
            "overriding all BE KCLs with: 'quiet \"a b\" c'"
        )));
        let (options, _) = parse("zbm.set_hostid=no zbm.kcl_override=\"spl_hostid=1 root=x\"");
        assert_eq!(options.kcl_override.as_deref(), Some("spl_hostid=1\n"));
    }

    #[test]
    fn conf_lists_every_export() {
        let (options, _) = parse("zbm.prefer=tank!!");
        let conf = options.conf("/zfsbootmenu");
        assert!(conf.starts_with(
            "# BEGIN additions by zfsbootmenu-preinit.sh\nexport BASE='/zfsbootmenu'\n"
        ));
        assert!(conf.contains("export zbm_require_pool='only'\n"));
        assert!(conf.contains("export zbm_prefer_pool='tank'\n"));
        assert!(conf.contains("export default_hostid=00bab10c\n"));
        assert!(conf.ends_with(
            "export control_term='/dev/tty1'\n# END additions by zfsbootmenu-preinit.sh\n"
        ));
        assert_eq!(conf.lines().count(), 17);
    }

    /// The parsing part of `zfsbootmenu-parse-commandline.sh`, without its sources
    /// and the final dracut set-up.
    fn legacy_parser() -> String {
        let script = std::fs::read_to_string(path(
            "zfsbootmenu/pre-init/zfsbootmenu-parse-commandline.sh",
        ))
        .unwrap();
        let start = script.find("unset src sources").unwrap();
        let end = script.find("# Make sure Dracut is happy").unwrap();
        script[start..end].to_owned()
    }

    const VARIABLES: &[&str] = &[
        "loglevel",
        "spl_hostid",
        "control_term",
        "import_policy",
        "menu_timeout",
        "zbm_retry_delay",
        "zbm_lines",
        "zbm_columns",
        "zbm_sort",
        "zbm_hook_root",
        "zbm_set_hostid",
        "zbm_wait_for_devices",
        "zbm_prefer_pool",
        "zbm_require_pool",
        "zbm_prefer_bootfs",
    ];

    fn rust_report(options: &ZbmOptions) -> String {
        let set_hostid = if options.set_hostid { "1" } else { "0" };
        let values = [
            options.loglevel.as_str(),
            &options.spl_hostid,
            &options.control_term,
            &options.import_policy,
            &options.menu_timeout,
            &options.retry_delay,
            options.lines.as_deref().unwrap_or_default(),
            options.columns.as_deref().unwrap_or_default(),
            &options.sort,
            &options.hook_root,
            set_hostid,
            &options.wait_for_devices,
            &options.prefer_pool,
            &options.require_pool,
            &options.prefer_bootfs,
        ];
        let mut report: String = VARIABLES
            .iter()
            .zip(values)
            .map(|(name, value)| format!("{name}={value}\n"))
            .collect();
        if let Some(text) = &options.kcl_override {
            report.push_str(&format!("override<<{text}>>\n"));
        }
        report
    }

    const PIECES: &[&str] = &[
        " ",
        " ",
        " ",
        "loglevel=7",
        "loglevel=3",
        "loglevel=x",
        "loglevel=+6",
        "spl_hostid=0x00bab10c",
        "spl.spl_hostid=123",
        "spl_hostid=0123",
        "spl_hostid=089",
        "spl_hostid=0",
        "spl_hostid=0x0",
        "spl_hostid=nope",
        "spl_hostid=deadbeef12",
        "spl_hostid=",
        "console=ttyS0,115200n8",
        "console=tty1",
        "console=a,b,c",
        "zbm.import_policy=force",
        "zbm.import_policy=strict",
        "zbm.import_policy=hostid",
        "zbm.import_policy=other",
        "zbm.force_import",
        "force_import=1",
        "zbm.force_import=no",
        "zbm.timeout=5",
        "zbm.timeout=-1",
        "zbm.timeout=x",
        "zbm.timeout=+3",
        "timeout=7",
        "zbm.show",
        "zbm.skip",
        "zbm.skip=0",
        "zbm.retry_delay=3",
        "zbm.retry_delay=0",
        "zbm.retry_delay=x",
        "zbm.import_delay=9",
        "zbm.retry_delay=",
        "zbm.lines=30",
        "zbm.lines=0",
        "zbm.lines=x",
        "zbm.columns=100",
        "zbm.columns=-5",
        "zbm.sort_key=creation",
        "zbm.sort_key=used",
        "zbm.sort_key=name",
        "zbm.sort_key=size",
        "zbm.hookroot=pool/hooks",
        "zbm.set_hostid=0",
        "zbm.set_hostid=off",
        "zbm.set_hostid=yes",
        "zbm.kcl_override=\"quiet root=x spl_hostid=1 'a b'\"",
        "zbm.kcl_override=\"spl.spl_hostid=2 c\"",
        "zbm.kcl_override=plain",
        "zbm.wait_for=/dev/sda,/dev/sdb",
        "zbm.prefer=pool",
        "zbm.prefer=pool!",
        "zbm.prefer=pool!!",
        "zbm.prefer=pool/ROOT/be!",
        "zbm.prefer=pool/ROOT/be",
        "zbm.prefer=p!!!",
        "-n",
        "=x",
        "\"",
    ];

    #[test]
    fn options_survive_the_configuration_file() {
        let mut rng = Rng::from_env(6);
        for _ in 0..50 {
            let cmdline = rng.join(PIECES, 8);
            let (options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline(&cmdline));
            let conf = crate::conf::Conf::parse(&options.conf("/zfsbootmenu")).unwrap();
            let read = ZbmOptions::from_conf(&conf);
            let expected = ZbmOptions {
                lines: None,
                columns: None,
                kcl_override: None,
                ..options
            };
            assert_eq!(read, expected, "cmdline {cmdline:?}");
        }
    }

    #[test]
    fn options_match_the_legacy_parser() {
        let mut parity = parity!("options_match_the_legacy_parser");
        let mut rng = Rng::from_env(4);
        let report: String = VARIABLES
            .iter()
            .map(|name| {
                // An unset variable and an empty one are the same to the legacy code, which
                // exports both as ''.
                format!("printf '{name}=%s\\n' \"${{{name}}}\"\n")
            })
            .collect();
        for _ in 0..150 {
            let cmdline = rng.join(PIECES, 8);
            let (expected, logged): (String, String) =
                parity.legacy(format!("{cmdline:?}"), || {
                    let base = TempDir::new("zbm-cmdline-options");
                    let file = base.write("proc-cmdline", &cmdline);
                    let legacy = Bash::new()
                        .env("BASE", &base.path().to_string_lossy())
                        .code(LOG_STUBS)
                        .source("zfsbootmenu/lib/zfsbootmenu-kcl.sh")
                        .code(&format!(
                            "_build_zbm_kcl() {{ kcl_tokenize < {}; }}",
                            file.display()
                        ))
                        .code(&legacy_parser())
                        .code(&report)
                        .code(
                            "if [ -f \"$BASE/cmdline\" ]; then \
                             printf 'override<<%s>>\\n' \"$(cat \"$BASE/cmdline\"; echo .)\"; fi",
                        )
                        .call("true", &[], b"");
                    assert!(legacy.status.success(), "{}", legacy.stderr_text());
                    // `$(cat file; echo .)` keeps the trailing newline of the override; drop
                    // the dot again.
                    (
                        legacy.stdout_text().replace(".>>\n", ">>\n"),
                        log_lines(&legacy.stderr_text()),
                    )
                });
            let (options, log) = ZbmOptions::parse(&ZbmArgs::from_cmdline(&cmdline));
            assert_eq!(rust_report(&options), expected, "cmdline {cmdline:?}");
            let messages: String = log
                .iter()
                .map(|message| format!("{}\n", message.stderr_line()))
                .collect();
            assert_eq!(messages, logged, "cmdline {cmdline:?}");
        }
    }
}
