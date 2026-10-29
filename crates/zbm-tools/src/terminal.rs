//! The console font: `setfont`. The terminal's settings and size are the
//! console port's (`zbm_platform::console`), without `tput` and `stty`.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::io;
use std::path::Path;

use zbm_platform::{Command, Input, Output, Runner, Stream};

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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::FakeRunner;

    #[test]
    fn fonts_of_a_console() {
        let runner = FakeRunner::new();
        runner.respond("setfont -C /dev/tty1 /f/ter-v32b.psf", 0, "");
        runner.respond("setfont -C /dev/tty1 -d /f/ter-v32b.psf", 1, "");
        let font = Path::new("/f/ter-v32b.psf");

        let loaded = |double| {
            setfont(&runner, "/dev/tty1", font, double).is_ok_and(|o| o.status.is_success())
        };
        assert!(loaded(false));
        assert!(!loaded(true));
        assert_eq!(*runner.calls()[0].get_stdin(), Input::Null);
    }
}
