//! The warnings of the image's plan among what dracut or mkinitcpio printed.
//!
//! The dracut module and the mkinitcpio hook print each warning of the plan
//! in their builder's words for a warning (`dracut[W]: `, `==> WARNING: `),
//! its text starting with [`WARNING_MARK`]. A build that succeeds shows the
//! builder's output only in the debug log; without it, the generator shows
//! the plan's warnings alone, found by their mark, without the builder's
//! words and colours.

use zbm_types::Printable;

use super::Generator;
use crate::image::WARNING_MARK;

/// The character that starts an escape sequence.
const ESC: char = '\u{1b}';

/// The character that ends an operating system command.
const BEL: char = '\u{7}';

impl Generator<'_, '_, '_> {
    /// Prints on standard error each warning of the image's plan in the
    /// builder's output `lines`, as `WARNING: TEXT`, once, in order.
    pub(super) fn show_plan_warnings(&mut self, lines: &[Vec<u8>]) {
        for warning in plan_warnings(lines) {
            let _ = writeln!(self.io.stderr, "WARNING: {}", Printable(&warning));
        }
    }
}

/// The texts of the plan's warnings in `lines`, each once, in order: what
/// follows the mark, without escape sequences.
fn plan_warnings(lines: &[Vec<u8>]) -> Vec<String> {
    let mut warnings: Vec<String> = Vec::new();
    for line in lines {
        let line = without_escapes(&String::from_utf8_lossy(line));
        let Some((_, text)) = line.split_once(WARNING_MARK) else {
            continue;
        };
        let text = text.trim_end();
        if !warnings.iter().any(|warning| warning == text) {
            warnings.push(text.to_owned());
        }
    }
    warnings
}

/// `text` without the escape sequences that colour it: a control sequence
/// (`ESC [`, up to its final character), an operating system command
/// (`ESC ]`, up to BEL or `ESC \`), a character set (`ESC (` and the set,
/// as `tput sgr0` writes it), and any other escape with the character after
/// it.
fn without_escapes(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != ESC {
            plain.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                let mut after_esc = false;
                for c in chars.by_ref() {
                    if c == BEL || after_esc {
                        break;
                    }
                    after_esc = c == ESC;
                }
            }
            Some('(' | ')' | '*' | '+') => {
                chars.next();
            }
            _ => {}
        }
    }
    plain
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate_zbm::config::Value;
    use crate::generate_zbm::generate::invocation::Invocation;
    use crate::generate_zbm::generate::{Context, Io};
    use zbm_testkit::FakeRunner;

    /// What dracut prints with `-q`, as dracut-ng's logger writes a warning
    /// (`dracut[W]: `), the plan's among others.
    const DRACUT: &str = "dracut[W]: [zfsbootmenu] optional component 'mbuffer' not found\n\
                          dracut[W]: dracut module 'resume' will not be installed\n\
                          dracut[E]: Module 'systemd' cannot be installed.\n\
                          dracut[W]: [zfsbootmenu] LUKS configuration was not fully installed\n";

    /// What mkinitcpio prints, as its `warning` writes (`==> WARNING: `),
    /// in the colours it uses on a terminal: bold yellow, then bold for the
    /// message, `tput sgr0` (`ESC ( B ESC [ m`) after each.
    const MKINITCPIO: &str = "==> Starting build: '6.1.0'\n  \
         -> Running build hook: [zfsbootmenu]\n\
         \x1b[1m\x1b[33m==> WARNING:\x1b(B\x1b[m \x1b[1m[zfsbootmenu] optional component \
         'mbuffer' not found\x1b(B\x1b[m\n\
         ==> WARNING: Possibly missing firmware for module: 'qla2xxx'\n\
         ==> WARNING: [zfsbootmenu] LUKS configuration was not fully installed\n\
         ==> Image generation successful\n";

    /// What the generator shows of either builder's output.
    const SHOWN: &str = "WARNING: optional component 'mbuffer' not found\n\
                         WARNING: LUKS configuration was not fully installed\n";

    /// Standard output and standard error of a build step whose `program`
    /// prints `output` and exits with `status`.
    fn build(program: &str, output: &str, status: u8, debug: bool) -> (String, String) {
        let runner = FakeRunner::new();
        runner.respond_program(program, status, output);
        let no_signal = || None::<zbm_sys::Signal>;
        let context = Context::from_environment(&runner, "generate-zbm".to_owned(), &no_signal);
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        let mut generator = Generator {
            context: &context,
            io: &mut io,
            debug,
            dry_run: false,
            config: Value::map(Vec::new()),
            mounted: None,
        };
        let _ = generator.run_build_step(&Invocation::new(program), "zfsbootmenu.img");
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
        (text(stdout), text(stderr))
    }

    #[test]
    fn a_plan_warning_is_shown_without_debug() {
        for (program, output) in [("dracut", DRACUT), ("mkinitcpio", MKINITCPIO)] {
            let (stdout, stderr) = build(program, output, 0, false);

            assert_eq!(stdout, "", "{program}");
            assert_eq!(stderr, SHOWN, "{program}");
        }
    }

    #[test]
    fn other_builder_output_stays_hidden_without_debug() {
        let output = "dracut[W]: dracut module 'resume' will not be installed\n\
                      ==> WARNING: Possibly missing firmware for module: 'qla2xxx'\n\
                      dracut[W]: [zfsbootmenu] the plan's\n\
                      [zfsbootmen] almost\n\
                      dracut[W]: [zfsbootmenu]no space\n";

        let (stdout, stderr) = build("dracut", output, 0, false);

        assert_eq!(stdout, "");
        assert_eq!(stderr, "WARNING: the plan's\n");
    }

    /// The debug log is as it was before the plan's warnings were shown:
    /// this guards against a warning shown there twice.
    #[test]
    fn a_plan_warning_is_shown_once_with_debug() {
        for (program, output) in [("dracut", DRACUT), ("mkinitcpio", MKINITCPIO)] {
            let (stdout, stderr) = build(program, output, 0, true);

            assert_eq!(stdout, "", "{program}");
            assert!(!stderr.contains("WARNING: optional"), "{program}: {stderr}");
            let logged = stderr.matches("[zfsbootmenu] optional component").count();
            assert_eq!(logged, 1, "{program}: {stderr}");
            let other = output.lines().nth(1).unwrap();
            assert!(
                stderr.contains(&format!("## {other}\n")),
                "{program}: {stderr}"
            );
        }
    }

    /// A failed build prints its output as it did before the plan's
    /// warnings were shown: this guards against a warning printed twice.
    #[test]
    fn a_failed_build_prints_all_its_output_once() {
        let (stdout, stderr) = build("dracut", DRACUT, 1, false);

        assert_eq!(
            stdout,
            format!("{DRACUT}Failed to create zfsbootmenu.img\n")
        );
        assert_eq!(stderr, "");
    }

    #[test]
    fn a_warning_said_twice_is_shown_once_in_its_place() {
        let output = "dracut[W]: [zfsbootmenu] b\n\
                      dracut[W]: [zfsbootmenu] a\n\
                      dracut[W]: [zfsbootmenu] b\n";

        let (_, stderr) = build("dracut", output, 0, false);

        assert_eq!(stderr, "WARNING: b\nWARNING: a\n");
    }

    #[test]
    fn what_is_left_of_a_warning_that_could_act_on_a_terminal_is_escaped() {
        let output = "dracut[W]: [zfsbootmenu] hook /x\x1b]0;title\x07/\x07y.sh\x1b\n";

        let (_, stderr) = build("dracut", output, 0, false);

        assert_eq!(stderr, "WARNING: hook /x/\\x07y.sh\n");
    }

    #[test]
    fn escape_sequences_are_left_out() {
        for (text, plain) in [
            ("\x1b[1;33mbold\x1b[0m", "bold"),
            ("\x1b(Bset\x1b)0", "set"),
            ("\x1b]0;title\x1b\\osc", "osc"),
            ("\x1b]0;title\x07osc", "osc"),
            ("\x1b]0;unended", ""),
            ("\x1b[1", ""),
            ("\x1bcreset", "reset"),
            ("end\x1b", "end"),
            ("plain", "plain"),
        ] {
            assert_eq!(without_escapes(text), plain, "{text:?}");
        }
    }
}
