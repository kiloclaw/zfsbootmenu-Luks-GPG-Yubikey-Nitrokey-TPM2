//! `zbm`: the ZFSBootMenu runtime.
//!
//! Like BusyBox, `zbm` is one program that acts as each of its commands, chosen
//! by the name it is run under (through a link such as `/bin/zfsbootmenu ->
//! /libexec/zbm`) or by its first argument (`zbm zfsbootmenu`). One binary
//! keeps the boot image small. The commands arrive with the runtime port: Phase
//! 5 of `PORT-ROADMAP.md` brings the start-up hooks, the start-up program and
//! the supervisor.

use std::io::Write;

pub mod applets;
pub mod supervisor;

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
    /// A one-line description for `zbm --list`.
    pub summary: &'static str,
    /// Runs the command with its arguments and returns its exit status.
    pub run: fn(&[String], &mut Io) -> i32,
}

/// The commands `zbm` provides.
pub const APPLETS: &[Applet] = &[
    Applet {
        name: "zfsbootmenu-init",
        summary: "the start-up program, and with --supervise process 1",
        run: applets::init,
    },
    Applet {
        name: "zfsbootmenu-parse-commandline",
        summary: "read the command line into /etc/zfsbootmenu.conf (dracut cmdline hook)",
        run: applets::parse_commandline,
    },
    Applet {
        name: "zfsbootmenu-preinit",
        summary: "set up the console and start the supervisor (dracut pre-mount hook)",
        run: applets::preinit,
    },
];

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

/// Runs `zbm` with the name it was started under and its arguments, using the
/// commands in `applets`.
pub fn run_with(applets: &[Applet], argv0: &str, args: &[String], io: &mut Io) -> i32 {
    let find = |name: &str| applets.iter().find(|applet| applet.name == name);
    let name = command_name(argv0);
    if name != "zbm" {
        return match find(name) {
            Some(applet) => (applet.run)(args, io),
            None => {
                let _ = writeln!(io.stderr, "zbm: {name}: unknown command");
                NOT_FOUND
            }
        };
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
            let _ = writeln!(io.stdout, "zbm {}", zbm_core::version::ZBM_VERSION);
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
        Some(command) => match find(command) {
            Some(applet) => (applet.run)(&args[1..], io),
            None => {
                let _ = writeln!(
                    io.stderr,
                    "zbm: {command}: unknown command (see zbm --list)"
                );
                NOT_FOUND
            }
        },
    }
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

    const TEST_APPLETS: &[Applet] = &[Applet {
        name: "echo",
        summary: "print the arguments",
        run: echo,
    }];

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
            "echo                 print the arguments\n"
        );
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
        assert_eq!(stdout, format!("zbm {}\n", zbm_core::version::ZBM_VERSION));
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
        assert_eq!(
            names,
            [
                "zfsbootmenu-init",
                "zfsbootmenu-parse-commandline",
                "zfsbootmenu-preinit"
            ]
        );
        let (code, _, stderr) = call(APPLETS, "zbm", &["zfsbootmenu-init", "--bogus"]);
        assert_eq!(code, USAGE);
        assert_eq!(stderr, "zfsbootmenu-init: unknown option --bogus\n");
        assert_eq!(command_name("/a/b/"), "b");
    }
}
