//! The wireframes whose keys are actions, typed on the five layouts of ADR
//! 0007's check besides the US one: each character by its place on a US
//! keyboard, or by its label where the layout's own character in that
//! place means something else, with the layout said before each key, as
//! the console on the display says it. Each shows at 120×40 what the US
//! keyboard's keys show: a layout changes keys, not drawing.

use zbm_display::keys::{Key, KeyPress};

use super::super::tests::screen;
use super::{Step, Wireframe, play, wireframes};
use crate::menu::caps::Capabilities;
use crate::menu::keymap::Map;
use crate::menu::keymap::layouts_tests::{LAYOUTS, SixLayouts, typed};
use crate::menu::model::TypedOn;
use crate::menu::session::Session;
use crate::menu::theme::Theme;

/// The wireframes whose keys are actions: no text typed, no time passing
/// between keys.
const ACTIONS: [&str; 14] = [
    "workspace-kernels",
    "workspace-snapshots",
    "workspace-help",
    "log-history",
    "workspace-power",
    "workspace-recovery",
    "workspace-notification",
    "workspace-help-snapshots",
    "kernels",
    "recovery",
    "log",
    "help",
    "power",
    "snapshots",
];

/// The session after `wireframe`'s scenario, its keys typed on the layout
/// `name` of a machine whose image offers the six layouts.
fn play_on(wireframe: &Wireframe, name: &str) -> Session<SixLayouts> {
    let map = if wireframe.options.classic {
        Map::Classic
    } else {
        Map::default()
    };
    let machine = SixLayouts::new(&wireframe.options);
    let mut session = Session::on(machine, Theme::new(Capabilities::FULL), map);
    for _ in 0..wireframe.ticks {
        let model = session.model();
        if model.started || model.dialog.is_some() {
            break;
        }
        session.tick(0);
    }
    let on = || Some(TypedOn::Layout(name.to_owned()));
    for step in wireframe.keys {
        match *step {
            Step::Char(c) => {
                for press in typed(name, map, KeyPress::new(Key::Char(c))) {
                    session.press_typed_on(press, on());
                }
            }
            Step::Code(key) => session.press_typed_on(KeyPress::new(key), on()),
            _ => panic!("{} has a step that is not a key", wireframe.name),
        }
    }
    session
}

#[test]
fn the_wireframes_typed_on_five_layouts_show_what_us_shows() {
    // Keys go by their place, or by label where the place's character is
    // another action's.
    let char = |name, c| typed(name, Map::Workspace, KeyPress::new(Key::Char(c)))[0].key;
    assert_eq!(char("ru", 'l'), Key::Char('д'));
    assert_eq!(char("fr", '2'), Key::Char('é'));
    assert_eq!(char("us(dvorak)", 'l'), Key::Char('l'));
    let all = wireframes();
    for name in ACTIONS {
        let wireframe = all
            .iter()
            .find(|wireframe| wireframe.name == name)
            .unwrap_or_else(|| panic!("no wireframe {name}"));
        let us = screen(play(wireframe, Capabilities::FULL).model(), 120, 40);
        for layout in LAYOUTS.into_iter().filter(|layout| *layout != "us") {
            let shown = screen(play_on(wireframe, layout).model(), 120, 40);
            assert_eq!(shown, us, "{name} typed on {layout}");
        }
    }
}
