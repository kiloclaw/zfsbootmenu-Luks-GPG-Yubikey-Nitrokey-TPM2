//! The wireframes of `docs/dev/menu-design/wireframes/`: every view of the
//! prototype at 120×40, 80×25 and 52×20, and the main dialogs in ASCII, as
//! snapshot tests. Change a view, then review and accept the new
//! wireframes with `cargo insta review` or `cargo xtask bless`.

use zbm_display::keys::{Key, KeyPress};
use zbm_display::psf::{Font, repertoire};

use super::tests::screen;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::theme::Theme;

/// The sizes of the wireframes.
const SIZES: [(u16, u16); 3] = [(120, 40), (80, 25), (52, 20)];

/// A key press of a scenario.
#[derive(Clone, Copy)]
enum Step {
    Char(char),
    Ctrl(char),
    /// Time passes until start-up is over or a dialog opens.
    Start,
    Code(Key),
    Text(&'static str),
}

/// A view and how to reach it from power-on.
struct Wireframe {
    name: &'static str,
    options: DemoOptions,
    ticks: usize,
    keys: &'static [Step],
}

/// The demo after a firmware update: the TPM keeps a volume's key.
fn tpm_changed() -> DemoOptions {
    DemoOptions {
        tpm_changed: true,
        ..DemoOptions::default()
    }
}

/// The demo with the default used up its boot tries, or a one-time boot.
fn health(failing: bool, next_boot: bool) -> DemoOptions {
    DemoOptions {
        failing,
        next_boot,
        ..DemoOptions::default()
    }
}

fn options(locked: bool, foreign: bool) -> DemoOptions {
    DemoOptions {
        locked,
        foreign,
        ..DemoOptions::default()
    }
}

/// Every wireframe.
fn wireframes() -> Vec<Wireframe> {
    let mut all = first_wireframes();
    all.extend(snapshot_wireframes());
    all.extend(unlock_wireframes());
    all.extend(naming_wireframes());
    all.extend(workspace_wireframes());
    all
}

/// The demo in the workspace (ADR 0007).
fn workspace() -> DemoOptions {
    DemoOptions {
        workspace: true,
        ..DemoOptions::default()
    }
}

/// The wireframes of the workspace: the Boot tab, and the kernels column
/// it goes into.
fn workspace_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code};
    let frame = |name, keys| Wireframe {
        name,
        options: workspace(),
        ticks: 50,
        keys,
    };
    vec![
        frame("workspace", &[]),
        frame("workspace-kernels", &[Code(Key::Down), Char('l')]),
        frame("workspace-pools", &[Char('3')]),
    ]
}

/// The wireframes of naming a clone and a snapshot.
fn naming_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Text};
    let frame = |name, keys| Wireframe {
        name,
        options: options(false, false),
        ticks: 50,
        keys,
    };
    vec![
        frame(
            "clone-name",
            &[Char('s'), Char('o'), Code(Key::Home), Char('c'), Text("-2")],
        ),
        frame("snapshot-name", &[Char('n')]),
    ]
}

/// The wireframes of unlocking and of the packages since a snapshot.
fn unlock_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Ctrl, Start, Text};
    let frame = |name, options, keys| Wireframe {
        name,
        options,
        ticks: 50,
        keys,
    };
    vec![
        frame("unlock-tpm-changed", tpm_changed(), &[]),
        frame(
            "unlock-wrong",
            options(true, false),
            &[Code(Key::Down), Text("dmeo"), Code(Key::Enter)],
        ),
        frame(
            "unlock-layout",
            options(true, false),
            &[Code(Key::Down), Ctrl('k'), Text("Y[rich"), Ctrl('r')],
        ),
        frame(
            "reseal-confirm",
            tpm_changed(),
            &[Text("demo"), Code(Key::Enter)],
        ),
        frame(
            "pools-tpm-changed",
            tpm_changed(),
            &[
                Text("demo"),
                Code(Key::Enter),
                Code(Key::Escape),
                Start,
                Char('p'),
            ],
        ),
        frame(
            "diff-packages",
            options(false, false),
            &[
                Char('s'),
                Char('o'),
                Code(Key::Home),
                Char('f'),
                Code(Key::Right),
            ],
        ),
    ]
}

/// The wireframes of the views, and of boot health.
fn first_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Text};
    let plain = || options(false, false);
    let frame = |name, options, ticks, keys| Wireframe {
        name,
        options,
        ticks,
        keys,
    };
    vec![
        frame("startup", plain(), 5, &[]),
        frame("main", plain(), 50, &[]),
        frame("main-filter", plain(), 50, &[Char('/'), Text("ta")]),
        frame("kernels", plain(), 50, &[Char('k')]),
        frame(
            "command-line",
            plain(),
            50,
            &[Char('e'), Text(" systemd.unit=rescue.target")],
        ),
        frame("pools", plain(), 50, &[Char('p')]),
        frame("unlock-pin", options(true, false), 50, &[Text("12")]),
        frame("import-problem", options(false, true), 50, &[]),
        frame(
            "import-failed",
            options(false, true),
            50,
            &[Code(Key::Enter)],
        ),
        frame("palette", plain(), 50, &[Char(':'), Text("2026-09")]),
        frame("recovery", plain(), 50, &[Char('!')]),
        frame("log", plain(), 50, &[Char('l')]),
        frame("help", plain(), 50, &[Char('?')]),
        frame("power", plain(), 50, &[Char('x')]),
        frame("notification", plain(), 50, &[Code(Key::Down), Char('d')]),
        frame("main-fallback", health(true, false), 50, &[]),
        frame("main-next-boot", health(false, true), 50, &[]),
        frame(
            "may-not-boot",
            plain(),
            50,
            &[Code(Key::Down), Code(Key::Down), Code(Key::Enter)],
        ),
    ]
}

/// The wireframes of the snapshots view, sorted newest first.
fn snapshot_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Text};
    let frame = |name, keys| Wireframe {
        name,
        options: options(false, false),
        ticks: 50,
        keys,
    };
    vec![
        frame("snapshots", &[Char('s'), Char('o'), Code(Key::Home)]),
        frame(
            "snapshot-actions",
            &[Char('s'), Char('o'), Code(Key::Home), Code(Key::Enter)],
        ),
        frame(
            "rollback-confirm",
            &[Char('s'), Char('o'), Code(Key::Home), Char('r'), Text("vo")],
        ),
        frame("diff", &[Char('s'), Char('o'), Code(Key::Home), Char('f')]),
        frame(
            "diff-marked",
            &[
                Char('s'),
                Char('o'),
                Code(Key::Home),
                Char('f'),
                Char(' '),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char(' '),
            ],
        ),
        frame(
            "restore-confirm",
            &[
                Char('s'),
                Char('o'),
                Code(Key::Home),
                Char('f'),
                Char(' '),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char(' '),
                Code(Key::Enter),
                Text("vo"),
            ],
        ),
    ]
}

/// The session after a wireframe's scenario.
fn play(wireframe: &Wireframe, capabilities: Capabilities) -> Session {
    let mut session = Session::new(&wireframe.options, Theme::new(capabilities));
    for _ in 0..wireframe.ticks {
        let model = session.model();
        if model.started || model.dialog.is_some() {
            break;
        }
        session.tick(0);
    }
    for key in wireframe.keys {
        match *key {
            Step::Char(c) => session.key(Key::Char(c)),
            Step::Ctrl(c) => session.press(KeyPress::new(Key::Char(c)).with_ctrl()),
            Step::Start => session.start(),
            Step::Code(key) => session.key(key),
            Step::Text(text) => session.type_text(text),
        }
    }
    session
}

fn settings() -> insta::Settings {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("../../../../../docs/dev/menu-design/wireframes");
    settings.set_prepend_module_to_snapshot(false);
    settings.set_omit_expression(true);
    settings
}

#[test]
fn every_view_at_three_sizes() {
    settings().bind(|| {
        for wireframe in wireframes() {
            let session = play(&wireframe, Capabilities::FULL);
            for (width, height) in SIZES {
                let name = format!("{}-{width}x{height}", wireframe.name);
                insta::assert_snapshot!(name, screen(session.model(), width, height));
            }
        }
    });
}

/// The workspace over a serial line: 80×24, in colour and in ASCII.
#[test]
fn the_workspace_on_a_serial_console() {
    settings().bind(|| {
        for wireframe in workspace_wireframes() {
            for (capabilities, suffix) in
                [(Capabilities::FULL, ""), (Capabilities::ASCII, "-ascii")]
            {
                let session = play(&wireframe, capabilities);
                let text = screen(session.model(), 80, 24);
                if suffix == "-ascii" {
                    assert!(text.is_ascii(), "{}: {text}", wireframe.name);
                }
                insta::assert_snapshot!(format!("{}-80x24{suffix}", wireframe.name), text);
            }
        }
    });
}

#[test]
fn the_main_views_and_dialogs_in_ascii() {
    let names = [
        "main",
        "snapshots",
        "unlock-pin",
        "rollback-confirm",
        "import-problem",
    ];
    settings().bind(|| {
        for wireframe in wireframes().into_iter().filter(|w| names.contains(&w.name)) {
            let session = play(&wireframe, Capabilities::ASCII);
            let text = screen(session.model(), 80, 25);
            assert!(text.is_ascii(), "{}: {text}", wireframe.name);
            insta::assert_snapshot!(format!("{}-80x25-ascii", wireframe.name), text);
        }
    });
}

/// Every view at 52×20, the smallest size, in the pseudo-language: English
/// accented and a third longer, as a translation may be. Text that is not
/// from the catalogue shows unaccented, and text that does not fit shows
/// cut.
#[test]
fn every_view_in_the_pseudo_language() {
    let old = crate::text::set_language(crate::text::Language::Pseudo);
    settings().bind(|| {
        for wireframe in wireframes() {
            let session = play(&wireframe, Capabilities::FULL);
            let name = format!("{}-52x20-pseudo", wireframe.name);
            insta::assert_snapshot!(name, screen(session.model(), 52, 20));
        }
    });
    crate::text::set_language(old);
}

/// The characters every console font, Terminus and Spleen, can draw as the
/// image installs it with the Rust runtime: a 256-glyph subset.
#[allow(
    clippy::disallowed_methods,
    reason = "the test reads the image's fonts from the repository"
)]
fn console_characters() -> std::collections::BTreeSet<char> {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../legacy/zfsbootmenu/fonts");
    let mut common: Option<std::collections::BTreeSet<char>> = None;
    let paths = [dir.clone(), dir.join("spleen")]
        .into_iter()
        .flat_map(|family| {
            std::fs::read_dir(family)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "psf"))
                .collect::<Vec<_>>()
        });
    for path in paths {
        let font = Font::parse(&std::fs::read(path).unwrap()).unwrap();
        let chars = font.subset(&repertoire()).font.chars().collect();
        common = Some(match common {
            Some(seen) => seen.intersection(&chars).copied().collect(),
            None => chars,
        });
    }
    common.unwrap()
}

#[test]
fn every_character_drawn_is_in_the_console_fonts() {
    let fonts = console_characters();
    let theme = Theme::new(Capabilities::FULL);
    let mut missing = std::collections::BTreeSet::new();
    for glyph in crate::menu::theme::Glyph::ALL {
        missing.extend(theme.glyph(glyph).chars().filter(|c| !fonts.contains(c)));
    }
    for language in [
        crate::text::Language::English,
        crate::text::Language::Pseudo,
    ] {
        let old = crate::text::set_language(language);
        for wireframe in wireframes() {
            let session = play(&wireframe, Capabilities::FULL);
            for (width, height) in SIZES {
                let text = screen(session.model(), width, height);
                missing.extend(text.chars().filter(|c| *c != '\n' && !fonts.contains(c)));
            }
        }
        crate::text::set_language(old);
    }

    assert!(fonts.len() > 200, "{} characters", fonts.len());
    assert_eq!(missing, std::collections::BTreeSet::new());
}
