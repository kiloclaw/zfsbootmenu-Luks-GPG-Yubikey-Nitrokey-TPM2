use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// What `help` showed: the pager, and the map it reads keys with.
type Shown = Vec<(Pager, Map)>;

/// Runs `help` with `args` on `fake`, its topic printed as it is without a
/// terminal: its exit status, standard output and standard error, and what
/// it showed.
fn call(fake: &FakeSystem, args: &[&str]) -> (i32, String, String, Shown) {
    let mut shown = Vec::new();
    let mut printed = |pager: &Pager, map: Map, out: &mut dyn Write| {
        shown.push((pager.clone(), map));
        pager
            .lines
            .iter()
            .try_for_each(|line| writeln!(out, "{line}"))
    };
    let (status, stdout, stderr) = call_with(fake, args, &mut printed);
    (status, stdout, stderr, shown)
}

/// Runs `help` with `args` on `fake`, its topic shown with `show`: its exit
/// status, standard output and standard error.
fn call_with(fake: &FakeSystem, args: &[&str], show: Show<'_>) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let status = run_in(
        &fake.system(),
        &args,
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
        show,
    );
    (
        status,
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
    )
}

/// `usage` as the commands topic shows it: each line indented under its
/// entry, as a code block.
fn indented(usage: &str) -> String {
    usage
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("    {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn every_linked_name_is_listed_with_its_usage() {
    let (status, text, _, shown) = call(&FakeSystem::new(), &["commands"]);
    assert_eq!(status, 0);
    assert_eq!(shown[0].0.topic, Topic::Commands);
    let headings: Vec<&str> = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with(' '))
        .collect();
    for linked in LINKED {
        let (applet, _) = crate::find(APPLETS, linked.name).unwrap();
        let heading = headings
            .iter()
            .position(|heading| heading.split(", ").any(|name| name.ends_with(linked.name)))
            .unwrap_or_else(|| panic!("{} is not listed", linked.name));
        let entry = format!("\n{text}");
        let entry = entry
            .split(&format!("\n{}\n", headings[heading]))
            .nth(1)
            .unwrap();
        assert!(
            entry.contains(&indented(applet.usage)),
            "{}: {entry}",
            linked.name
        );
    }
    // An applet with several names is listed once, under all of them; a
    // name outside /bin with its directory.
    assert!(headings.contains(&"logs, zlogtail, ztrace"));
    assert!(headings.contains(&"poweroff, shutdown"));
    assert!(headings.contains(&"/libexec/zfunc"));
    assert!(
        text.starts_with("zluks\n\n  Unlock, close and list LUKS volumes.\n\n    Usage: zluks")
    );
}

#[test]
fn the_six_library_functions_are_listed() {
    let (_, text, _, _) = call(&FakeSystem::new(), &["commands"]);
    let applets_end = text.find("\nhelp\n").unwrap();
    for function in &FUNCTIONS {
        let summary = function.summary;
        let usage = indented(function.usage);
        let entry = format!(
            "\n{}\n\n  {}{}.\n\n{usage}\n",
            function.name,
            summary[..1].to_uppercase(),
            &summary[1..]
        );
        let at = text
            .find(&entry)
            .unwrap_or_else(|| panic!("{entry}\n{text}"));
        assert!(at > applets_end, "{}", function.name);
    }
    assert!(text.contains("\nmount_efivarfs\n\n  Mount efivarfs at DIRECTORY"));
    assert!(text.contains("\n    Usage: mount_efivarfs [rw|ro [DIRECTORY]]\n"));
    // Then the line that points to the other commands, last.
    assert!(text.ends_with(
        "zbm --list lists every command of zbm, also those the recovery shell does not offer, \
         and zbm command --help prints the usage of one.\n"
    ));
}

#[test]
fn no_topic_shows_the_commands_then_names_the_others() {
    let fake = FakeSystem::new();
    let (status, text, stderr, shown) = call(&fake, &[]);
    assert_eq!((status, stderr.as_str()), (0, ""));
    assert_eq!(shown[0].0.topic, Topic::Commands);
    let (_, commands, _, _) = call(&fake, &["commands"]);
    assert_eq!(
        text,
        format!(
            "{commands}\nOther topics:\n  \
             zbm help keys      the menu's keys\n  \
             zbm help options   the kernel command line's options\n"
        )
    );
}

#[test]
fn an_unknown_topic_exits_2() {
    let fake = FakeSystem::new();
    let (status, stdout, stderr, shown) = call(&fake, &["bogus\x1b"]);
    assert_eq!(status, USAGE);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!(
            "help: bogus\\x1b is no topic: commands, keys or options\n{}",
            HELP.usage
        )
    );
    assert!(shown.is_empty());
    let (status, _, stderr, shown) = call(&fake, &["keys", "options"]);
    assert_eq!((status, stderr.as_str()), (USAGE, HELP.usage));
    assert!(shown.is_empty());
    // Through `zbm`, on this machine, before anything is read.
    let args = ["help".to_owned(), "nothing".to_owned()];
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };
    assert_eq!(crate::run("zbm", &args, &mut io), USAGE);
}

#[test]
fn keys_follow_zbm_menu() {
    let fake = FakeSystem::new();
    let (status, workspace, _, shown) = call(&fake, &["keys"]);
    assert_eq!(status, 0);
    assert_eq!(shown[0].0.topic, Topic::Keys);
    assert_eq!(shown[0].1, Map::Workspace);
    let printed = |map| {
        let text = docs::plain(&manual::help_page(map));
        text.lines()
            .flat_map(|line| [line, "\n"])
            .collect::<String>()
    };
    assert_eq!(workspace, printed(Map::Workspace));
    assert!(workspace.contains("\nAliases of the fzf Menu\n"));
    fake.files
        .file("/zfsbootmenu/zbm.cmdline", "quiet\nzbm.menu=classic\n");
    let (status, classic, _, shown) = call(&fake, &["keys"]);
    assert_eq!(status, 0);
    assert_eq!(shown[0].1, Map::Classic);
    assert_eq!(classic, printed(Map::Classic));
    assert!(!classic.contains("Aliases"));
    // The pager reads keys with that map, whatever the topic.
    let (_, _, _, shown) = call(&fake, &["options"]);
    assert_eq!(shown[0].1, Map::Classic);
}

#[test]
fn help_piped_is_plain_text() {
    let fake = FakeSystem::new();
    for topic in ["commands", "keys", "options"] {
        let (status, text, stderr, _) = call(&fake, &[topic]);
        assert_eq!((status, stderr.as_str()), (0, ""), "{topic}");
        assert!(text.lines().count() > 50, "{topic}");
        for line in text.lines() {
            assert!(
                !line.contains("**") && !line.contains('`') && !line.contains(":doc:"),
                "{topic}: {line}"
            );
            assert!(!line.trim_start().starts_with(".. "), "{topic}: {line}");
            assert!(!line.chars().any(char::is_control), "{topic}: {line:?}");
        }
    }
    let (_, options, _, _) = call(&fake, &["options"]);
    assert!(options.contains("\nzbm.timeout\n") || options.contains("\nzbm.timeout="));
    assert!(options.contains("see zfsbootmenu-luks(7)"));
}

#[test]
fn a_topic_that_cannot_be_shown_exits_1() {
    let mut broken = |_: &Pager, _: Map, _: &mut dyn Write| -> io::Result<()> {
        Err(io::Error::other("no terminal"))
    };
    let (status, stdout, stderr) = call_with(&FakeSystem::new(), &["keys"], &mut broken);
    assert_eq!(
        (status, stdout.as_str(), stderr.as_str()),
        (1, "", "help: unable to show the topic: no terminal\n")
    );
}

/// The man page the example `gen_commands` writes, and its section.
const PAGE: &str = "docs/man/zfsbootmenu.7.rst";
const SHELL_COMMANDS: &str = "shell-commands";

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "the test reads the manual page from the repository"
)]
fn the_manual_page_lists_the_shell_commands() {
    let text = std::fs::read_to_string(zbm_testkit::repo_root().join(PAGE)).unwrap();
    let updated = docs::splice(PAGE, &text, SHELL_COMMANDS, &commands()).unwrap();
    assert!(
        updated == text,
        "{PAGE} is out of date: run `cargo xtask gen-docs`"
    );
}

#[test]
fn a_reader_that_stops_reading_ends_help_with_0() {
    /// A pipe whose reader is gone, as after `| head -1`.
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut printed = |pager: &Pager, _: Map, out: &mut dyn Write| {
        pager
            .lines
            .iter()
            .try_for_each(|line| writeln!(out, "{line}"))
    };
    let mut stderr = Vec::new();
    let mut io = Io {
        stdout: &mut Closed,
        stderr: &mut stderr,
    };
    let args = ["options".to_owned()];
    let status = run_in(&FakeSystem::new().system(), &args, &mut io, &mut printed);
    assert_eq!((status, stderr.as_slice()), (0, &b""[..]));
}

#[test]
fn the_usage_and_the_error_name_every_topic() {
    let names: Vec<&str> = TOPICS.iter().map(|(name, ..)| *name).collect();
    let first = HELP.usage.lines().next().unwrap();
    assert_eq!(first, format!("Usage: help [{}]", names.join(" | ")));
    assert_eq!(topic_names(), "commands, keys or options");
    // Each topic is shown with its own page, the commands' with the pager's
    // own topic too.
    let fake = FakeSystem::new();
    for (name, subject, _) in &TOPICS {
        let (status, text, _, shown) = call(&fake, &[name]);
        assert_eq!(status, 0, "{name}");
        assert_eq!(shown[0].0.lines, lines(Some(*subject), Map::Workspace));
        assert!(!text.contains("Other topics:"), "{name}");
    }
}

#[test]
fn the_six_functions_are_the_shell_library_s() {
    for function in &FUNCTIONS {
        assert!(
            zbm_host::image::library::FUNCTIONS.contains(&function.name),
            "{} is no function of the library",
            function.name
        );
    }
}

#[test]
fn brackets_that_repeat_the_heading_are_left_out_of_the_summary() {
    let (_, text, _, _) = call(&FakeSystem::new(), &["commands"]);
    assert!(
        text.contains("\npoweroff, shutdown\n\n  Export the writable pools, then power off.\n")
    );
    assert!(text.contains("\n/libexec/zfunc\n\n  Run a function of the hooks' shell library.\n"));
    let names = ["zbm".to_owned()];
    assert_eq!(without_names("a (also zbm)", &names), "a");
    assert_eq!(without_names("a (zbm)", &names), "a");
    assert_eq!(without_names("a (also b)", &names), "a (also b)");
    assert_eq!(without_names("a (zbm", &names), "a (zbm");
    assert_eq!(without_names("a", &names), "a");
}
