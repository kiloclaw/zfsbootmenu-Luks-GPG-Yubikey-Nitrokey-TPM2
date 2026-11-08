//! Each layout's keys as the menu counts them, one chart per layout in
//! `docs/dev/menu-design/layouts/`: what each key of the main block types,
//! plain and with Shift, the character the menu looks up for it (rule 3),
//! and what it does on the Boot, Snapshots and Pools tabs of the workspace.
//! A reviewer reads there the second keys rule 3 makes on Latin layouts,
//! such as Dvorak's `.`, which is the US `e` in its place.

use std::fmt::Write as _;

use zbm_display::keys::{Key, KeyPress};
use zbm_display::layout::MAIN_BLOCK;

use super::super::{Context, Input, Map, Modifiers, input_on};
use super::{LAYOUTS, Stroke, all_positions, is_bound_anywhere, lower, positions};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, data};
use crate::menu::model::{Model, TypedOn};
use crate::menu::theme::Theme;
use crate::menu::update::{Message, update};

/// The tabs of the chart: the views of the Boot, Snapshots and Pools tabs.
const TABS: [Context; 3] = [Context::Main, Context::Snapshots, Context::Pools];

/// The menu of the demo machine, whose image offers the six layouts, its
/// keys typed on the layout `name`.
fn typed_on(name: &str) -> Model {
    let mut snapshot = data::machine(&DemoOptions::default());
    snapshot.keyboard.layouts = LAYOUTS.map(str::to_owned).to_vec();
    snapshot.keyboard.positions = all_positions();
    let model = Model::new(snapshot, Theme::new(Capabilities::FULL));
    let on = TypedOn::Layout(name.to_owned());
    update(model, Message::TypedOn(Some(on))).0
}

/// The character the menu looks `press` up as, typed on the layout
/// `name`: its own when the workspace binds it or the layout is the US
/// one, else the character of its key at its level on the US layout the
/// image has; none when no key of the main block types it. Its key is the
/// first that types it at its level, else the first that types it.
fn counted_as(name: &str, press: &KeyPress) -> Option<char> {
    let Key::Char(c) = press.key else {
        return None;
    };
    if name == "us" || is_bound_anywhere(Map::Workspace, c, Modifiers::of(press)) {
        return Some(c);
    }
    let layout = positions(name);
    let at_level = |c| {
        MAIN_BLOCK
            .iter()
            .copied()
            .find(|&code| layout.character_of(code, press.shift) == Some(c))
    };
    let code = [lower(c), c]
        .into_iter()
        .find_map(|c| at_level(c).or_else(|| layout.key_of(c).map(|(code, _)| code)))?;
    positions("us").character_of(code, press.shift)
}

/// What `input` asks for, as the chart writes it.
fn written(input: Option<Input>) -> String {
    match input {
        Some(Input::Intent(intent)) => format!("{intent:?}"),
        Some(other) => format!("{other:?}"),
        None => "·".to_owned(),
    }
}

/// The chart of the layout `name`: a row for each key of the main block,
/// plain and with Shift.
fn chart(name: &str) -> String {
    let model = typed_on(name);
    let mut text = format!(
        "{:<8} {:<10} {:<8} {:<16} {:<16} {}\n",
        "key", "typed", "counted", "Boot tab", "Snapshots tab", "Pools tab"
    );
    for &code in MAIN_BLOCK {
        for shift in [false, true] {
            let key = format!("{code}{}", if shift { " Shift" } else { "" });
            let presses = Stroke::new(code, shift).on(name);
            let Some(press) = presses.last() else {
                let accent = positions(name).character_of(code, shift);
                let typed = accent.map_or_else(String::new, |c| format!("{c} dead"));
                let _ = writeln!(text, "{key:<8} {typed}");
                continue;
            };
            let typed = match press.key {
                Key::Char(c) => c.to_string(),
                other => format!("{other:?}"),
            };
            let counted = counted_as(name, press).map_or_else(|| "·".to_owned(), String::from);
            let intents = TABS.map(|context| {
                let typing = model.typing(press);
                written(input_on(Map::Workspace, context, *press, typing))
            });
            let [boot, snapshots, pools] = intents;
            let _ = writeln!(
                text,
                "{key:<8} {typed:<10} {counted:<8} {boot:<16} {snapshots:<16} {pools}"
            );
        }
    }
    text
}

fn settings() -> insta::Settings {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("../../../../../../docs/dev/menu-design/layouts");
    settings.set_prepend_module_to_snapshot(false);
    settings.set_omit_expression(true);
    settings.set_description(
        "Each key of the main block, by Linux key code, plain and with Shift: \
         what the layout types, the character the menu counts it as (rule 3 \
         of docs/dev/menu-design/keymap.md), and what it does in the \
         workspace's Boot, Snapshots and Pools tabs; · for nothing.",
    );
    settings
}

#[test]
fn each_layout_s_keys_as_the_menu_counts_them() {
    settings().bind(|| {
        for name in LAYOUTS {
            let file = name.replace('(', "-").replace(')', "");
            insta::assert_snapshot!(file, chart(name));
        }
    });
}
