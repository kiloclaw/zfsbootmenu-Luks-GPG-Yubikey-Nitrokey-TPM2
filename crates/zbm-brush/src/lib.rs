//! `brush`, the shell of the images with the Rust runtime: the recovery
//! shell, the shell that runs hooks, and the shell of SSH sessions. It is
//! [brush](https://github.com/reubeno/brush), a Bash-compatible shell
//! written in Rust, unchanged but for its line editor, which this crate
//! chooses for the terminal.
//!
//! brush's richest line editor (reedline) asks the terminal where its
//! cursor is, and ends the shell when no answer comes within two seconds.
//! Virtual terminals and pseudo-terminals (the console on the display, SSH)
//! answer; a serial line answers only when the terminal at its other end
//! does. On a serial line, then, `brush` uses its basic line editor, which
//! asks nothing.

use std::ffi::{OsStr, OsString};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The option that chooses brush's line editor.
pub const BACKEND: &str = "--input-backend";

/// The line editor that asks the terminal nothing.
pub const BASIC: &str = "basic";

/// Whether the terminal at `path`, as `/proc/self/fd/0` names it, answers
/// the queries of brush's richest line editor: a virtual terminal
/// (`/dev/tty1`) or a pseudo-terminal (`/dev/pts/0`). Serial lines
/// (`/dev/ttyS0`, `/dev/ttyAMA0`, `/dev/hvc0`) and `/dev/console`, which may
/// be one, may not.
pub fn is_answering(path: &Path) -> bool {
    let text = path.to_string_lossy();
    if text.starts_with("/dev/pts/") {
        return true;
    }
    text.strip_prefix("/dev/tty")
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// The arguments to run brush with instead of `args` (the program first),
/// when its standard input is the terminal `terminal`: the basic line editor
/// first, on a terminal that may not answer, unless one was chosen. `None`
/// when `args` will do.
pub fn arguments(args: &[OsString], terminal: Option<&Path>) -> Option<Vec<OsString>> {
    let terminal = terminal?;
    let chosen = args.iter().any(|arg| {
        arg == OsStr::new(BACKEND) || arg.to_string_lossy().starts_with("--input-backend=")
    });
    if chosen || is_answering(terminal) {
        return None;
    }
    let (program, rest) = args.split_first()?;
    let mut changed = vec![program.clone(), BACKEND.into(), BASIC.into()];
    changed.extend(rest.iter().cloned());
    Some(changed)
}

/// The terminal that `link`, standard input's entry in `/proc/self/fd`,
/// names, when standard input `is_terminal`.
pub fn terminal_of(link: &Path, is_terminal: bool) -> Option<PathBuf> {
    #[allow(
        clippy::disallowed_methods,
        reason = "the shell reads where its own standard input comes from"
    )]
    let target = std::fs::read_link(link);
    is_terminal.then_some(target.ok()).flatten()
}

/// The restart of this program, `exe`, that [`arguments`] asks for, with
/// the name it was started by; `None` when it needs none.
pub fn restart(exe: &Path, args: &[OsString], terminal: Option<&Path>) -> Option<Command> {
    let changed = arguments(args, terminal)?;
    let (program, rest) = changed.split_first()?;
    #[allow(
        clippy::disallowed_methods,
        reason = "the shell replaces itself, which no Runner does"
    )]
    let mut command = Command::new(exe);
    command.arg0(program).args(rest);
    Some(command)
}

#[cfg(test)]
#[allow(clippy::disallowed_methods, reason = "the tests make a link to read")]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn words(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn virtual_terminals_and_pseudo_terminals_answer() {
        for answering in ["/dev/tty1", "/dev/tty12", "/dev/pts/0"] {
            assert!(is_answering(Path::new(answering)), "{answering}");
        }
        for silent in [
            "/dev/ttyS0",
            "/dev/ttyAMA0",
            "/dev/hvc0",
            "/dev/console",
            "/dev/tty",
        ] {
            assert!(!is_answering(Path::new(silent)), "{silent}");
        }
    }

    #[test]
    fn a_serial_line_gets_the_basic_line_editor() {
        let serial = Some(Path::new("/dev/ttyS0"));
        assert_eq!(
            arguments(&words(&["brush", "-i"]), serial),
            Some(words(&["brush", "--input-backend", "basic", "-i"]))
        );
        // Chosen already, on a terminal that answers, or without one.
        assert_eq!(
            arguments(&words(&["brush", "--input-backend", "reedline"]), serial),
            None
        );
        assert_eq!(
            arguments(&words(&["brush", "--input-backend=minimal"]), serial),
            None
        );
        assert_eq!(
            arguments(&words(&["brush", "-i"]), Some(Path::new("/dev/tty1"))),
            None
        );
        assert_eq!(arguments(&words(&["brush", "-i"]), None), None);
        assert_eq!(arguments(&[], serial), None);
    }

    #[test]
    fn a_serial_line_restarts_the_shell_with_the_basic_editor() {
        let args = words(&["-brush", "-l"]);
        let exe = Path::new("/proc/self/exe");

        let command = restart(exe, &args, Some(Path::new("/dev/ttyS0"))).unwrap();

        assert_eq!(command.get_program(), exe);
        let rest: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(rest, [BACKEND, BASIC, "-l"]);
        assert!(restart(exe, &args, Some(Path::new("/dev/tty1"))).is_none());
    }

    #[test]
    fn the_terminal_is_where_standard_input_points() {
        let dir = std::env::temp_dir().join(format!("zbm-brush-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("0");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink("/dev/ttyS0", &link).unwrap();

        assert_eq!(terminal_of(&link, true), Some(PathBuf::from("/dev/ttyS0")));
        assert_eq!(terminal_of(&link, false), None);
        assert_eq!(terminal_of(&dir.join("missing"), true), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
