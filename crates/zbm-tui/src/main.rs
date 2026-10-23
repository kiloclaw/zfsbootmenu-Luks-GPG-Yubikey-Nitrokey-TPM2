//! `zbm-tui --demo`: the prototype of the new menu on a fake machine
//! (`docs/dev/menu-design/`).

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_macros,
    reason = "the demo is a program: it reads its environment, writes the key log and prints"
)]

use std::fs::File;
use std::process::ExitCode;

use zbm_tui::menu::caps::Capabilities;
use zbm_tui::menu::demo::{DemoOptions, Session};
use zbm_tui::menu::terminal;
use zbm_tui::menu::theme::Theme;

/// The text of `zbm-tui --help`.
const USAGE: &str = "\
Usage: zbm-tui --demo [options]

Runs the new menu of ZFSBootMenu on a fake machine: two pools, four boot
environments, 400 snapshots, three LUKS volumes (passphrase demo, TPM2 PIN
1234). Nothing on this computer is touched.

Options:
  --locked               start with the LUKS volumes locked
  --foreign              add a pool of another host, which does not import
  --wait                 no countdown: wait for a key
  --failing              the default used up its boot tries: the countdown
                         boots a clone of its snapshot pre-upgrade instead
  --next-boot            the running system asked to boot tank/ROOT/arch once
  --tpm-changed          the firmware was updated: the TPM keeps the key of
                         luks-zroot-a, and the passphrase is asked instead
  --theme <name>         colour, console, mono or ascii (default: from TERM,
                         NO_COLOR and the locale)
  --size <cols>x<rows>   the size when the terminal reports none
  --log-keys <file>      log every key press with its time, for usability
                         sessions
";

/// What the command line asks for.
#[derive(Debug, Default)]
struct Arguments {
    options: DemoOptions,
    theme: Option<Capabilities>,
    size: Option<(u16, u16)>,
    keys: Option<String>,
}

/// Parses the arguments after the program's name.
fn parse(args: &[String]) -> Result<Arguments, String> {
    let mut parsed = Arguments::default();
    let mut demo = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => demo = true,
            "--locked" => parsed.options.locked = true,
            "--foreign" => parsed.options.foreign = true,
            "--wait" => parsed.options.timeout = None,
            "--failing" => parsed.options.failing = true,
            "--next-boot" => parsed.options.next_boot = true,
            "--tpm-changed" => parsed.options.tpm_changed = true,
            "--theme" => {
                let name = args.next().ok_or("--theme needs a name")?;
                parsed.theme =
                    Some(Capabilities::named(name).ok_or(format!("unknown theme {name}"))?);
            }
            "--size" => {
                let size = args.next().ok_or("--size needs <cols>x<rows>")?;
                parsed.size = Some(parse_size(size).ok_or(format!("bad size {size}"))?);
            }
            "--log-keys" => {
                parsed.keys = Some(args.next().ok_or("--log-keys needs a file")?.clone());
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if demo { Ok(parsed) } else { Err(String::new()) }
}

/// `80x24` as columns and rows.
fn parse_size(text: &str) -> Option<(u16, u16)> {
    let (columns, rows) = text.split_once('x')?;
    Some((columns.parse().ok()?, rows.parse().ok()?))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arguments = match parse(&args) {
        Ok(arguments) => arguments,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("zbm-tui: {message}");
            }
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let capabilities = arguments
        .theme
        .unwrap_or_else(|| Capabilities::detect(|name| std::env::var(name).ok()));
    let keys = match arguments.keys.as_deref().map(File::create).transpose() {
        Ok(keys) => keys,
        Err(error) => {
            eprintln!("zbm-tui: --log-keys: {error}");
            return ExitCode::FAILURE;
        }
    };
    let session = Session::new(&arguments.options, Theme::new(capabilities));
    let size = terminal::size(arguments.size.unwrap_or((80, 24)));
    match terminal::run(session, size, keys) {
        Ok(why) => {
            println!("zbm-tui demo: {why}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("zbm-tui: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(line: &str) -> Result<Arguments, String> {
        let args: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
        parse(&args)
    }

    #[test]
    fn the_demo_options_are_parsed() {
        let line = "--demo --locked --foreign --wait --failing --next-boot --tpm-changed \
                    --theme ascii --size 52x20 --log-keys k";
        let parsed = arguments(line).unwrap();

        assert!(parsed.options.locked && parsed.options.foreign);
        assert!(parsed.options.failing && parsed.options.next_boot);
        assert!(parsed.options.tpm_changed);
        assert_eq!(parsed.options.timeout, None);
        assert_eq!(parsed.theme, Some(Capabilities::ASCII));
        assert_eq!(parsed.size, Some((52, 20)));
        assert_eq!(parsed.keys.as_deref(), Some("k"));
    }

    #[test]
    fn without_demo_or_with_bad_options_the_usage_is_shown() {
        assert_eq!(arguments("").err().as_deref(), Some(""));
        assert!(arguments("--demo --theme loud").is_err());
        assert!(arguments("--demo --size 80").is_err());
        assert!(arguments("--demo --frobnicate").is_err());
    }
}
