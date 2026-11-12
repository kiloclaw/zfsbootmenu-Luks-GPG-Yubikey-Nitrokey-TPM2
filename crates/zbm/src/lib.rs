//! `zbm`: the ZFSBootMenu runtime.
//!
//! Like BusyBox, `zbm` is one program that acts as each of its commands, chosen
//! by the name it is run under (through a link such as `/bin/zfsbootmenu ->
//! /libexec/zbm`) or by its first argument (`zbm zfsbootmenu`). One binary
//! keeps the boot image small. The commands arrived with the runtime port:
//! Phase 5 of the port plan (`docs/dev/history/port-roadmap.md`) brought the
//! start-up hooks, the start-up program and the supervisor, and package B of
//! Phase 6 brought `zluks`.

use std::io::Write;

pub mod applets;
mod chroot;
pub mod console;
pub mod console_font;
pub mod debug;
mod esp;
pub mod explain;
pub mod fonts;
mod help;
pub mod image;
mod kexec;
pub mod layouts;
mod library_usage;
pub mod lock;
mod logs;
pub mod luks;
pub mod measure;
pub mod menu;
pub mod menu_demo;
mod power;
mod remote;
pub mod report;
mod run_hooks;
pub mod saved;
#[cfg(any(test, feature = "secret-test"))]
pub mod secret_search;
mod snapshots;
pub mod startup;
pub mod supervisor;
mod table;
pub mod unlock;
mod zfunc;

pub use table::APPLETS;

/// The program's output streams.
pub struct Io<'a> {
    /// Standard output.
    pub stdout: &'a mut dyn Write,
    /// Standard error.
    pub stderr: &'a mut dyn Write,
}

/// One command of the multi-call program.
pub struct Applet {
    /// The name it is run under.
    pub name: &'static str,
    /// The other names it answers to through a link, each with the
    /// arguments it puts before the user's (`ztrace` runs `logs --trace`).
    pub names: &'static [(&'static str, &'static [&'static str])],
    /// A one-line description for `zbm --list`.
    pub summary: &'static str,
    /// Its usage text, which `--help` as its first argument prints.
    pub usage: &'static str,
    /// Runs the command with its arguments and returns its exit status.
    pub run: fn(&[String], &mut Io) -> i32,
}

/// The exit status for a command that does not exist, as a shell reports it.
pub const NOT_FOUND: i32 = 127;

/// The exit status for wrong usage.
pub const USAGE: i32 = 2;

/// The base name of a path, which selects the command when `zbm` runs under
/// another name.
fn command_name(argv0: &str) -> &str {
    argv0
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(argv0)
}

fn usage(applets: &[Applet]) -> String {
    let mut text = String::from(
        "\
Usage: zbm <command> [arguments]
       <command> [arguments]   (through a link named after the command)

       zbm --list      list the commands
       zbm --version   print the version
       zbm --help      print this help
",
    );
    if applets.is_empty() {
        text.push_str("\nNo commands are available yet.\n");
    }
    text
}

/// The applet of `applets` that `name` runs, by its name or one of its
/// names, with the arguments that name puts first.
fn find<'a>(applets: &'a [Applet], name: &str) -> Option<(&'a Applet, &'static [&'static str])> {
    applets.iter().find_map(|applet| {
        if applet.name == name {
            return Some((applet, &[][..]));
        }
        let (_, first) = applet.names.iter().find(|(other, _)| *other == name)?;
        Some((applet, *first))
    })
}

/// Runs `applet` with `first`, then `args`; `--help` as the first of `args`
/// prints its usage instead.
fn run_applet(applet: &Applet, first: &[&str], args: &[String], io: &mut Io) -> i32 {
    if args.first().is_some_and(|arg| arg == "--help") {
        let _ = io.stdout.write_all(applet.usage.as_bytes());
        return 0;
    }
    if first.is_empty() {
        return (applet.run)(args, io);
    }
    let args: Vec<String> = first
        .iter()
        .map(|arg| (*arg).to_owned())
        .chain(args.iter().cloned())
        .collect();
    (applet.run)(&args, io)
}

/// Runs `zbm` with the name it was started under and its arguments, using the
/// commands in `applets`.
pub fn run_with(applets: &[Applet], argv0: &str, args: &[String], io: &mut Io) -> i32 {
    let name = command_name(argv0);
    if name != "zbm" {
        let Some((applet, first)) = find(applets, name) else {
            let _ = writeln!(io.stderr, "zbm: {name}: unknown command");
            return NOT_FOUND;
        };
        return run_applet(applet, first, args, io);
    }
    match args.first().map(String::as_str) {
        None => {
            let _ = io.stderr.write_all(usage(applets).as_bytes());
            USAGE
        }
        Some("-h" | "--help") => {
            let _ = io.stdout.write_all(usage(applets).as_bytes());
            0
        }
        Some("-V" | "--version") => {
            let _ = writeln!(io.stdout, "zbm {}", zbm_platform::version::ZBM_VERSION);
            0
        }
        Some("--list") => {
            for applet in applets {
                let _ = writeln!(io.stdout, "{:<20} {}", applet.name, applet.summary);
            }
            0
        }
        Some(option) if option.starts_with('-') => {
            let _ = writeln!(io.stderr, "zbm: unknown option: {option}");
            let _ = io.stderr.write_all(usage(applets).as_bytes());
            USAGE
        }
        Some(command) => {
            let Some((applet, first)) = find(applets, command) else {
                let _ = writeln!(
                    io.stderr,
                    "zbm: {command}: unknown command (see zbm --list)"
                );
                return NOT_FOUND;
            };
            run_applet(applet, first, args.get(1..).unwrap_or_default(), io)
        }
    }
}

/// The name of the applet `zbm` runs for `argv0` and `args` with the
/// commands in `applets`: that of the name it was started under, else of its
/// first argument; the name itself when no applet answers to it.
fn applet_name_in(applets: &[Applet], argv0: &str, args: &[String]) -> String {
    let name = match command_name(argv0) {
        "zbm" => args.first().map_or("zbm", String::as_str),
        name => name,
    };
    find(applets, name)
        .map_or(name, |(applet, _)| applet.name)
        .to_owned()
}

/// The name of the command `zbm` runs for `argv0` and `args`, which crash
/// reports name: the applet's, whether it was started under its name or one
/// of its other names, or with either as its first argument.
pub fn applet_name(argv0: &str, args: &[String]) -> String {
    applet_name_in(APPLETS, argv0, args)
}

/// The recovery shell's commands as the man page's "The Recovery Shell"
/// lists them, in reStructuredText: each name an image of the Rust runtime
/// links to `zbm`, with its applet's summary and usage, then the functions
/// of the shell library the shell offers. `cargo xtask gen-docs` writes it
/// through the example `gen_commands`; `zbm help commands` shows it as
/// plain text.
pub fn shell_commands() -> String {
    help::commands()
}

/// Runs `zbm` with its own commands.
pub fn run(argv0: &str, args: &[String], io: &mut Io) -> i32 {
    run_with(APPLETS, argv0, args, io)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo(args: &[String], io: &mut Io) -> i32 {
        let _ = writeln!(io.stdout, "echo {}", args.join(" "));
        i32::try_from(args.len()).unwrap_or(0)
    }

    const TEST_APPLETS: &[Applet] = &[
        Applet {
            name: "echo",
            names: &[],
            summary: "print the arguments",
            usage: "Usage: echo [ARGS]\n",
            run: echo,
        },
        Applet {
            name: "t",
            names: &[("u", &["--x"])],
            summary: "print the arguments, under another name with --x",
            usage: "Usage: t [ARGS]\n",
            run: echo,
        },
    ];

    fn call(applets: &[Applet], argv0: &str, args: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = run_with(
            applets,
            argv0,
            &args,
            &mut Io {
                stdout: &mut stdout,
                stderr: &mut stderr,
            },
        );
        (
            code,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    #[test]
    fn dispatches_by_name_or_argument() {
        assert_eq!(
            call(TEST_APPLETS, "/bin/echo", &["a", "b"]),
            (2, "echo a b\n".to_owned(), String::new())
        );
        assert_eq!(
            call(TEST_APPLETS, "/libexec/zbm", &["echo", "x"]),
            (1, "echo x\n".to_owned(), String::new())
        );
        assert_eq!(
            call(TEST_APPLETS, "zbm", &["--list"]).1,
            "echo                 print the arguments\n\
             t                    print the arguments, under another name with --x\n"
        );
    }

    #[test]
    fn a_name_runs_its_applet_with_its_arguments_first() {
        assert_eq!(
            call(TEST_APPLETS, "/bin/u", &["a"]),
            (2, "echo --x a\n".to_owned(), String::new())
        );
        assert_eq!(
            call(TEST_APPLETS, "zbm", &["u", "a"]),
            (2, "echo --x a\n".to_owned(), String::new())
        );
        assert_eq!(
            call(TEST_APPLETS, "/bin/t", &["a"]),
            (1, "echo a\n".to_owned(), String::new())
        );
    }

    #[test]
    fn help_prints_the_usage_and_exits_0_for_every_applet() {
        for applets in [APPLETS, TEST_APPLETS] {
            for applet in applets {
                let expected = (0, applet.usage.to_owned(), String::new());
                let link = format!("/bin/{}", applet.name);
                assert_eq!(call(applets, &link, &["--help"]), expected);
                assert_eq!(call(applets, "zbm", &[applet.name, "--help"]), expected);
            }
        }
        assert_eq!(
            call(TEST_APPLETS, "/bin/u", &["--help"]),
            (0, "Usage: t [ARGS]\n".to_owned(), String::new())
        );
    }

    #[test]
    fn a_crash_under_a_name_is_reported_under_its_applet() {
        let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        assert_eq!(applet_name_in(TEST_APPLETS, "/bin/u", &args(&["a"])), "t");
        assert_eq!(applet_name_in(TEST_APPLETS, "zbm", &args(&["u", "a"])), "t");
        assert_eq!(applet_name_in(TEST_APPLETS, "/bin/v", &[]), "v");
    }

    #[test]
    fn reports_unknown_commands_and_options() {
        let (code, _, stderr) = call(TEST_APPLETS, "/bin/other", &[]);
        assert_eq!(
            (code, stderr.as_str()),
            (NOT_FOUND, "zbm: other: unknown command\n")
        );
        let (code, _, stderr) = call(TEST_APPLETS, "zbm", &["nothing"]);
        assert_eq!(code, NOT_FOUND);
        assert!(stderr.starts_with("zbm: nothing: unknown command"));
        let (code, _, stderr) = call(TEST_APPLETS, "zbm", &["--bogus"]);
        assert_eq!(code, USAGE);
        assert!(stderr.starts_with("zbm: unknown option: --bogus\nUsage:"));
        assert_eq!(call(TEST_APPLETS, "zbm", &[]).0, USAGE);
    }

    #[test]
    fn the_built_in_options() {
        let (code, stdout, _) = call(APPLETS, "zbm", &["--version"]);
        assert_eq!(code, 0);
        assert_eq!(
            stdout,
            format!("zbm {}\n", zbm_platform::version::ZBM_VERSION)
        );
        let (code, stdout, _) = call(APPLETS, "zbm", &["--help"]);
        assert_eq!(code, 0);
        assert!(!stdout.contains("No commands are available yet."));
        assert!(
            call(&[], "zbm", &["--help"])
                .1
                .contains("No commands are available yet.")
        );
        let (code, list, _) = call(APPLETS, "zbm", &["--list"]);
        assert_eq!(code, 0);
        let names: Vec<&str> = list
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .collect();
        let expected = vec![
            "zfsbootmenu-init",
            "zfsbootmenu-parse-commandline",
            "zfsbootmenu-preinit",
            "zluks",
            "unlock",
            "console-autosize",
            "console-font",
            "console-fonts",
            "keyboard-layouts",
            "image",
            "console",
            "zfsbootmenu",
            "debug",
            "report",
            "explain",
            "measure",
            "lock",
            "run-hooks",
            "zfunc",
            "reboot",
            "poweroff",
            "firmware-setup",
            "zfs-chroot",
            "zkexec",
            "mount_esp",
            "logs",
            "help",
            "zsnapshots",
        ];
        assert_eq!(names, expected);
        let (code, _, stderr) = call(APPLETS, "zbm", &["zfsbootmenu-init", "--bogus"]);
        assert_eq!(code, USAGE);
        assert_eq!(stderr, "zfsbootmenu-init: unknown option --bogus\n");
        let (code, _, stderr) = call(APPLETS, "zbm", &["console-autosize", "--console"]);
        assert_eq!(code, USAGE);
        assert_eq!(stderr, "console-autosize: unknown option --console\n");
        // /dev/null is no virtual console: without the image's configuration
        // the applet says so, and with it the font is left alone.
        let (code, _, stderr) = call(
            APPLETS,
            "zbm",
            &["console-autosize", "--console", "/dev/null"],
        );
        assert!(
            (code == 1 && stderr.contains("unable to source")) || (code == 0 && stderr.is_empty()),
            "{code}: {stderr}"
        );
        assert_eq!(command_name("/a/b/"), "b");
        let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        assert_eq!(applet_name("/bin/zluks", &args(&["open"])), "zluks");
        assert_eq!(
            applet_name("/libexec/zbm", &args(&["debug", "crash"])),
            "debug"
        );
        assert_eq!(applet_name("zbm", &[]), "zbm");
    }
}
