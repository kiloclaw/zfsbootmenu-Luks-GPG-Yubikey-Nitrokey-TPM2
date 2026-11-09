use pretty_assertions::assert_eq;
use zbm_display::keys::KeyPress;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::keymap::classic_bindings;
use crate::menu::model::{Dialog, Model, Screen};
use crate::menu::theme::Theme;
use crate::menu::update::{Message, update};

/// The page the sections are written into, and their names, as the
/// example `gen_keys` writes them.
const PAGE: &str = "docs/man/zfsbootmenu.7.rst";
const KEYS: &str = "keys";
const ALIASES: &str = "keys-aliases";

/// The text of the place called `title` in `text`: what its title heads,
/// to the next title.
fn place_of(text: &str, title: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let is_title = |line: usize| {
        lines
            .get(line + 1)
            .is_some_and(|next| !next.is_empty() && next.chars().all(|c| c == '~'))
    };
    let start = (0..lines.len())
        .find(|line| lines[*line] == title && is_title(*line))
        .unwrap_or_else(|| panic!("no place {title}"));
    let end = (start + 2..lines.len())
        .find(|line| is_title(*line))
        .unwrap_or(lines.len());
    lines[start + 2..end].join("\n") + "\n"
}

#[test]
fn every_binding_of_the_workspace_is_named_in_its_place() {
    let text = workspace();
    let every = place_of(&text, "Every View");
    for context in VIEWS.iter().chain(OTHERS) {
        let own = place_of(&text, &name(*context));
        for binding in Map::Workspace.table() {
            if !Map::Workspace.is_bound(binding, *context)
                || said(*context, binding.intent).is_none()
            {
                continue;
            }
            let named = format!("**{}**", key_name(binding.key, binding.modifiers));
            let listed = own.contains(&named) || VIEWS.contains(context) && every.contains(&named);
            assert!(listed, "{named} in {context:?}");
        }
    }
}

#[test]
fn the_views_list_what_they_add_to_every_view() {
    let text = workspace();
    let every = place_of(&text, "Every View");
    assert!(every.contains("**h**, **Esc**, **Ctrl-C**\n\n  Back.\n"));
    assert!(every.contains("**1**\n\n  The Boot tab.\n"));
    assert!(
        !every.contains("**Enter**"),
        "Enter means something else in each view"
    );
    assert!(!every.contains("**/**"), "the log has no filter");
    let main = place_of(&text, "Boot Environments");
    assert!(main.contains("**Enter**, **b**\n\n  Boot.\n"));
    assert!(main.contains("**Left**\n\n  Back.\n"));
    assert!(!main.contains("**Esc**"));
    assert!(every.contains("**j**, **Down**, **Ctrl-N**\n\n  Select the row below.\n"));
    assert!(every.contains("**PgUp**\n\n  Move a page up.\n"));
    let log = place_of(&text, "Log");
    assert!(log.contains("**j**, **Down**, **Ctrl-N**\n\n  Scroll down.\n"));
    assert!(log.contains("**k**, **Up**, **Ctrl-P**\n\n  Scroll up.\n"));
    assert!(!log.contains("**Enter**"), "nothing to choose in the log");
    let help = place_of(&text, "Help");
    assert!(help.contains("**F1**"));
    assert!(!help.contains("**?**"));
    assert!(!help.contains("**Enter**"));
}

/// The demo under the workspace at power-on, start-up's view showing.
fn starting() -> Session {
    let mut session = Session::new(&DemoOptions::default(), Theme::new(Capabilities::FULL));
    session.tick(0);
    session
}

/// The demo under the workspace, started, its countdown stopped, after
/// `keys`.
fn after(keys: &[Key]) -> Session {
    let mut session = Session::new(&DemoOptions::default(), Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    for key in keys {
        session.key(*key);
    }
    session
}

/// What a key could change that the page speaks of: the screen, the
/// dialog, the palette, the command line, the scroll and the list.
fn focus(model: &Model) -> String {
    format!(
        "{:?}",
        (
            &model.screen,
            &model.dialog,
            &model.palette,
            &model.editor,
            model.scroll,
            model.list()
        )
    )
}

/// `model` after `press`, unless it changed nothing the page speaks of and
/// asked the machine for nothing.
fn pressed(model: Model, press: KeyPress) -> Option<Model> {
    let before = focus(&model);
    let (model, actions) = update(model, Message::Key(press));
    (!actions.is_empty() || focus(&model) != before).then_some(model)
}

/// A place opened in the demo, and keys the page leaves out there.
type Case = (fn() -> Session, Context, Vec<KeyPress>);

#[test]
fn the_keys_left_out_do_nothing_where_they_are_left_out() {
    let text = workspace();
    let ctrl = |c| KeyPress::new(Key::Char(c)).with_ctrl();
    let plain = KeyPress::new;
    let cases: [Case; 6] = [
        (starting, Startup, vec![plain(Key::Up), ctrl('n')]),
        (
            || after(&[Key::Char('l')]),
            Kernels,
            vec![plain(Key::Right)],
        ),
        (
            || after(&[Key::Char('x')]),
            Choose,
            vec![plain(Key::F(1)), plain(Key::Home), plain(Key::End)],
        ),
        (
            || after(&[Key::Char(':')]),
            Context::Command,
            vec![plain(Key::F(1)), plain(Key::PageUp), plain(Key::PageDown)],
        ),
        (
            || after(&[Key::Char('/')]),
            Context::Text,
            vec![plain(Key::F(1))],
        ),
        (
            || {
                after(&[
                    Key::Char('2'),
                    Key::Down,
                    Key::Down,
                    Key::Down,
                    Key::Char('r'),
                ])
            },
            Review,
            vec![plain(Key::F(1))],
        ),
    ];
    for (open, context, presses) in cases {
        let place = place_of(&text, &name(context));
        for press in presses {
            let model = open().into_model();
            assert_eq!(model.context(), context);
            assert!(pressed(model, press).is_none(), "{press:?} in {context:?}");
            let named = format!("**{}**", key_name(press.key, Modifiers::of(&press)));
            assert!(!place.contains(&named), "{named} in {context:?}");
        }
    }
    // An alert takes only Enter and the keys that cancel.
    let mut model = after(&[]).into_model();
    model.dialog = Some(Dialog::Error {
        what: String::new(),
        why: Vec::new(),
        next: Vec::new(),
    });
    assert!(pressed(model, KeyPress::new(Key::Down)).is_none());
}

#[test]
fn the_keys_of_some_fields_only_work_there_and_the_page_says_where() {
    let reveal = KeyPress::new(Key::Char('r')).with_ctrl();
    // The unlock prompt shows what is typed once start-up is over; the
    // command line shows the commands chosen before instead.
    let locked = DemoOptions {
        locked: true,
        ..DemoOptions::default()
    };
    let mut asked = Session::new(&locked, Theme::new(Capabilities::FULL));
    asked.start();
    let Some(Dialog::Unlock(prompt)) = asked.into_model().dialog else {
        panic!("no unlock prompt");
    };
    let mut model = after(&[]).into_model();
    assert_eq!(model.screen, Screen::Main);
    model.dialog = Some(Dialog::Unlock(prompt));
    let shown = pressed(model, reveal).and_then(|model| model.dialog);
    assert!(matches!(shown, Some(Dialog::Unlock(prompt)) if prompt.revealed));
    let recent = pressed(after(&[Key::Char(':')]).into_model(), reveal);
    assert!(
        recent
            .and_then(|model| model.palette)
            .is_some_and(|p| p.recent)
    );
    // A list's filter moves a page; the palette does not.
    let filter = after(&[Key::Char('2'), Key::Char('/')]).into_model();
    assert!(pressed(filter, KeyPress::new(Key::PageDown)).is_some());
    let palette = after(&[Key::Char(':')]).into_model();
    assert!(pressed(palette, KeyPress::new(Key::PageDown)).is_none());
    let text = workspace();
    let fields = place_of(&text, "Fields That Take Text");
    assert!(fields.contains("**PgDn**\n\n  Move a page down, in a list's filter.\n"));
    assert!(fields.contains(
        "**Ctrl-R**\n\n  Show what is typed, in the unlock prompt, once start-up is over.\n"
    ));
    let command_line = place_of(&text, "The Command Line");
    assert!(command_line.contains("**Ctrl-R**\n\n  Show only the commands chosen before"));
    assert!(command_line.contains("**Tab**\n\n  Complete the query to the selected command.\n"));
    assert!(command_line.contains("**Ctrl-W**\n\n  Delete the last word of the query.\n"));
    assert!(command_line.contains("**Backspace**\n\n  Erase the last character.\n"));
    assert!(!command_line.contains("**Ctrl-K**"));
    assert!(!fields.contains("**Tab**"));
}

#[test]
fn fields_erase_and_clear_as_input_reads_them() {
    let text = workspace();
    for title in ["Fields That Take Text", "Review the Change"] {
        let field = place_of(&text, title);
        assert!(field.contains("**Backspace**\n\n  Erase the last character.\n"));
        assert!(field.contains("**Ctrl-U**\n\n  Clear the field.\n"));
    }
    assert!(!place_of(&text, "Kernels").contains("**Ctrl-U**"));
}

#[test]
fn every_classic_letter_has_an_alias_entry() {
    let text = aliases();
    for (letter, _) in CLASSIC {
        let upper = letter.to_ascii_uppercase();
        let named = text.contains(&format!("**MOD+{upper}**"))
            || text.contains(&format!("**Alt+{upper}**"));
        assert!(named, "{letter}");
    }
    assert!(text.contains(
        "**MOD+D**\n\n  Boot Environments: default; Snapshots: diff; Kernels: default.\n"
    ));
}

#[test]
fn c_l_n_and_p_are_aliases_with_alt_only() {
    let alt_only: Vec<char> = CLASSIC
        .iter()
        .filter(|(letter, meanings)| {
            meanings
                .iter()
                .any(|(view, _)| taken_with_ctrl(*letter, *view).is_some())
        })
        .map(|(letter, _)| *letter)
        .collect();
    assert_eq!(alt_only, ['p', 'l', 'c', 'n']);
    for (letter, meanings) in CLASSIC {
        for (view, _) in *meanings {
            let upper = letter.to_ascii_uppercase();
            let label = if alt_only.contains(letter) {
                format!("Alt+{upper}")
            } else {
                format!("MOD+{upper}")
            };
            let labels = classic_bindings(*view, &|_| true);
            assert!(labels.iter().any(|(key, _)| *key == label), "{label}");
        }
    }
    assert!(
        aliases()
            .contains("**Alt+P**\n\n  Boot Environments: pools. Alt only: Ctrl-P is Select.\n")
    );
}

#[test]
fn only_ctrl_j_is_sent_as_another_key_and_its_entry_says_so() {
    let sent: Vec<(char, KeyPress)> = CLASSIC
        .iter()
        .filter_map(|(letter, _)| Some((*letter, sent_as(*letter)?)))
        .collect();
    assert_eq!(sent, [('j', KeyPress::new(Key::Enter))]);
    assert!(aliases().contains(
        "**MOD+J**\n\n  Boot Environments: chroot; Snapshots: chroot. \
         On a serial line or over SSH, Ctrl-J arrives as Enter: use Alt.\n"
    ));
    assert_eq!(sent_as('?'), None);
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "the test reads the manual page from the repository"
)]
fn the_manual_page_is_up_to_date() {
    let text = std::fs::read_to_string(zbm_testkit::repo_root().join(PAGE)).unwrap();
    let updated = zbm_config::docs::splice(PAGE, &text, KEYS, &workspace())
        .and_then(|text| zbm_config::docs::splice(PAGE, &text, ALIASES, &aliases()))
        .unwrap();
    assert!(
        updated == text,
        "{PAGE} is out of date: run `cargo xtask gen-docs`"
    );
}
