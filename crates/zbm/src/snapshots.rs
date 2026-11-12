//! `zsnapshots [ENVIRONMENT]`: the menu on its Snapshots tab, of the boot
//! environment named, else of the one selected (ADR 0018, point 18), as
//! `zfsbootmenu --snapshots [ENVIRONMENT]` opens it, where the legacy
//! script showed the snapshots of any filesystem in `fzf`. It is the menu:
//! the session serves it, so its changes keep the lock, and it ends a menu
//! running elsewhere, as `zfsbootmenu` does.

use crate::menu::{self, Opening};
use crate::table::ZSNAPSHOTS;
use crate::{Io, USAGE};

/// `zsnapshots [ENVIRONMENT]` on this process's terminal.
pub(crate) fn run(args: &[String], io: &mut Io) -> i32 {
    match opening(args) {
        Ok(opening) => menu::show(ZSNAPSHOTS.name, &opening, io),
        Err(None) => {
            let _ = io.stderr.write_all(ZSNAPSHOTS.usage.as_bytes());
            USAGE
        }
        Err(Some(error)) => {
            let _ = writeln!(io.stderr, "{}: {error}", ZSNAPSHOTS.name);
            USAGE
        }
    }
}

/// What `args` open the menu on: the Snapshots tab, of the environment
/// they name, if they name one.
///
/// # Errors
///
/// A usage error: `None` for more than one argument, else what is wrong
/// with the name, which is no filesystem's.
fn opening(args: &[String]) -> Result<Opening, Option<String>> {
    if args.len() > 1 {
        return Err(None);
    }
    let options: Vec<String> = std::iter::once(menu::SNAPSHOTS.to_owned())
        .chain(args.iter().cloned())
        .collect();
    menu::opening(&options).map_err(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn zsnapshots_is_the_menu_on_the_snapshots_tab() {
        let void = "tank/ROOT/void".parse().unwrap();
        assert_eq!(
            opening(&args(&["tank/ROOT/void"])),
            Ok(Opening::Snapshots(Some(void)))
        );
        assert_eq!(opening(&[]), Ok(Opening::Snapshots(None)));
        assert_eq!(opening(&args(&[""])), Ok(Opening::Snapshots(None)));
        assert_eq!(
            opening(&args(&["tank/ROOT/void@now"])),
            Err(Some(
                "tank/ROOT/void@now is not the name of a filesystem".to_owned()
            ))
        );
        assert_eq!(opening(&args(&["a", "b"])), Err(None));
    }

    #[test]
    fn a_wrong_argument_prints_the_usage_and_exits_2() {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        assert_eq!(run(&args(&["a", "b"]), &mut io), USAGE);
        assert_eq!(run(&args(&["tank/a*b"]), &mut io), USAGE);
        let said = String::from_utf8_lossy(&stderr);
        let message = "zsnapshots: tank/a*b is not the name of a filesystem\n";
        assert_eq!(said, format!("{}{message}", ZSNAPSHOTS.usage));
        assert!(stdout.is_empty());
    }
}
