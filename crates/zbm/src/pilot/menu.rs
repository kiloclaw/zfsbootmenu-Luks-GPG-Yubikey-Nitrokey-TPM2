//! `zbm-menu-pilot`: the boot environment list of the new menu, on the
//! terminal it is run on.
//!
//! It shows the boot environments of the imported pools (or, with `--demo`,
//! those of [`Snapshot::example`]) and prints the one chosen; it boots
//! nothing. With `--stats` it prints how long frames took to draw, which is
//! what the pilot measures on a serial console.

use zbm_app::{App, Snapshot};
use zbm_platform::{Context, RealFiles, SystemClock, SystemRunner};
use zbm_tui::Request;
use zbm_tui::terminal::{self, Options};

use crate::{Io, USAGE as USAGE_STATUS};

/// The usage of `zbm-menu-pilot`.
pub const USAGE: &str = "\
Usage: zbm-menu-pilot [options]

Shows the boot environments in the menu of the refactoring (REFACTOR-ROADMAP.md,
pilot) and prints the one chosen. Nothing is booted.

Options:
  --demo            show an example machine instead of the imported pools
  --ascii           draw with ASCII characters only (the default when TERM is
                    vt100, vt102, vt220 or dumb)
  --size COLSxROWS  the screen size, when the terminal does not report one
  --stats           print how long frames took to draw
";

/// Terminals that draw no line-drawing characters.
const ASCII_TERMINALS: &[&str] = &["vt100", "vt102", "vt220", "dumb"];

/// What `zbm-menu-pilot` was asked to do.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MenuArgs {
    /// Show [`Snapshot::example`].
    pub demo: bool,
    /// How to run the menu.
    pub options: Options,
    /// Print frame timings.
    pub stats: bool,
}

/// Parses the arguments of `zbm-menu-pilot`; `term` is the value of `TERM`.
///
/// # Errors
///
/// A message saying what is wrong with the arguments.
pub fn parse(args: &[String], term: Option<&str>) -> Result<MenuArgs, String> {
    let mut parsed = MenuArgs::default();
    parsed.options.ascii = term.is_some_and(|term| ASCII_TERMINALS.contains(&term));
    let mut words = args.iter().map(String::as_str);
    while let Some(word) = words.next() {
        match word {
            "--demo" => parsed.demo = true,
            "--ascii" => parsed.options.ascii = true,
            "--stats" => parsed.stats = true,
            "--size" => {
                let size = words.next().ok_or("--size needs COLSxROWS")?;
                parsed.options.size = Some(parse_size(size)?);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(parsed)
}

/// `COLSxROWS` as columns and rows.
fn parse_size(text: &str) -> Result<(u16, u16), String> {
    let invalid = || format!("{text:?} is not COLSxROWS, such as 80x24");
    let (columns, rows) = text.split_once('x').ok_or_else(invalid)?;
    let columns: u16 = columns.parse().map_err(|_| invalid())?;
    let rows: u16 = rows.parse().map_err(|_| invalid())?;
    if columns == 0 || rows == 0 {
        return Err(invalid());
    }
    Ok((columns, rows))
}

/// `zbm-menu-pilot ARGS`.
#[allow(
    clippy::disallowed_methods,
    reason = "a front end reads TERM to choose its drawing characters"
)]
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let term = std::env::var("TERM").ok();
    let parsed = match parse(args, term.as_deref()) {
        Ok(parsed) => parsed,
        Err(message) => {
            let _ = write!(io.stderr, "zbm-menu-pilot: {message}\n\n{USAGE}");
            return USAGE_STATUS;
        }
    };
    let snapshot = if parsed.demo {
        Snapshot::example()
    } else {
        let (runner, files, clock) = (SystemRunner, RealFiles, SystemClock::new());
        match App::new(Context::from_parts(&runner, &files, &clock)).snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let _ = writeln!(io.stderr, "zbm-menu-pilot: {error}");
                return 1;
            }
        }
    };
    match terminal::run(&snapshot, parsed.options) {
        Ok((request, stats)) => {
            let _ = match request {
                Request::Boot(name) => writeln!(io.stdout, "chosen: {name}"),
                Request::Quit => writeln!(io.stdout, "left the menu"),
            };
            if parsed.stats {
                let _ = writeln!(io.stderr, "{stats}");
            }
            0
        }
        Err(error) => {
            let _ = writeln!(io.stderr, "zbm-menu-pilot: the terminal failed: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn options_parse() {
        let parsed = parse(&args("--demo --size 100x30 --stats"), Some("linux")).unwrap();

        assert!(parsed.demo);
        assert!(parsed.stats);
        assert!(!parsed.options.ascii);
        assert_eq!(parsed.options.size, Some((100, 30)));
    }

    #[test]
    fn simple_terminals_get_ascii_drawing() {
        assert!(parse(&[], Some("vt100")).unwrap().options.ascii);
        assert!(parse(&[], Some("dumb")).unwrap().options.ascii);
        assert!(!parse(&[], Some("xterm-256color")).unwrap().options.ascii);
        assert!(!parse(&[], None).unwrap().options.ascii);
    }

    #[test]
    fn invalid_arguments_say_what_is_wrong() {
        let cases = [
            ("--size", "--size needs COLSxROWS"),
            ("--size 80", "\"80\" is not COLSxROWS, such as 80x24"),
            ("--size 0x24", "\"0x24\" is not COLSxROWS, such as 80x24"),
            ("--fast", "unknown argument --fast"),
        ];

        for (line, message) in cases {
            assert_eq!(parse(&args(line), None).unwrap_err(), message, "{line}");
        }
    }
}
