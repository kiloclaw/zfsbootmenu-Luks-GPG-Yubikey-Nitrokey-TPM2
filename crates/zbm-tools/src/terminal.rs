//! The terminal: `tput` and `stty`.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use zbm_platform::{Command, Input, Runner, Stream};

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
}
