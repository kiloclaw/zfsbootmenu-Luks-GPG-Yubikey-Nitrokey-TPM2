//! The wireframes of `docs/dev/menu-design/wireframes/`: every view of the
//! prototype at 120×40, 80×25 and 52×20, and the main dialogs in ASCII, as
//! snapshot tests. Change a view, then review and accept the new
//! wireframes with `cargo insta review` or `cargo xtask bless`.

use zbm_display::keys::{Key, KeyPress};
use zbm_display::psf::{Font, repertoire};

use super::tests::screen;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine, Session};
use crate::menu::theme::Theme;
use crate::menu::update::{Message, update};

/// The sizes of the wireframes.
const SIZES: [(u16, u16); 3] = [(120, 40), (80, 25), (52, 20)];

/// A key press of a scenario.
#[derive(Clone, Copy)]
enum Step {
    Char(char),
    Ctrl(char),
    /// Time passes until start-up is over or a dialog opens.
    Start,
    /// A moment passes.
    Tick,
    Code(Key),
    Text(&'static str),
    /// A key whose actions the machine has not answered yet.
    Unanswered(char),
}

/// A view and how to reach it from power-on.
struct Wireframe {
    name: &'static str,
    options: DemoOptions,
    ticks: usize,
    keys: &'static [Step],
}

mod classic;
mod workspace;

use self::classic::{first_wireframes, naming_wireframes, snapshot_wireframes, unlock_wireframes};
use self::workspace::workspace_wireframes;

/// Every wireframe.
fn wireframes() -> Vec<Wireframe> {
    let mut all = first_wireframes();
    all.extend(snapshot_wireframes());
    all.extend(unlock_wireframes());
    all.extend(naming_wireframes());
    all.extend(workspace_wireframes());
    all
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
            Step::Tick => session.tick(0),
            Step::Code(key) => session.key(key),
            Step::Text(text) => session.type_text(text),
            Step::Unanswered(c) => {
                let message = Message::Key(KeyPress::new(Key::Char(c)));
                let (model, _) = update(session.into_model(), message);
                session = Session::with_model(FakeMachine::new(&wireframe.options), model);
            }
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
