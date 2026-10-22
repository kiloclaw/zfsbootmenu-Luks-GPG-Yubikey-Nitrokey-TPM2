//! `udevadm`.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use zbm_platform::{Command, Input, Runner, Stream};

use crate::Shown;

/// Waits until udev has handled the events it has (`udevadm settle`); with
/// [`Shown::Console`] its messages go to the console.
pub fn settle(runner: &dyn Runner, output: Shown) {
    let command = Command::new("udevadm").arg("settle");
    let command = match output {
        Shown::Console => command.stdout(Stream::Inherit),
        Shown::Hidden => command
            .stdin(Input::Null)
            .stdout(Stream::Null)
            .stderr(Stream::Null),
    };
    let _ = runner.run(&command);
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::FakeRunner;

    #[test]
    fn settling_shows_or_hides_the_output() {
        let runner = FakeRunner::new();

        settle(&runner, Shown::Console);
        settle(&runner, Shown::Hidden);

        assert_eq!(runner.lines(), ["udevadm settle", "udevadm settle"]);
        let calls = runner.calls();
        assert_eq!(calls[0].get_stdout(), Stream::Inherit);
        assert_eq!(calls[1].get_stdout(), Stream::Null);
    }
}
