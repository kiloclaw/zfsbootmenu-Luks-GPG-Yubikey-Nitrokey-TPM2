//! `zbm console`: the console on the display, without the kernel's
//! (ROADMAP.md #19, step 2), which the supervisor starts when
//! `zbm.console=drm` asks for it.
//!
//! Its standard input is the controlling side of the pseudo-terminal on
//! which the start-up program and the recovery shell run. It draws them on
//! the display through DRM with the image's console font, at the size
//! `zbm console-autosize` would choose, and sends them what is typed on the
//! keyboards, with the virtual terminal shown now taken over meanwhile.
//! `zbm console release`, before `kexec`, has it hand all of that back; it
//! then relays between the pseudo-terminal and the kernel console, which
//! it also does at once when it cannot use the display.
//!
//! ```text
//! zbm console [--card <path>] [--height <pixels>] [--mirror <file>]
//! zbm console release [--timeout <milliseconds>]
//! ```
//!
//! `--mirror` writes the screen's text to a file or device after each
//! change, after a form feed, for the VM tests.

use std::path::{Path, PathBuf};
use std::time::Duration;

use zbm_console::serve::{Places, Released, Settings, release, serve};
use zbm_platform::RealFiles;

use crate::{Io, USAGE};

/// The usage text.
const USAGE_TEXT: &str = "\
Usage: zbm console [--card <path>] [--height <pixels>] [--mirror <file>]
       zbm console release [--timeout <milliseconds>]
";

/// How long `zbm console release` waits by default.
const RELEASE_TIMEOUT: Duration = Duration::from_secs(3);

/// What the command line asks for.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Options {
    /// The DRM card; the first otherwise.
    pub card: Option<PathBuf>,
    /// The font height asked for, as `zbm.autosize=<pixels>`.
    pub height: Option<u16>,
    /// Where to write the screen's text.
    pub mirror: Option<PathBuf>,
}

/// The options in `args`, or `None` when one is wrong.
pub fn parse(args: &[String]) -> Option<Options> {
    let mut options = Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--card" => options.card = Some(PathBuf::from(args.next()?)),
            "--height" => options.height = Some(args.next()?.parse().ok()?),
            "--mirror" => options.mirror = Some(PathBuf::from(args.next()?)),
            _ => return None,
        }
    }
    Some(options)
}

/// The timeout of `zbm console release` in `args`, or `None` when an
/// option is wrong.
pub fn parse_release(args: &[String]) -> Option<Duration> {
    match args {
        [] => Some(RELEASE_TIMEOUT),
        [option, millis] if option == "--timeout" => millis.parse().ok().map(Duration::from_millis),
        _ => None,
    }
}

/// `zbm console`, or `zbm console release`.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    if args.first().map(String::as_str) == Some("release") {
        return run_release(args.get(1..).unwrap_or(&[]), io);
    }
    let Some(options) = parse(args) else {
        let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
        return USAGE;
    };
    let settings = Settings {
        card: options.card,
        mirror: options.mirror,
    };
    let places = Places::system();
    let files = RealFiles;
    let mut grid = |size| crate::menu_demo::grid(&files, Path::new("/"), options.height, size);
    match serve(&settings, &places, &mut grid) {
        Ok(()) => 0,
        Err(error) => {
            places.log(3, &error.to_string());
            let _ = writeln!(io.stderr, "zbm console: {error}");
            1
        }
    }
}

/// `zbm console release`: asks the console to hand the display back, and
/// waits until it did. Nothing to do without a console.
fn run_release(args: &[String], io: &mut Io) -> i32 {
    let Some(timeout) = parse_release(args) else {
        let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
        return USAGE;
    };
    match release(&Places::system().base, timeout) {
        Ok(Released::Done) => 0,
        Ok(Released::Late) => {
            let _ = writeln!(io.stderr, "zbm console: the display was not handed back");
            1
        }
        Err(error) => {
            let _ = writeln!(io.stderr, "zbm console: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn options_are_parsed_and_wrong_ones_refused() {
        assert_eq!(
            parse(&args(
                "--card /dev/dri/card1 --height 32 --mirror /dev/ttyS1"
            )),
            Some(Options {
                card: Some(PathBuf::from("/dev/dri/card1")),
                height: Some(32),
                mirror: Some(PathBuf::from("/dev/ttyS1")),
            })
        );
        assert_eq!(parse(&[]), Some(Options::default()));
        assert_eq!(parse(&args("--height tall")), None);
        assert_eq!(parse(&args("--mirror")), None);
        assert_eq!(parse(&args("--loud")), None);

        assert_eq!(parse_release(&[]), Some(RELEASE_TIMEOUT));
        assert_eq!(
            parse_release(&args("--timeout 500")),
            Some(Duration::from_millis(500))
        );
        assert_eq!(parse_release(&args("--timeout soon")), None);
        assert_eq!(parse_release(&args("--now")), None);
    }

    #[test]
    fn wrong_options_show_the_usage() {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };

        assert_eq!(run(&args("--loud"), &mut io), USAGE);
        // No console runs on the machine of the tests: nothing to hand back.
        assert_eq!(run(&args("release --timeout 10"), &mut io), 0);
        assert_eq!(run(&args("release --now"), &mut io), USAGE);
        let text = String::from_utf8_lossy(&stderr).into_owned();
        assert_eq!(text.matches("Usage: zbm console").count(), 2, "{text}");
    }
}
