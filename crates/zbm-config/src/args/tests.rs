use super::*;
use crate::spec::cmdline::luks::{LUKS, LUKS_KEY, LUKS_TOKENS};
use crate::spec::cmdline::{HOOKROOT, RETRY_DELAY, SET_HOSTID, SHOW, TIMEOUT};
use zbm_testkit::fakes::FakeSystem;

#[test]
fn last_occurrence_wins_and_flags_are_one() {
    let args = ZbmArgs::from_cmdline("quiet zbm.timeout=5 zbm.timeout=7 a=b=c x= -n=y");
    assert_eq!(args.get("quiet"), Some("1"));
    assert_eq!(args.get("zbm.timeout"), Some("7"));
    assert_eq!(args.get("a"), Some("b=c"));
    assert_eq!(args.get("x"), Some(""));
    assert_eq!(args.get("-n"), Some("y"));
    assert_eq!(args.get("missing"), None);
}

#[test]
fn values_are_what_the_kernel_reads() {
    let args = ZbmArgs::from_cmdline("a=-n b=-e c=\"x y\" d/e=1 .=1 ..=1 =z na\"me\"=1");
    assert_eq!(args.get("a"), Some("-n"));
    assert_eq!(args.get("b"), Some("-e"));
    assert_eq!(args.get("c"), Some("x y"));
    assert_eq!(args.get("d/e"), None);
    assert_eq!(args.get("."), None);
    assert_eq!(args.get(".."), None);
    assert_eq!(args.get(""), None);
    assert_eq!(args.values.len(), 3);
    // An unterminated quote runs to the end of the line.
    assert_eq!(ZbmArgs::from_cmdline("k=\"open a").get("k"), Some("open a"));
}

#[test]
fn the_cmdline_directory_gets_a_file_per_name() {
    let cmdline = KernelCommandLine::lenient("quiet zbm.timeout=5 d/e=1 zbm.timeout=7 c=\"x y\"").0;
    assert_eq!(
        cmdline_files(&cmdline),
        [
            ("quiet".to_owned(), "1\n".to_owned()),
            ("zbm.timeout".to_owned(), "5\n".to_owned()),
            ("zbm.timeout".to_owned(), "7\n".to_owned()),
            ("c".to_owned(), "x y\n".to_owned()),
        ]
    );
}

#[test]
fn lookup_warns_about_deprecated_names() {
    let args = ZbmArgs::from_cmdline("timeout=3 zbm.retry_delay=4");
    let mut messages = Vec::new();
    assert_eq!(args.lookup(&TIMEOUT, &mut messages), Some("3".to_owned()));
    assert_eq!(
        messages,
        [Message::warning(
            "using deprecated arg 'timeout' when 'zbm.timeout' is preferred"
        )]
    );
    assert_eq!(
        args.find(&RETRY_DELAY),
        Some(Found {
            name: "zbm.retry_delay",
            value: "4".to_owned()
        })
    );
    assert_eq!(args.lookup(&HOOKROOT, &mut messages), None);
    assert_eq!(messages.len(), 1);
}

#[test]
fn flags_follow_get_zbm_bool() {
    let on = |cmdline: &str, spec: &OptionSpec| {
        ZbmArgs::from_cmdline(cmdline).enabled(spec, &mut Vec::new())
    };
    // zbm.set_hostid is on by default, zbm.show off.
    assert!(on("", &SET_HOSTID));
    assert!(!on("", &SHOW));
    assert!(on("zbm.show", &SHOW));
    for off in ["0", "OFF", "No", ""] {
        assert!(!on(&format!("zbm.set_hostid={off}"), &SET_HOSTID), "{off}");
    }
    assert!(on("zbm.set_hostid=yes", &SET_HOSTID));
    // A specification without a default is off.
    assert!(!on("", &HOOKROOT));
    assert!(is_on("On") && !is_on("off"));
}

#[test]
fn the_cmdline_directory_holds_the_first_line_of_each_value() {
    let fake = FakeSystem::new();
    fake.files
        .dir("/zfsbootmenu/cmdline.d")
        .file("/zfsbootmenu/cmdline.d/zbm.luks", "\tUUID=a, b \nmore\n")
        .file("/zfsbootmenu/cmdline.d/zbm.luks_key", "");
    let dir = CmdlineDir::new(&fake.files, Path::new("/zfsbootmenu"));
    let mut messages = Vec::new();
    assert_eq!(
        dir.lookup(&LUKS, &mut messages).as_deref(),
        Some("UUID=a, b")
    );
    assert_eq!(dir.lookup(&LUKS_KEY, &mut messages).as_deref(), Some(""));
    assert_eq!(dir.lookup(&LUKS_TOKENS, &mut messages), None);
    assert_eq!(dir.raw(""), None);
    assert_eq!(dir.raw("a/b"), None);
    assert!(messages.is_empty());
}
