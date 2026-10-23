//! The terminal: `tput`, `stty` and `setfont`.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::io;
use std::path::Path;

use zbm_platform::{Command, Input, Output, Runner, Stream};

/// What `tput ARGS` prints, such as the escape sequence of a capability, or
/// nothing when it fails; its standard input is `input`.
pub fn tput(runner: &dyn Runner, args: &[&str], input: Input) -> Vec<u8> {
    let command = Command::new("tput")
        .args(args.iter().copied())
        .stdin(input)
        .stderr(Stream::Null);
    runner
        .run(&command)
        .map(|output| output.stdout)
        .unwrap_or_default()
}

/// A numeric capability of the terminal, such as `lines` (`tput lines`); 0
/// when it is unknown.
pub fn tput_number(runner: &dyn Runner, capability: &str) -> usize {
    String::from_utf8_lossy(&tput(runner, &[capability], Input::Inherit))
        .trim()
        .parse()
        .unwrap_or(0)
}

/// Changes settings of the console (`stty ARGS`), which is its standard
/// input and output.
pub fn stty(runner: &dyn Runner, args: &[&str]) {
    let command = Command::new("stty")
        .args(args.iter().copied())
        .stdin(Input::Inherit)
        .stdout(Stream::Inherit);
    let _ = runner.run(&command);
}

/// Loads the console font `font` on the virtual console `console`
/// (`setfont -C CONSOLE [-d] FONT`), doubled with `double`. `setfont` fails
/// for a font the kernel cannot show, such as a doubled font where the
/// kernel takes no font that large.
///
/// # Errors
///
/// When `setfont` cannot be run.
pub fn setfont(
    runner: &dyn Runner,
    console: &str,
    font: &Path,
    double: bool,
) -> io::Result<Output> {
    let mut command = Command::new("setfont").args(["-C", console]);
    if double {
        command = command.arg("-d");
    }
    let command = command
        .arg(font.as_os_str())
        .stdin(Input::Null)
        .stdout(Stream::Null)
        .stderr(Stream::Null);
    runner.run(&command)
}

/// The columns and rows of the terminal `device` (`stty -F DEVICE size`), or
/// `None` when `stty` cannot tell.
pub fn size_of(runner: &dyn Runner, device: &str) -> Option<(u16, u16)> {
    let command = Command::new("stty")
        .args(["-F", device, "size"])
        .stdin(Input::Null)
        .stderr(Stream::Null);
    let output = runner.run(&command).ok()?;
    if !output.status.is_success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut numbers = text.split_whitespace().map(str::parse::<u16>);
    let rows = numbers.next()?.ok()?;
    let columns = numbers.next()?.ok()?;
    Some((columns, rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::FakeRunner;

    #[test]
    fn capabilities_and_settings() {
        let runner = FakeRunner::new();
        runner.respond("tput lines", 0, "40\n");
        runner.respond("tput cols", 1, "");
        runner.respond("tput cnorm", 0, "\u{1b}[?25h");

        assert_eq!(tput_number(&runner, "lines"), 40);
        assert_eq!(tput_number(&runner, "cols"), 0);
        assert_eq!(tput(&runner, &["cnorm"], Input::Null), b"\x1b[?25h");
        stty(&runner, &["echo"]);

        assert_eq!(
            runner.lines(),
            ["tput lines", "tput cols", "tput cnorm", "stty echo"]
        );
        let calls = runner.calls();
        assert_eq!(*calls[2].get_stdin(), Input::Null);
        assert_eq!(calls[3].get_stdout(), Stream::Inherit);
        let failing = FakeRunner::new();
        failing.on(|_| Some(Err(std::io::Error::other("no tput"))));
        assert!(tput(&failing, &["clear"], Input::Null).is_empty());
    }

    #[test]
    fn fonts_and_sizes_of_a_console() {
        let runner = FakeRunner::new();
        runner.respond("setfont -C /dev/tty1 /f/ter-v32b.psf", 0, "");
        runner.respond("setfont -C /dev/tty1 -d /f/ter-v32b.psf", 1, "");
        runner.respond("stty -F /dev/tty1 size", 0, "67 240\n");
        runner.respond("stty -F /dev/ttyS0 size", 1, "");
        let font = Path::new("/f/ter-v32b.psf");

        let loaded = |double| {
            setfont(&runner, "/dev/tty1", font, double).is_ok_and(|o| o.status.is_success())
        };
        assert!(loaded(false));
        assert!(!loaded(true));
        assert_eq!(size_of(&runner, "/dev/tty1"), Some((240, 67)));
        assert_eq!(size_of(&runner, "/dev/ttyS0"), None);
        assert_eq!(*runner.calls()[0].get_stdin(), Input::Null);
    }
}
