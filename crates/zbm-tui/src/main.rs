//! `zbm-tui --demo`: the prototype of the new menu on a fake machine
//! (`docs/dev/menu-design/`).

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_macros,
    reason = "the demo is a program: it reads its environment, writes the key log and prints"
)]

use std::fs::File;
use std::path::Path;
use std::process::ExitCode;

use zbm_display::grid::Grid;
use zbm_tui::menu::caps::Capabilities;
use zbm_tui::menu::demo::{DemoOptions, Session};
use zbm_tui::menu::theme::Theme;
use zbm_tui::menu::{display, terminal};
use zbm_tui::text::{Language, set_language};
use zbm_types::psf::Font;

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
  --language <name>      en (the default) or pseudo: English accented and a
                         third longer, to see where a translation would not
                         fit
  --log-keys <file>      log every key press with its time, for usability
                         sessions
  --drm                  draw on the display through DRM instead of the
                         terminal, as root on a virtual console, with keys
                         from the keyboards (ROADMAP.md #19)
  --font <file.psf>      the console font to draw with, with --drm
  --scale <n>            draw each pixel of the font n times as large
  --card <path>          the DRM card (default: the first in /dev/dri)
";

/// What the command line asks for.
#[derive(Debug, Default)]
struct Arguments {
    options: DemoOptions,
    theme: Option<Capabilities>,
    size: Option<(u16, u16)>,
    keys: Option<String>,
    language: Language,
    drm: Option<Drm>,
}

/// What `--drm` draws with.
#[derive(Debug, Default, PartialEq)]
struct Drm {
    font: String,
    scale: u32,
    card: Option<String>,
}

/// Parses the arguments after the program's name.
fn parse(args: &[String]) -> Result<Arguments, String> {
    let mut parsed = Arguments::default();
    let mut demo = false;
    let (mut drm, mut font, mut scale, mut card) = (false, None, 1, None);
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
            "--language" => {
                let name = args.next().ok_or("--language needs a name")?;
                parsed.language =
                    Language::from_name(name).ok_or(format!("unknown language {name}"))?;
            }
            "--size" => {
                let size = args.next().ok_or("--size needs <cols>x<rows>")?;
                parsed.size = Some(parse_size(size).ok_or(format!("bad size {size}"))?);
            }
            "--log-keys" => {
                parsed.keys = Some(args.next().ok_or("--log-keys needs a file")?.clone());
            }
            "--drm" => drm = true,
            "--font" => font = Some(args.next().ok_or("--font needs a file")?.clone()),
            "--scale" => {
                let n = args.next().ok_or("--scale needs a number")?;
                scale = n
                    .parse()
                    .ok()
                    .filter(|&n| n > 0)
                    .ok_or(format!("bad scale {n}"))?;
            }
            "--card" => card = Some(args.next().ok_or("--card needs a path")?.clone()),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if drm {
        let font = font.ok_or("--drm needs --font")?;
        parsed.drm = Some(Drm { font, scale, card });
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
    set_language(arguments.language);
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
    if let Some(drm) = &arguments.drm {
        // The display is drawn like the Linux console unless a theme is asked.
        let capabilities = arguments.theme.unwrap_or(Capabilities::CONSOLE);
        let session = Session::new(&arguments.options, Theme::new(capabilities));
        return run_drm(session, drm);
    }
    let session = Session::new(&arguments.options, Theme::new(capabilities));
    let size = terminal::size(arguments.size.unwrap_or((80, 24)));
    let escape_wait = zbm_tui::menu::term_keys::CONSOLE_ESCAPE_WAIT;
    match terminal::run(session, size, keys, escape_wait) {
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

/// Runs the demo on the display.
fn run_drm(session: Session, drm: &Drm) -> ExitCode {
    let font = match std::fs::read(&drm.font).map(|bytes| Font::parse(&bytes)) {
        Ok(Ok(font)) => font,
        Ok(Err(error)) => {
            eprintln!("zbm-tui: {}: {error}", drm.font);
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("zbm-tui: {}: {error}", drm.font);
            return ExitCode::FAILURE;
        }
    };
    let card = drm.card.as_deref().map(Path::new);
    let mut grid = |_| Ok(Grid::new(font.clone(), drm.scale));
    match display::run_on_display(session, card, &mut grid, &mut |_| {}) {
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
                    --theme ascii --size 52x20 --log-keys k --language pseudo";
        let parsed = arguments(line).unwrap();

        assert!(parsed.options.locked && parsed.options.foreign);
        assert!(parsed.options.failing && parsed.options.next_boot);
        assert!(parsed.options.tpm_changed);
        assert_eq!(parsed.options.timeout, None);
        assert_eq!(parsed.theme, Some(Capabilities::ASCII));
        assert_eq!(parsed.size, Some((52, 20)));
        assert_eq!(parsed.keys.as_deref(), Some("k"));
        assert_eq!(parsed.language, Language::Pseudo);
        assert!(arguments("--demo --language xx").is_err());
        assert!(arguments("--demo --language").is_err());
    }

    #[test]
    fn without_demo_or_with_bad_options_the_usage_is_shown() {
        assert_eq!(arguments("").err().as_deref(), Some(""));
        assert!(arguments("--demo --theme loud").is_err());
        assert!(arguments("--demo --size 80").is_err());
        assert!(arguments("--demo --frobnicate").is_err());
    }

    #[test]
    fn a_font_that_cannot_be_read_stops_the_demo_on_the_display() {
        let session = || Session::new(&DemoOptions::default(), Theme::new(Capabilities::CONSOLE));
        let drm = |font: &str| Drm {
            font: font.to_owned(),
            scale: 1,
            card: Some("/nonexistent/card0".to_owned()),
        };
        let manifest = env!("CARGO_MANIFEST_DIR");

        assert_eq!(
            run_drm(session(), &drm("/nonexistent.psf")),
            ExitCode::FAILURE
        );
        assert_eq!(
            run_drm(session(), &drm(&format!("{manifest}/Cargo.toml"))),
            ExitCode::FAILURE
        );
        let font = format!("{manifest}/../../legacy/zfsbootmenu/fonts/ter-v14b.psf");
        assert_eq!(run_drm(session(), &drm(&font)), ExitCode::FAILURE);
    }

    #[test]
    fn drm_needs_a_font_and_takes_a_scale_and_a_card() {
        let parsed =
            arguments("--demo --drm --font f.psf --scale 2 --card /dev/dri/card1").unwrap();

        assert_eq!(
            parsed.drm,
            Some(Drm {
                font: "f.psf".to_owned(),
                scale: 2,
                card: Some("/dev/dri/card1".to_owned()),
            })
        );
        assert_eq!(
            arguments("--demo --drm --font f.psf")
                .unwrap()
                .drm
                .unwrap()
                .scale,
            1
        );
        assert!(arguments("--demo --drm").is_err());
        assert!(arguments("--demo --drm --font f --scale 0").is_err());
        assert_eq!(arguments("--demo --font f.psf").unwrap().drm, None);
    }
}
