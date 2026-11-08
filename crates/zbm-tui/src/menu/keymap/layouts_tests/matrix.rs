//! Every binding of both maps typed on the six layouts, through the model:
//! the layout of each key goes in with `Message::TypedOn`, the key with
//! `Message::Key`, as the console on the display sends them, and what the
//! menu then is and asks for is compared with what the US keyboard's key
//! does on the same menu. By its label, a character does what it does on a
//! US keyboard; by its place, what the US character there does, unless the
//! layout's own character there is bound somewhere: then that one's.

use std::sync::OnceLock;

use zbm_display::keys::{Key, KeyPress};

use super::super::*;
use super::{LAYOUTS, SixLayouts, Stroke, by_label, by_position, is_bound_anywhere, positions};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::machine::Machine;
use crate::menu::model::{Dialog, Model, Prompt, Screen, TypedOn};
use crate::menu::theme::Theme;
use crate::menu::update::{Message, update};
use crate::menu::world::{Action, Method};

/// The views whose bindings the check types: every one but start-up,
/// which binds no character but `?` (a key there is the first of a
/// passphrase), and those that take text.
const VIEWS: [Context; 10] = [
    Main,
    Snapshots,
    Kernels,
    Pools,
    Recovery,
    Log,
    Context::Help,
    Diff,
    Packages,
    Context::Choose,
];

/// The keys that go from the main view into `context` with the key map
/// `map`, as a US keyboard types them; the selection one entry down where
/// there is a list, so that both moves show.
fn route(map: Map, context: Context) -> Vec<Key> {
    let pick = |classic: char, workspace: char| {
        Key::Char(if map == Map::Workspace {
            workspace
        } else {
            classic
        })
    };
    let snapshots = pick('s', '2');
    match context {
        Main => vec![Key::Down],
        Snapshots => vec![snapshots, Key::Down],
        Kernels => vec![pick('k', 'l'), Key::Down],
        Pools => vec![pick('p', '3'), Key::Down],
        Recovery => vec![Key::Char('!'), Key::Down],
        Log => vec![pick('l', '4'), Key::Down],
        Context::Help => vec![Key::Char('?'), Key::Down],
        Diff => vec![snapshots, Key::Char('f'), Key::Down],
        Packages => vec![snapshots, Key::Char('f'), Key::Right, Key::Down],
        _ => vec![Key::Char('x'), Key::Down],
    }
}

/// The demo machine under `map` once start-up is over: started once, as
/// starting takes the longest.
fn started(map: Map) -> &'static SixLayouts {
    static STARTED: [OnceLock<SixLayouts>; 2] = [const { OnceLock::new() }; 2];
    STARTED[usize::from(map == Map::Workspace)].get_or_init(|| {
        let options = DemoOptions {
            classic: map == Map::Classic,
            timeout: None,
            snapshots: 3,
            ..DemoOptions::default()
        };
        let mut machine = SixLayouts::new(&options);
        while !machine.tick().is_empty() {}
        machine
    })
}

/// The menu under `map` in `context`, on a machine whose image offers the
/// six layouts.
fn base(map: Map, context: Context) -> Model {
    let machine = started(map);
    let mut model = Model::new(machine.snapshot().clone(), Theme::new(Capabilities::FULL));
    model.map = map;
    model.started = true;
    model.scanned = true;
    model.screen = Screen::Main;
    let mut session = Session::with_model(machine.clone(), model);
    for key in route(map, context) {
        session.key(key);
    }
    let model = session.into_model();
    assert_eq!(model.context(), context, "{map:?}");
    model
}

/// What the menu is, without where keys were typed and the layout chosen,
/// and what it asked the machine for.
type Outcome = (Model, Vec<Action>);

/// The menu under `map` in `context` after `presses` typed on the layout
/// `typed_on`, `Ctrl-K` having chosen the layout after it when `chosen`.
fn after(
    map: Map,
    context: Context,
    typed_on: &str,
    chosen: bool,
    presses: &[KeyPress],
) -> Outcome {
    let mut model = base(map, context);
    let on = TypedOn::Layout(typed_on.to_owned());
    model = update(model, Message::TypedOn(Some(on))).0;
    if chosen {
        let (dialog, keys) = (model.dialog.take(), model.keys);
        let prompt = Prompt::new("luks-tank".into(), Method::Passphrase, None, None);
        model.dialog = Some(Dialog::Unlock(prompt));
        let ctrl_k = KeyPress::new(Key::Char('k')).with_ctrl();
        model = update(model, Message::Key(ctrl_k)).0;
        model.dialog = dialog;
        model.notification = None;
        model.keys = keys;
    }
    let mut actions = Vec::new();
    for press in presses {
        let (next, asked) = update(model, Message::Key(*press));
        model = next;
        actions.extend(asked);
    }
    model.typed_on = None;
    model.layout = None;
    (model, actions)
}

/// One key of the check: a character, alone or with `Ctrl` or `Alt`, in a
/// view of a map.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Case {
    map: Map,
    context: Context,
    c: char,
    press: KeyPress,
}

/// Every character binding of both maps in each view it works in, and the
/// classic `Ctrl` and `Alt` aliases in their views.
fn cases() -> Vec<Case> {
    let mut all: Vec<Case> = Vec::new();
    let mut add = |map, context, c: char, held: Modifiers| {
        let case = Case {
            map,
            context,
            c,
            press: press(Key::Char(c), held),
        };
        if !all.contains(&case) {
            all.push(case);
        }
    };
    for map in [Map::Classic, Map::Workspace] {
        for binding in map.table() {
            if let Key::Char(c) = binding.key {
                for context in VIEWS
                    .into_iter()
                    .filter(|view| map.is_bound(binding, *view))
                {
                    add(map, context, c, binding.modifiers);
                }
            }
        }
        for (letter, meanings) in CLASSIC {
            for (context, _) in *meanings {
                for held in [CTRL, Modifiers::ALT] {
                    add(map, *context, *letter, held);
                }
            }
        }
    }
    all
}

/// The character the last of `presses` types, if one does.
fn character(presses: &[KeyPress]) -> Option<char> {
    match presses.last()?.key {
        Key::Char(c) => Some(c),
        _ => None,
    }
}

/// What a key typed on a layout should do in a case: what the US key of
/// this character does, or nothing, as a dead key types nothing yet.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Want {
    Us(char),
    Nothing,
}

/// What a key typed by its place should do, typing `typed` there: what
/// the character typed does when the map binds it, else what the US
/// character of the case does.
fn wanted_by_place(case: &Case, typed: &[KeyPress]) -> Want {
    wanted_for(case, character(typed))
}

/// What a key by its place that the menu counts as `typed` should do:
/// what `typed` does when the map binds it, else what the US character of
/// the case does; nothing without a character.
fn wanted_for(case: &Case, typed: Option<char>) -> Want {
    match typed {
        Some(t) if is_bound_anywhere(case.map, t, Modifiers::of(&case.press)) => Want::Us(t),
        Some(_) => Want::Us(case.c),
        None => Want::Nothing,
    }
}

/// A key of a case typed on a layout, and what it should do.
struct Typed {
    name: &'static str,
    chosen: bool,
    presses: Vec<KeyPress>,
    want: Want,
    how: &'static str,
}

impl Typed {
    /// `presses` typed on the layout `name`, by label or by place (`how`),
    /// which should do as `want`.
    fn new(name: &'static str, presses: Vec<KeyPress>, want: Want, how: &'static str) -> Typed {
        Typed {
            name,
            chosen: false,
            presses,
            want,
            how,
        }
    }
}

/// The variants of the check.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Variant {
    /// By label and by place.
    Plain,
    /// The same with Caps Lock on, which changes no action.
    CapsLock,
    /// Letters with Shift, a capital being its letter.
    Shift,
    /// By place on a layout, after `Ctrl-K` chose the next one.
    CtrlK,
}

/// The key labelled with the character of `case` on the layout `name`, if
/// there is one, and its US key, with the case's modifiers.
fn strokes(case: &Case, name: &str) -> (Option<Stroke>, Stroke) {
    let label = by_label(name, case.c).map(|stroke| stroke.held_as(&case.press));
    (label, by_position(case.c).held_as(&case.press))
}

/// The keys `variant` types for `case` on the layout `name`.
fn keys(case: &Case, name: &'static str, variant: Variant) -> Vec<Typed> {
    let (label, place) = strokes(case, name);
    let label_key = |stroke: Stroke| Typed::new(name, stroke.on(name), Want::Us(case.c), "label");
    let place_key = |stroke: Stroke| {
        let presses = stroke.on(name);
        let want = wanted_by_place(case, &presses);
        Typed::new(name, presses, want, "place")
    };
    match variant {
        Variant::Plain => {
            let place = (label != Some(place)).then(|| place_key(place));
            label.map(label_key).into_iter().chain(place).collect()
        }
        Variant::CapsLock => caps_lock_keys(case, name),
        Variant::Shift if case.c.is_ascii_lowercase() => {
            let label = label.map(|stroke| label_key(stroke.with_shift()));
            label
                .into_iter()
                .chain([place_key(place.with_shift())])
                .collect()
        }
        Variant::Shift => Vec::new(),
        Variant::CtrlK => {
            let at = LAYOUTS
                .iter()
                .position(|layout| *layout == name)
                .unwrap_or(0);
            let next = LAYOUTS[(at + 1) % LAYOUTS.len()];
            let held = Modifiers::of(&case.press).has_any(CTRL | Modifiers::ALT);
            let presses = place.on(name);
            // A key the layout types is the chosen layout's character in
            // its place, a dead key's accent too; chords are as typed.
            let chosen = (!held).then(|| positions(next).character_of(place.code, place.shift));
            let typed = character(&presses).map(|own| chosen.flatten().unwrap_or(own));
            let mut key = Typed::new(name, presses, wanted_for(case, typed), "place");
            key.chosen = true;
            vec![key]
        }
    }
}

/// The keys of `case` on the layout `name` with Caps Lock on, where it
/// changes what they type: they should do as they do without it.
fn caps_lock_keys(case: &Case, name: &'static str) -> Vec<Typed> {
    let (label, place) = strokes(case, name);
    let label = label.map(|stroke| (stroke, "label"));
    let mut keys = Vec::new();
    for (stroke, how) in label.into_iter().chain([(place, "place")]) {
        let plain = stroke.on(name);
        let caps = stroke.with_caps_lock().on(name);
        if caps != plain {
            let want = match how {
                "label" => Want::Us(case.c),
                _ => wanted_by_place(case, &plain),
            };
            keys.push(Typed::new(name, caps, want, how));
        }
    }
    keys
}

/// What the US keys do in a view of a map, each typed once.
struct UsKeys {
    map: Map,
    context: Context,
    typed: Vec<((char, KeyPress), Outcome)>,
    nothing: Option<Outcome>,
}

impl UsKeys {
    fn new(map: Map, context: Context) -> UsKeys {
        UsKeys {
            map,
            context,
            typed: Vec::new(),
            nothing: None,
        }
    }

    /// What `want` is with the modifiers of `held`.
    fn outcome(&mut self, want: Want, held: KeyPress) -> &Outcome {
        let Want::Us(c) = want else {
            let (map, context) = (self.map, self.context);
            return self
                .nothing
                .get_or_insert_with(|| after(map, context, "us", false, &[]));
        };
        let index = self.typed.iter().position(|(at, _)| *at == (c, held));
        let index = index.unwrap_or_else(|| {
            let presses = by_position(c).held_as(&held).on("us");
            let outcome = after(self.map, self.context, "us", false, &presses);
            self.typed.push(((c, held), outcome));
            self.typed.len() - 1
        });
        &self.typed[index].1
    }

    /// How many of `cases` the US keys of which leave the menu as a key it
    /// does not bind leaves it: they check nothing.
    fn silent(&mut self, cases: &[&Case]) -> usize {
        let unbound = after(self.map, self.context, "us", false, &[Key::F(12).into()]);
        cases
            .iter()
            .filter(|case| *self.outcome(Want::Us(case.c), case.press) == unbound)
            .count()
    }
}

/// Types every case on the six layouts as `variant` says, and compares
/// what each key does with what it should.
fn check(variant: Variant) {
    let cases = cases();
    let (mut compared, mut silent) = (0, 0);
    for map in [Map::Classic, Map::Workspace] {
        for context in VIEWS {
            let mut us = UsKeys::new(map, context);
            let view: Vec<&Case> = cases
                .iter()
                .filter(|case| case.map == map && case.context == context)
                .collect();
            for case in &view {
                for key in LAYOUTS
                    .into_iter()
                    .flat_map(|name| keys(case, name, variant))
                {
                    let got = after(map, context, key.name, key.chosen, &key.presses);
                    let (how, name, presses, want) = (key.how, key.name, &key.presses, key.want);
                    assert!(
                        got == *us.outcome(key.want, case.press),
                        "{case:?} by {how} on {name}: {presses:?} should do as {want:?}"
                    );
                    compared += 1;
                }
            }
            silent += us.silent(&view);
        }
    }
    assert!(compared > 1000, "{compared} keys compared");
    // A key that changes nothing on the menu checks nothing: the digit of
    // the tab open, `Ctrl-L`, Back in the main view; not many more.
    assert!(silent * 10 < cases.len(), "{silent} keys do nothing");
}

#[test]
fn every_binding_is_reached_on_six_layouts() {
    check(Variant::Plain);
}

#[test]
fn every_binding_is_reached_on_six_layouts_with_caps_lock() {
    check(Variant::CapsLock);
}

#[test]
fn every_letter_is_reached_with_shift_on_six_layouts() {
    check(Variant::Shift);
}

#[test]
fn every_binding_is_reached_on_six_layouts_after_ctrl_k() {
    check(Variant::CtrlK);
}
