//! The six layouts of ADR 0007's check of decision 18: `us`, `de`, `fr`,
//! `gb`, `ru` and `us(dvorak)`, compiled from the XKB data and the compose
//! rules of the machine that runs the tests (`xkb-data` and `libx11-data`
//! on Debian and Ubuntu), as the image build compiles them; no keymap is
//! kept in the repository. Keys are typed by their code through the
//! console's decoder: by their place on a US keyboard, or by their label,
//! the key of the layout that types the character. The tests type every
//! binding on them ([`matrix`]) and chart each layout's keys ([`charts`]).

#![allow(
    clippy::disallowed_methods,
    reason = "the compose rules are the machine's own file, as the image build reads it"
)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use zbm_display::keys::{Decoder, Key, KeyPress};
use zbm_display::layout::compile::{self, COMPOSE, XKB};
use zbm_display::layout::{Layout, Positions};

use super::{CLASSIC, Context, Map, Modifiers};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine};
use crate::menu::machine::Machine;
use crate::menu::model::{Screen, TypedOn};
use crate::menu::session::Session;
use crate::menu::theme::Theme;
use crate::menu::world::{Action, Event, Snapshot};

mod charts;
mod matrix;

/// The layouts of the check, US first. XKB has no `uk`: the British layout
/// is `gb`.
pub(crate) const LAYOUTS: [&str; 6] = ["us", "de", "fr", "gb", "ru", "us(dvorak)"];

/// The space bar, which no layout's positions list.
const SPACE: u16 = 57;

/// The modifiers and Caps Lock, by key code.
const SHIFT: u16 = 42;
const CTRL: u16 = 29;
const ALT: u16 = 56;
const CAPS_LOCK: u16 = 58;

/// A key of the main block, or the space bar, pressed with modifiers, and
/// with Caps Lock on or off.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each modifier is held or not, and Caps Lock on or not"
)]
struct Stroke {
    /// The key, by Linux key code.
    code: u16,
    shift: bool,
    ctrl: bool,
    alt: bool,
    caps_lock: bool,
}

impl Stroke {
    /// Key `code` alone, at the level `shift` says.
    fn new(code: u16, shift: bool) -> Stroke {
        Stroke {
            code,
            shift,
            ctrl: false,
            alt: false,
            caps_lock: false,
        }
    }

    /// The same stroke with Shift held.
    fn with_shift(self) -> Stroke {
        Stroke {
            shift: true,
            ..self
        }
    }

    /// The same stroke with `Ctrl` and `Alt` held as in `press`.
    fn held_as(self, press: &KeyPress) -> Stroke {
        Stroke {
            ctrl: press.ctrl,
            alt: press.alt,
            ..self
        }
    }

    /// The same stroke with Caps Lock on.
    fn with_caps_lock(self) -> Stroke {
        Stroke {
            caps_lock: true,
            ..self
        }
    }

    /// What the console's decoder gives for the stroke on the layout
    /// `name`: nothing for a dead key, whose accent waits for the next key.
    fn on(self, name: &str) -> Vec<KeyPress> {
        let compiled = compiled(name);
        let mut decoder = compiled.decoder.lock().unwrap();
        decoded(&mut decoder, self)
    }
}

/// The US key of `c` (the space bar for a space), at its level: the key
/// that types `c` on a US keyboard, typed by its place on every layout.
fn by_position(c: char) -> Stroke {
    if c == ' ' {
        return Stroke::new(SPACE, false);
    }
    let (code, shift) = Positions::us()
        .key_of(c)
        .unwrap_or_else(|| panic!("no US key types {c:?}"));
    Stroke::new(code, shift)
}

/// The key of the layout `name` labelled `c`, at its level: the key that
/// types `c` there, if one does.
fn by_label(name: &str, c: char) -> Option<Stroke> {
    if c == ' ' {
        return Some(Stroke::new(SPACE, false));
    }
    let (code, shift) = positions(name).key_of(c)?;
    Some(Stroke::new(code, shift))
}

/// The presses that do on the layout `name` under `map` what `press`, a
/// character alone or with `Ctrl` or `Alt`, does on a US keyboard: its US
/// key, unless the layout's character there is another the map binds,
/// then the key labelled with it.
pub(crate) fn typed(name: &str, map: Map, press: KeyPress) -> Vec<KeyPress> {
    let Key::Char(c) = press.key else {
        panic!("{press:?} is not a character");
    };
    let place = by_position(c).held_as(&press).on(name);
    match place.last().map(|typed| typed.key) {
        Some(Key::Char(own))
            if lower(own) != c && is_bound_anywhere(map, own, Modifiers::of(&press)) =>
        {
            let label = by_label(name, c).unwrap_or_else(|| panic!("no key types {c} on {name}"));
            label.held_as(&press).on(name)
        }
        _ => place,
    }
}

/// The characters by key position of the layout `name`.
pub(crate) fn positions(name: &str) -> &'static Positions {
    &compiled(name).positions
}

/// The characters by key position of the six layouts, as an image that
/// offers them has them.
pub(crate) fn all_positions() -> BTreeMap<String, Positions> {
    LAYOUTS
        .iter()
        .map(|name| ((*name).to_owned(), positions(name).clone()))
        .collect()
}

/// A layout compiled: its positions, and the console's decoder on it.
struct Compiled {
    positions: Positions,
    decoder: Mutex<Decoder>,
}

/// The layout `name`, compiled the first time a test asks for it.
fn compiled(name: &str) -> &'static Compiled {
    static COMPILED: [OnceLock<Compiled>; 6] = [const { OnceLock::new() }; 6];
    let index = LAYOUTS
        .iter()
        .position(|layout| *layout == name)
        .unwrap_or_else(|| panic!("{name} is not one of the six layouts"));
    COMPILED[index].get_or_init(|| compile_layout(name))
}

/// The rules of dead keys, from the machine's compose file, compiled once.
fn dead_keys() -> &'static str {
    static RULES: OnceLock<String> = OnceLock::new();
    RULES.get_or_init(|| {
        let compose =
            std::fs::read(COMPOSE).expect("the compose rules of en_US.UTF-8 (libx11-data)");
        compile::dead_keys(&compose).expect("compose rules that compile")
    })
}

/// The layout `name` from the machine's XKB data, with its dead keys.
fn compile_layout(name: &str) -> Compiled {
    let keymap = compile::keymap(Path::new(XKB), name).expect("the XKB data (xkb-data)");
    let layout = Layout::from_keymap(name, keymap.as_bytes())
        .and_then(|layout| layout.with_dead_keys(dead_keys().as_bytes()))
        .unwrap_or_else(|error| panic!("{name}: {error}"));
    let positions = layout.positions();
    let mut decoder = Decoder::new();
    decoder.set_layout(Some(layout));
    Compiled {
        positions,
        decoder: Mutex::new(decoder),
    }
}

/// What `stroke` types through `decoder`, which is as it was after.
fn decoded(decoder: &mut Decoder, stroke: Stroke) -> Vec<KeyPress> {
    let held: Vec<u16> = [
        (stroke.shift, SHIFT),
        (stroke.ctrl, CTRL),
        (stroke.alt, ALT),
    ]
    .into_iter()
    .filter_map(|(on, code)| on.then_some(code))
    .collect();
    let mut presses = Vec::new();
    let mut ignored = Vec::new();
    let caps_lock = |decoder: &mut Decoder, ignored: &mut Vec<KeyPress>| {
        if stroke.caps_lock {
            decoder.feed_into(CAPS_LOCK, 1, ignored);
            decoder.feed_into(CAPS_LOCK, 0, ignored);
        }
    };
    caps_lock(decoder, &mut ignored);
    for &code in &held {
        decoder.feed_into(code, 1, &mut presses);
    }
    decoder.feed_into(stroke.code, 1, &mut presses);
    decoder.feed_into(stroke.code, 0, &mut presses);
    for &code in &held {
        decoder.feed_into(code, 0, &mut presses);
    }
    caps_lock(decoder, &mut ignored);
    // A dead key's accent waits for the next key: the space bar ends it.
    if decoder.pending_accent().is_some() {
        decoder.feed_into(SPACE, 1, &mut ignored);
        decoder.feed_into(SPACE, 0, &mut ignored);
    }
    presses
}

/// Whether `map` binds `c`, in lower case, in some view with the modifiers
/// `held`: then `c` means itself wherever it is typed, and only there (rule
/// 3, maintainer decision 2).
fn is_bound_anywhere(map: Map, c: char, held: Modifiers) -> bool {
    let c = lower(c);
    let classic = held.has_any(Modifiers::CTRL | Modifiers::ALT)
        && CLASSIC.iter().any(|(letter, _)| *letter == c);
    classic
        || map
            .table()
            .into_iter()
            .any(|binding| binding.key == Key::Char(c) && binding.modifiers == held)
}

/// `c` in lower case, when that is one character.
fn lower(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(lower), None) => lower,
        _ => c,
    }
}

/// The demo machine, whose image offers the six layouts: what it tells
/// the menu of its keyboard says so, whenever it tells it.
#[derive(Clone, Debug)]
pub(crate) struct SixLayouts {
    machine: FakeMachine,
    snapshot: Snapshot,
}

impl SixLayouts {
    /// The demo machine `options` describe, at power-on.
    pub(crate) fn new(options: &DemoOptions) -> SixLayouts {
        let machine = FakeMachine::new(options);
        let snapshot = with_six_layouts(machine.snapshot().clone());
        SixLayouts { machine, snapshot }
    }

    /// `events`, their snapshots with the six layouts; the last snapshot
    /// told is the machine's.
    fn told(&mut self, events: Vec<Event>) -> Vec<Event> {
        events
            .into_iter()
            .map(|event| match event {
                Event::Changed(snapshot) => {
                    self.snapshot = with_six_layouts(*snapshot);
                    Event::Changed(Box::new(self.snapshot.clone()))
                }
                other => other,
            })
            .collect()
    }
}

/// `snapshot` with the six layouts, the US one the console's own.
fn with_six_layouts(mut snapshot: Snapshot) -> Snapshot {
    snapshot.keyboard.layouts = LAYOUTS.map(str::to_owned).to_vec();
    snapshot.keyboard.positions = all_positions();
    snapshot
}

impl Machine for SixLayouts {
    fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    fn tick(&mut self) -> Vec<Event> {
        let events = self.machine.tick();
        self.told(events)
    }

    fn apply(&mut self, action: Action) -> Vec<Event> {
        let events = self.machine.apply(action);
        self.told(events)
    }

    fn reask(&mut self) {
        self.machine.reask();
    }
}

#[test]
fn a_keyboard_with_its_own_layout_counts_by_its_own_positions() {
    // A Russian keyboard of its own layout beside the console's US and
    // German ones: each key counts by the layout it was typed on.
    let theme = Theme::new(Capabilities::FULL);
    let machine = SixLayouts::new(&DemoOptions::default());
    let mut session = Session::on(machine, theme, Map::Workspace);
    session.start();
    let on = |name: &str| Some(TypedOn::Layout(name.to_owned()));
    let key = |c| KeyPress::new(Key::Char(c));

    // The Russian `д` is the US `l`: into the kernels column.
    session.press_typed_on(key('д'), on("ru"));
    assert_eq!(session.model().screen, Screen::Kernels);
    session.press_typed_on(key('h'), on("us"));
    assert_eq!(session.model().screen, Screen::Main);
    // Named after the US layout, it has no place.
    session.press_typed_on(key('д'), on("us"));
    assert_eq!(session.model().context(), Context::Main);
    // The German `-` is the US `/`, the filter; the US `-` is nothing.
    session.press_typed_on(key('-'), on("de"));
    assert_eq!(session.model().context(), Context::Text);
    session.press_typed_on(KeyPress::new(Key::Escape), on("de"));
    session.press_typed_on(key('-'), on("us"));
    assert_eq!(session.model().context(), Context::Main);
    session.press_typed_on(key('-'), on("ru"));
    assert_eq!(session.model().context(), Context::Main);
}
