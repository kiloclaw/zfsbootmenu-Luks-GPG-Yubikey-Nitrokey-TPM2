//! The keyboard layouts while the console runs: the one `zbm console
//! layout` asks for, and the image's next one (Super+Space).

use zbm_display::input::Keyboards;
use zbm_platform::RealFiles;
use zbm_state::State;

use super::{LoadLayout, Places};
use crate::host::{Change, Switch};

/// What the host runs when a layout is asked for, with the image's layouts
/// `names` and `load` to load them.
pub(super) fn switcher<'s>(
    places: &'s Places,
    names: Vec<String>,
    load: LoadLayout<'s>,
) -> Switch<'s> {
    Box::new(move |keyboards: &mut Keyboards, change| match change {
        Change::Requested => switch_layout(places, &mut *load, keyboards),
        Change::Next => next_layout(places, &names, &mut *load, keyboards),
    })
}

/// Takes the request for a keyboard layout in `places`' state directory,
/// and gives `keyboards` the layout when `load` has it; the state records
/// the layout they type with before the request goes, which tells the
/// process that asked.
pub fn switch_layout(places: &Places, load: LoadLayout<'_>, keyboards: &mut Keyboards) {
    let state = State::new(&RealFiles, &places.base);
    if let Some(name) = state.console_layout_request() {
        match load(&name) {
            Ok(layout) => {
                keyboards.set_layout(Some(layout));
                places.log(6, &format!("keyboard {name}"));
            }
            Err(error) => {
                let kept = keyboards.layout();
                places.log(4, &format!("{error}; keeping keyboard {kept}"));
            }
        }
    }
    let _ = state.set_console_layout(keyboards.layout());
    state.clear_console_layout_request();
}

/// Gives `keyboards` the layout after the one they type with among `names`,
/// the image's layouts, when `load` has it, and records it in `places`'
/// state directory.
pub fn next_layout(
    places: &Places,
    names: &[String],
    load: LoadLayout<'_>,
    keyboards: &mut Keyboards,
) {
    let current = names.iter().position(|name| name == keyboards.layout());
    let next = current.map_or(0, |index| (index + 1) % names.len().max(1));
    let Some(name) = names.get(next) else {
        return;
    };
    match load(name) {
        Ok(layout) => {
            keyboards.set_layout(Some(layout));
            places.log(6, &format!("keyboard {name}"));
            let _ = State::new(&RealFiles, &places.base).set_console_layout(name);
        }
        Err(error) => places.log(4, &error),
    }
}
