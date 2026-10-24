//! Colours and the timed prompt.
//!
//! The timed prompt, and the colours of `zbm-core`. The prompt draws centred
//! lines with `tput`; `tput`'s output is captured and written to the console,
//! so tests see it.

use std::time::Duration;

use zbm_platform::System;
use zbm_platform::exec::Input;
use zbm_tools::terminal;

pub use zbm_core::color::{colorize, decolorize};

/// The delay of a retry prompt, `-d "${zbm_retry_delay:-5}"`. The start-up
/// writes only positive numbers; a value that is not a number is read as 0,
/// which answers at once.
pub fn retry_delay(value: &str) -> i64 {
    match value {
        "" => 5,
        delay => delay.trim().parse().unwrap_or(0),
    }
}

/// The text of the countdown line for `seconds` left.
type PromptText = Box<dyn Fn(u64) -> String>;

/// A centred prompt that waits for RETURN or ESCAPE, counting down.
pub struct TimedPrompt {
    delay: Option<i64>,
    prompt: Option<PromptText>,
    messages: Vec<String>,
}

/// What the user did: [`Answer::Continue`] for RETURN or when the time ran
/// out (status 0), [`Answer::Escape`] for ESCAPE (status 1).
pub use zbm_types::Answer;

impl Default for TimedPrompt {
    fn default() -> TimedPrompt {
        TimedPrompt::new()
    }
}

impl TimedPrompt {
    /// A prompt with the default delay of 30 seconds and no messages.
    pub fn new() -> TimedPrompt {
        TimedPrompt {
            delay: None,
            prompt: None,
            messages: Vec::new(),
        }
    }

    /// `-d`: the delay in seconds; 0 answers at once, a negative delay waits
    /// without limit.
    pub fn delay(mut self, seconds: i64) -> TimedPrompt {
        self.delay = Some(seconds);
        self
    }

    /// `-p`: the countdown line, given the seconds left.
    pub fn prompt(mut self, text: impl Fn(u64) -> String + 'static) -> TimedPrompt {
        self.prompt = Some(Box::new(text));
        self
    }

    /// `-m`: a line of text.
    pub fn message(mut self, text: impl Into<String>) -> TimedPrompt {
        self.messages.push(text.into());
        self
    }

    /// `-r`: what RETURN does.
    pub fn on_return(self, text: &str) -> TimedPrompt {
        let line = format!("{} {text}", colorize("green", "[RETURN]"));
        self.message(line)
    }

    /// `-e`: what ESCAPE does.
    pub fn on_escape(self, text: &str) -> TimedPrompt {
        let line = format!("{} {text}", colorize("red", "[ESCAPE]"));
        self.message(line)
    }

    /// Shows the prompt and waits for RETURN or ESCAPE until the delay runs
    /// out.
    pub fn run(self, system: &System<'_>) -> Answer {
        let (delay, infinite) = match self.delay {
            Some(0) => return Answer::Continue,
            Some(delay) if delay < 0 => (30, true),
            Some(delay) => (delay.unsigned_abs(), false),
            None => (30, false),
        };
        let width = delay.to_string().len();
        let prompt = self
            .prompt
            .unwrap_or_else(|| default_prompt(infinite, width));
        let mut messages = self.messages;
        messages.push(String::new());

        tput(system, &["civis"]);
        let height = tput_number(system, "lines");
        let columns = tput_number(system, "cols");
        tput(system, &["clear"]);

        let lines = i64::try_from(messages.len()).unwrap_or(i64::MAX);
        let mut row = (height / 2 - lines).max(0);
        for message in &messages {
            draw_centred(system, row, columns, message);
            row += 1;
        }
        let countdown = Countdown {
            prompt,
            delay,
            infinite,
            row,
            columns,
        };
        countdown.wait(system)
    }
}

/// The countdown line of a prompt without `-p`: how to continue, and the
/// seconds left unless the prompt waits without limit. `width` is the number
/// of digits the seconds take.
fn default_prompt(infinite: bool, width: usize) -> PromptText {
    Box::new(move |seconds| {
        let mut text = format!("Press {} to continue", colorize("green", "[RETURN]"));
        if !infinite {
            let left = colorize("yellow", &format!("{seconds:0width$}"));
            text.push_str(&format!(" or wait {left} seconds"));
        }
        text
    })
}

/// Writes `text` on `row` of the screen, centred in its `columns`.
fn draw_centred(system: &System<'_>, row: i64, columns: i64, text: &str) {
    let len = i64::try_from(decolorize(text).chars().count()).unwrap_or(0);
    let column = ((columns - len) / 2).max(0);
    tput(system, &["cup", &row.to_string(), &column.to_string()]);
    let _ = system.console.write(text.as_bytes());
}

/// The last line of a timed prompt, counting down while it waits for a key.
struct Countdown {
    /// The text of the line for the seconds left.
    prompt: PromptText,
    /// The seconds to wait.
    delay: u64,
    /// Whether the prompt waits without limit.
    infinite: bool,
    /// The screen row of the line.
    row: i64,
    /// The width of the screen.
    columns: i64,
}

impl Countdown {
    /// Redraws the line every second and waits for RETURN or ESCAPE, until
    /// the time runs out.
    fn wait(&self, system: &System<'_>) -> Answer {
        let timeout = (!self.infinite).then_some(Duration::from_secs(1));
        let mut left = self.delay;
        while left > 0 {
            let text = (self.prompt)(left);
            draw_centred(system, self.row, self.columns, &text);
            match system.console.read_key(timeout) {
                Ok(Some(0x1b)) => return Answer::Escape,
                Ok(Some(b'\n')) => return Answer::Continue,
                // The end of the input: `read` fails at once, every time, so the
                // legacy loop would spin; a terminal never ends, so stopping here
                // changes nothing a user can see
                Ok(None) | Err(_) if self.infinite => return Answer::Continue,
                _ => {}
            }
            if !self.infinite {
                left -= 1;
            }
        }
        Answer::Continue
    }
}

/// Runs `tput` with `args` and writes what it prints to the console.
pub fn tput(system: &System<'_>, args: &[&str]) {
    let output = terminal::tput(system.runner, args, Input::Inherit);
    let _ = system.console.write(&output);
}

/// The timed prompts of the LUKS code, drawn on the console as
/// [`TimedPrompt`]s.
pub struct ConsolePrompter<'a> {
    system: System<'a>,
}

impl<'a> ConsolePrompter<'a> {
    /// Prompts on `system`'s console.
    pub fn new(system: System<'a>) -> ConsolePrompter<'a> {
        ConsolePrompter { system }
    }
}

impl zbm_luks::unlocker::Prompter for ConsolePrompter<'_> {
    fn timed_prompt(&self, request: &zbm_luks::unlocker::PromptRequest) -> Answer {
        let mut prompt = TimedPrompt::new();
        if let Some(delay) = &request.delay {
            prompt = prompt.delay(retry_delay(delay));
        }
        prompt
            .message(colorize("red", &request.message))
            .on_return(&request.on_return)
            .on_escape(&request.on_escape)
            .prompt(|seconds| {
                format!(
                    "Retrying in {} seconds",
                    colorize("yellow", &format!("{seconds:02}"))
                )
            })
            .run(&self.system)
    }
}

/// `$( tput <capability> )` as a number, 0 when it cannot be read.
fn tput_number(system: &System<'_>, capability: &str) -> i64 {
    i64::try_from(terminal::tput_number(system.runner, capability)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_platform::clock::Clock;
    use zbm_testkit::fakes::FakeSystem;
    use zbm_testkit::legacy::{Bash, LOG_STUBS};
    use zbm_testkit::parity;

    fn legacy_core() -> Bash {
        Bash::new()
            .code(LOG_STUBS)
            .code(
                r#"source() {
                    case "$1" in
                      /lib/*) builtin source "$LEGACY/zfsbootmenu/lib/${1#/lib/}" ;;
                      *) builtin source "$@" ;;
                    esac
                  }"#,
            )
            .source("zfsbootmenu/lib/zfsbootmenu-core.sh")
    }

    #[test]
    fn a_default_prompt_is_a_new_one() {
        let fake = zbm_testkit::fakes::FakeSystem::new();
        assert_eq!(TimedPrompt::default().run(&fake.system()), Answer::Continue);
    }

    #[test]
    fn colours_match_the_legacy_functions() {
        let mut parity = parity!("colours_match_the_legacy_functions");
        for color in [
            "red",
            "lightcyan",
            "white",
            "yellow",
            "nothing",
            "black",
            "darkgray",
        ] {
            let (colorized, decolorized): (String, String) =
                parity.legacy(format!("colorize {color}"), || {
                    let colorized = legacy_core()
                        .call("colorize", &[color, "tank/ROOT/void"], b"")
                        .stdout_text();
                    let decolorized = legacy_core()
                        .call("decolorize", &[&colorized], b"")
                        .stdout_text();
                    (colorized, decolorized)
                });
            assert_eq!(colorize(color, "tank/ROOT/void"), colorized, "{color}");
            assert_eq!(format!("{}\n", decolorize(&colorized)), decolorized);
        }
        for text in [
            "\\033[1;31mred\\033[0m",
            "a\x01[0;35mb",
            "\x1b[2;31mkept",
            "[0m",
        ] {
            let legacy: String = parity.legacy(format!("decolorize {text:?}"), || {
                legacy_core().call("decolorize", &[text], b"").stdout_text()
            });
            assert_eq!(format!("{}\n", decolorize(text)), legacy, "{text:?}");
        }
    }

    fn setup() -> FakeSystem {
        let fake = FakeSystem::new();
        fake.runner.respond("tput lines", 0, "25\n");
        fake.runner.respond("tput cols", 0, "80\n");
        fake
    }

    #[test]
    fn counts_down_and_continues() {
        let fake = setup();
        fake.console.timeout().timeout().timeout();
        let answer = TimedPrompt::new()
            .delay(3)
            .message("Unsupported features detected")
            .run(&fake.system());
        assert_eq!(answer, Answer::Continue);
        let output = fake.console.output_text();
        assert!(output.contains("Unsupported features detected"));
        assert!(output.contains("or wait \x1b[1;33m1\x1b[0m seconds"));
        assert_eq!(fake.clock.now(), Duration::from_secs(3));
        // Two lines (the message and a blank) centred on 25 rows: rows 10 and 11,
        // then the countdown on row 12.
        let lines = fake.runner.lines();
        assert_eq!(
            lines[..4],
            ["tput civis", "tput lines", "tput cols", "tput clear"]
        );
        assert_eq!(lines[4], format!("tput cup 10 {}", (80 - 29) / 2));
        assert_eq!(lines[5], "tput cup 11 40");
        assert!(lines[6].starts_with("tput cup 12 "));
        assert!(fake.console.remaining().is_empty());
    }

    #[test]
    fn keys_answer_the_prompt() {
        let fake = setup();
        fake.console.keys(b"x\n");
        let answer = TimedPrompt::new()
            .delay(5)
            .prompt(|seconds| format!("retrying in {seconds:02} seconds"))
            .on_return("to retry immediately")
            .on_escape("for a recovery shell")
            .run(&fake.system());
        assert_eq!(answer, Answer::Continue);
        let output = fake.console.output_text();
        assert!(output.contains("retrying in 05 seconds"));
        // Any other key counts as a second.
        assert!(output.contains("retrying in 04 seconds"));
        assert!(output.contains("\x1b[0;32m[RETURN]\x1b[0m to retry immediately"));

        let fake = setup();
        fake.console.keys(&[0x1b]);
        assert_eq!(
            TimedPrompt::new().delay(5).run(&fake.system()),
            Answer::Escape
        );
        assert_eq!(
            TimedPrompt::new().delay(0).run(&fake.system()),
            Answer::Continue
        );
    }

    #[test]
    fn luks_prompts_count_down_from_their_delay() {
        use zbm_luks::unlocker::{PromptRequest, Prompter};
        let fake = setup();
        fake.console.timeout().keys(&[0x1b]);
        let request = PromptRequest {
            delay: Some("5".to_owned()),
            message: "LUKS device LABEL=x is missing".to_owned(),
            on_return: "retry now".to_owned(),
            on_escape: "skip this device".to_owned(),
        };
        assert_eq!(
            ConsolePrompter::new(fake.system()).timed_prompt(&request),
            Answer::Escape
        );
        let output = fake.console.output_text();
        assert!(output.contains("\x1b[0;31mLUKS device LABEL=x is missing\x1b[0m"));
        assert!(output.contains("Retrying in \x1b[1;33m05\x1b[0m seconds"));
        assert!(output.contains("Retrying in \x1b[1;33m04\x1b[0m seconds"));
        assert!(output.contains("[ESCAPE]\x1b[0m skip this device"));
    }

    #[test]
    fn an_infinite_prompt_waits_for_a_key() {
        let fake = setup();
        fake.console.keys(b"abc\x1b");
        let answer = TimedPrompt::new()
            .delay(-1)
            .message("ZFSBootMenu must be initialized to continue")
            .run(&fake.system());
        assert_eq!(answer, Answer::Escape);
        let output = fake.console.output_text();
        assert!(output.contains("Press \x1b[0;32m[RETURN]\x1b[0m to continue"));
        assert!(!output.contains("or wait"));
        // Without a timeout, the end of the input ends the wait.
        let fake = setup();
        assert_eq!(
            TimedPrompt::new().delay(-1).run(&fake.system()),
            Answer::Continue
        );
    }
}
