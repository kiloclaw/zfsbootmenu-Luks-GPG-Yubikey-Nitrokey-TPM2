use super::*;
use zbm_testkit::legacy::{Bash, LOG_STUBS, log_lines, path};
use zbm_testkit::{Rng, TempDir, parity};

fn parse(cmdline: &str) -> (ZbmOptions, Vec<Message>) {
    ZbmOptions::parse(&ZbmArgs::from_cmdline(cmdline))
}

#[test]
fn defaults() {
    let (options, log) = parse("");
    assert_eq!(options.loglevel, LogLevel::MIN);
    assert_eq!(options.spl_hostid, None);
    assert_eq!(options.control_term, "/dev/tty1");
    assert_eq!(options.import_policy, ImportPolicy::Hostid);
    assert_eq!(options.menu_timeout, Timeout::DEFAULT);
    assert_eq!(options.retry_delay, RetryDelay::DEFAULT);
    assert_eq!(options.lines, None);
    assert_eq!(options.sort, "name;creation;used");
    assert!(options.set_hostid);
    assert_eq!(options.kcl_override, None);
    assert_eq!(options.prefer_pool, None);
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
    assert_eq!(options.loglevel.get(), 7);
    assert_eq!(options.control_term, "/dev/ttyS0");
    assert_eq!(options.import_policy, ImportPolicy::Strict);
    // Written to the configuration as 3, where the shell kept `+3`.
    assert_eq!(options.menu_timeout, Timeout::Seconds(3));
    assert_eq!(options.retry_delay.seconds(), 2);
    assert_eq!(options.lines.as_deref(), Some("40"));
    assert_eq!(options.columns.as_deref(), Some("80"));
    assert_eq!(options.sort, "used;name;creation");
    assert_eq!(options.hook_root, "pool/hooks");
    assert!(!options.set_hostid);
    assert_eq!(options.wait_for_devices, "/dev/sda");
    assert_eq!(options.prefer_pool, "pool".parse().ok());
    assert_eq!(options.require_pool, PoolRequirement::Required);
    assert_eq!(
        options.prefer_bootfs,
        BootfsPreference::Dataset("pool/ROOT/be".parse().unwrap())
    );
    // A name that no pool can have prefers none.
    let (options, _) = parse("zbm.prefer=1pool!");
    assert_eq!(options.prefer_pool, None);
    assert_eq!(options.require_pool, PoolRequirement::Required);
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
    let (options, log) =
        parse("zbm.kcl_override=\"quiet root=zfs:x spl_hostid=1 d='a b' 'c d' e\"");
    let text = options
        .kcl_override
        .as_ref()
        .map(KernelCommandLine::one_per_line);
    assert_eq!(text.as_deref(), Some("quiet\nd=\"a b\"\ne\n"));
    assert!(log.contains(&Message::info(
        "overriding all BE KCLs with: 'quiet d=\"a b\" e'"
    )));
    // A quoted word with a space is not a kernel argument.
    assert!(log.contains(&Message::warning(
        "ignoring \"c d\" in zbm.kcl_override: it is not one kernel argument"
    )));
    let (options, _) = parse("zbm.set_hostid=no zbm.kcl_override=\"spl_hostid=1 root=x\"");
    let text = options.kcl_override.as_ref().map(ToString::to_string);
    assert_eq!(text.as_deref(), Some("spl_hostid=1"));
}

#[test]
fn conf_lists_every_export() {
    let (options, _) = parse("zbm.prefer=tank!!");
    let conf = options.conf("/zfsbootmenu");
    assert!(
        conf.starts_with(
            "# BEGIN additions by zfsbootmenu-preinit.sh\nexport BASE='/zfsbootmenu'\n"
        )
    );
    assert!(conf.contains("export zbm_require_pool='only'\n"));
    assert!(conf.contains("export zbm_prefer_pool='tank'\n"));
    assert!(conf.contains("export default_hostid=00bab10c\n"));
    assert!(
        conf.ends_with(
            "export control_term='/dev/tty1'\n# END additions by zfsbootmenu-preinit.sh\n"
        )
    );
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
    let hostid = options.spl_hostid.as_ref().map(HostId::to_string);
    let pool = options.prefer_pool.as_ref().map(PoolName::to_string);
    let values = [
        &options.loglevel.to_string(),
        hostid.as_deref().unwrap_or_default(),
        &options.control_term,
        options.import_policy.as_str(),
        &options.menu_timeout.to_string(),
        &options.retry_delay.to_string(),
        options.lines.as_deref().unwrap_or_default(),
        options.columns.as_deref().unwrap_or_default(),
        &options.sort,
        &options.hook_root,
        set_hostid,
        &options.wait_for_devices,
        pool.as_deref().unwrap_or_default(),
        options.require_pool.as_str(),
        &options.prefer_bootfs.to_string(),
    ];
    let mut report: String = VARIABLES
        .iter()
        .zip(values)
        .map(|(name, value)| [*name, "=", value, "\n"].concat())
        .collect();
    if let Some(cmdline) = &options.kcl_override {
        report.push_str(&["override<<", &cmdline.one_per_line(), ">>\n"].concat());
    }
    report
}

/// The legacy script's report with its numbers, host IDs and pool names as
/// the types of `zbm-types` print them: `+3` as `3`, a host ID of `0` as
/// `00000000`, and a pool that no pool can be named as none.
fn canonical(report: &str) -> String {
    // The override's arguments, after the variables, stay as they are.
    let (variables, overridden) = report
        .split_once("override<<")
        .map_or((report, ""), |(variables, rest)| (variables, rest));
    let overridden = if overridden.is_empty() {
        String::new()
    } else {
        ["override<<", overridden].concat()
    };
    let variables: String = variables
        .lines()
        .map(|line| {
            let Some((name, value)) = line.split_once('=') else {
                return [line, "\n"].concat();
            };
            let value = match name {
                "loglevel" => value
                    .parse::<LogLevel>()
                    .map_or(value.to_owned(), |v| v.to_string()),
                "menu_timeout" => value
                    .parse::<Timeout>()
                    .map_or(value.to_owned(), |v| v.to_string()),
                "zbm_retry_delay" => RetryDelay::or_default(value).to_string(),
                "spl_hostid" if !value.is_empty() => {
                    HostId::from_hex(value).map_or(value.to_owned(), |v| v.to_string())
                }
                "zbm_prefer_pool" => value
                    .parse::<PoolName>()
                    .map(|pool| pool.to_string())
                    .unwrap_or_default(),
                _ => value.to_owned(),
            };
            [name, "=", &value, "\n"].concat()
        })
        .collect();
    variables + &overridden
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
    "zbm.kcl_override=\"quiet root=x spl_hostid=1 x='a b'\"",
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
];

#[test]
fn options_survive_the_configuration_file() {
    let mut rng = Rng::from_env(6);
    for _ in 0..50 {
        let cmdline = rng.join(PIECES, 8);
        let (options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline(&cmdline));
        let conf = crate::conf::Conf::parse(&options.conf("/zfsbootmenu"));
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

/// Quotes are left out of the pieces: the legacy tokenizer kept them in the
/// argument, where the kernel removes them or groups words with them, and the
/// runtime splits as the kernel does (`zbm_types::KernelCommandLine`, tested
/// there and in `kcl_override_drops_root_and_host_ids`). The single quotes of
/// `zbm.kcl_override`, which both turn into double quotes, are kept.
#[test]
fn options_match_the_legacy_parser() {
    let mut parity = parity!("options_match_the_legacy_parser");
    let mut rng = Rng::from_env(4);
    let report: String = VARIABLES
        .iter()
        .map(|name| {
            // An unset variable and an empty one are the same to the legacy code, which
            // exports both as ''.
            ["printf '", name, "=%s\\n' \"${", name, "}\"\n"].concat()
        })
        .collect();
    for _ in 0..150 {
        let count = rng.below(9);
        let pieces: Vec<&str> = (0..count).map(|_| *rng.choose(PIECES)).collect();
        let cmdline = pieces.join(" ");
        let (expected, logged): (String, String) = parity.legacy(format!("{cmdline:?}"), || {
            let base = TempDir::new("zbm-config-options");
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
            assert!(legacy.status.is_success(), "{}", legacy.stderr_text());
            // `$(cat file; echo .)` keeps the trailing newline of the override; drop
            // the dot again.
            (
                legacy.stdout_text().replace(".>>\n", ">>\n"),
                log_lines(&legacy.stderr_text()),
            )
        });
        let (options, log) = ZbmOptions::parse(&ZbmArgs::from_cmdline(&cmdline));
        assert_eq!(
            rust_report(&options),
            canonical(&expected),
            "cmdline {cmdline:?}"
        );
        let messages: String = log
            .iter()
            .map(|message| message.stderr_line() + "\n")
            .collect();
        assert_eq!(messages, logged, "cmdline {cmdline:?}");
    }
}

#[test]
fn origins_say_which_name_gave_each_value_and_which_were_invalid() {
    use crate::origin::Origin;
    use crate::spec::cmdline::{
        CONSOLE, FORCE_IMPORT, IMPORT_POLICY, LOGLEVEL, RETRY_DELAY, SHOW, SPL_HOSTID, TIMEOUT,
    };
    let args = ZbmArgs::from_cmdline(
        "loglevel=2 spl_hostid=nope zbm.import_policy=other zbm.timeout=x zbm.import_delay=0",
    );
    let (options, origins, log) = ZbmOptions::read(&args);
    assert_eq!(ZbmOptions::parse(&args), (options, log));
    let invalid = |name: &'static str, value: &str| Origin::Invalid {
        name,
        value: value.to_owned(),
    };
    assert_eq!(origins.of(&LOGLEVEL), Some(&invalid("loglevel", "2")));
    assert_eq!(
        origins.of(&SPL_HOSTID),
        Some(&invalid("spl_hostid", "nope"))
    );
    assert_eq!(
        origins.of(&IMPORT_POLICY),
        Some(&invalid("zbm.import_policy", "other"))
    );
    assert_eq!(origins.of(&TIMEOUT), Some(&invalid("zbm.timeout", "x")));
    assert_eq!(
        origins.of(&RETRY_DELAY),
        Some(&invalid("zbm.import_delay", "0"))
    );
    assert_eq!(origins.of(&CONSOLE), Some(&Origin::Default));
    // Options read only when others are missing were not read.
    assert_eq!(origins.of(&FORCE_IMPORT), None);
    assert_eq!(origins.of(&SHOW), None);
}

#[test]
fn values_are_found_by_their_option() {
    use crate::spec::cmdline::{KCL_OVERRIDE, PREFER, SET_HOSTID};
    let (options, _) = parse("zbm.prefer=tank! zbm.set_hostid=0 zbm.kcl_override=\"a x='b c'\"");
    assert_eq!(options.value_of(&PREFER).as_deref(), Some("tank!"));
    assert_eq!(options.value_of(&SET_HOSTID).as_deref(), Some("0"));
    assert_eq!(
        options.value_of(&KCL_OVERRIDE).as_deref(),
        Some("a x=\"b c\"")
    );
    assert_eq!(options.value_of(&crate::spec::cmdline::luks::LUKS), None);
    let (none, _) = parse("");
    assert_eq!(none.value_of(&PREFER), None);
    let (shown, _) = parse("zbm.show zbm.import_policy=hostid");
    for spec in [&crate::spec::cmdline::SHOW, &crate::spec::cmdline::SKIP] {
        assert_eq!(shown.value_of(spec).as_deref(), Some("-1"));
    }
    let force = &crate::spec::cmdline::FORCE_IMPORT;
    assert_eq!(shown.value_of(force).as_deref(), Some("hostid"));
}
