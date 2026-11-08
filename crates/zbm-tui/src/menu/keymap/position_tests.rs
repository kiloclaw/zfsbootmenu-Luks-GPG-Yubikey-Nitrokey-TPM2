//! Rule 3 of decision 18: on the console on the display, a character the
//! key map binds nowhere counts as the US character of its key, at the same
//! level. The layouts are made by hand, a few keys each.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};
use zbm_display::layout::Positions;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, data};
use crate::menu::model::{Dialog, Model, Prompt, Screen, TypedOn};
use crate::menu::theme::Theme;
use crate::menu::update::{Message, update};
use crate::menu::world::Method;

/// A layout of the keys `keys`: key code, plain and shifted character.
fn layout(keys: &[(u16, char, char)]) -> Positions {
    Positions::from_keys(
        keys.iter()
            .map(|&(code, plain, shifted)| (code, Some(plain), Some(shifted)))
            .collect(),
    )
}

/// A few keys of the Russian layout: `й` `о` `л` `д` `ж` `с` and the key
/// of the US `/`.
fn russian() -> Positions {
    layout(&[
        (16, 'й', 'Й'),
        (36, 'о', 'О'),
        (37, 'л', 'Л'),
        (38, 'д', 'Д'),
        (39, 'ж', 'Ж'),
        (46, 'с', 'С'),
        (53, '.', ','),
    ])
}

/// The key of the US `/` on a German keyboard.
fn german() -> Positions {
    layout(&[(53, '-', '_')])
}

/// A few keys of Dvorak: `o` and `t` on the US `s` and `k`, `,` on the US
/// `w`, `k` on the US `v`.
fn dvorak() -> Positions {
    layout(&[
        (17, ',', '<'),
        (31, 'o', 'O'),
        (37, 't', 'T'),
        (47, 'k', 'K'),
    ])
}

/// The French digit row: symbols, and the digits with Shift.
fn french() -> Positions {
    layout(&[(2, '&', '1'), (3, 'é', '2'), (4, '"', '3'), (5, '\'', '4')])
}

/// What `c` with `modifiers` means in `context` of the workspace, typed on
/// `positions`.
fn on(positions: &Positions, context: Context, c: char, modifiers: Modifiers) -> Option<Input> {
    let us = Positions::us();
    let typing = Typing {
        layout: positions,
        us: &us,
    };
    input_on(
        Map::Workspace,
        context,
        press(Key::Char(c), modifiers),
        Some(typing),
    )
}

fn intent(intent: Intent) -> Input {
    Input::Intent(intent)
}

#[test]
fn an_unbound_character_counts_as_the_us_key_in_its_place() {
    let russian = russian();
    assert_eq!(
        on(&russian, Context::Main, 'л', NONE),
        Some(intent(Intent::Up))
    );
    assert_eq!(
        on(&russian, Context::Main, 'о', NONE),
        Some(intent(Intent::Down))
    );
    assert_eq!(
        on(&russian, Context::Main, 'д', NONE),
        Some(intent(Intent::Kernels))
    );
    assert_eq!(
        on(&russian, Context::Snapshots, 'с', NONE),
        Some(intent(Intent::Clone))
    );
    // Its capital, as rule 2 has it.
    assert_eq!(
        on(&russian, Context::Main, 'Л', NONE),
        Some(intent(Intent::Up))
    );
    // A chord the map binds nowhere: `Ctrl-,` on Dvorak is `Ctrl-W`.
    assert_eq!(
        on(&dvorak(), Context::Main, ',', CTRL),
        Some(intent(Intent::ReadWrite))
    );
    // A key US has nothing for, and a character no key types.
    assert_eq!(on(&russian, Context::Main, 'й', NONE), None);
    assert_eq!(on(&russian, Context::Main, 'щ', NONE), None);
    // Without a layout to look in, a character is as typed.
    let alone = input_on(Map::Workspace, Context::Main, Key::Char('л').into(), None);
    assert_eq!(alone, None);
}

#[test]
fn a_character_the_map_binds_anywhere_counts_as_typed() {
    let dvorak = dvorak();
    // `o` sorts the snapshots, and so means nothing on the Boot tab, where
    // the US `s` of its key would open the snapshots.
    assert_eq!(
        on(&dvorak, Context::Snapshots, 'o', NONE),
        Some(intent(Intent::Sort))
    );
    assert_eq!(on(&dvorak, Context::Main, 'o', NONE), None);
    assert_eq!(on(&dvorak, Context::Main, 'O', NONE), None);
    // With Ctrl, a classic letter means itself too: not `Ctrl-S`.
    assert_eq!(on(&dvorak, Context::Main, 'o', CTRL), None);
    // `k` is bound: it moves up, on whichever key it is.
    assert_eq!(
        on(&dvorak, Context::Main, 'k', NONE),
        Some(intent(Intent::Up))
    );
    // `t` is not, and its key is the US `k`.
    assert_eq!(
        on(&dvorak, Context::Main, 't', NONE),
        Some(intent(Intent::Up))
    );
}

#[test]
fn the_shifted_level_counts_as_the_us_shifted_character() {
    let russian = russian();
    // The key of the US `/` and `?` types `.` and `,` in Russian.
    assert_eq!(
        on(&russian, Context::Main, '.', NONE),
        Some(intent(Intent::Filter))
    );
    assert_eq!(
        on(&russian, Context::Main, ',', Modifiers::SHIFT),
        Some(intent(Intent::Help))
    );
    assert_eq!(
        on(&russian, Context::Log, ',', Modifiers::SHIFT),
        Some(intent(Intent::Help))
    );
    // A capital with Shift is of the shifted level, though its letter
    // names the key: `Ж` is the US `:`, the palette.
    assert_eq!(
        on(&russian, Context::Main, 'Ж', Modifiers::SHIFT),
        Some(intent(Intent::Palette))
    );
    // With Caps Lock, of the plain one: the US `;`, which means nothing.
    assert_eq!(on(&russian, Context::Main, 'Ж', NONE), None);
    assert_eq!(on(&russian, Context::Main, 'ж', NONE), None);
}

#[test]
fn a_keypad_character_counts_as_typed() {
    let us = Positions::us();
    let keypad = |positions: &Positions, c: char| {
        let typing = Typing {
            layout: positions,
            us: &us,
        };
        let press = KeyPress {
            keypad: true,
            ..Key::Char(c).into()
        };
        input_on(Map::Workspace, Context::Main, press, Some(typing))
    };
    // German `-` on the key of the US `/` filters; the keypad's does not.
    let german = german();
    assert_eq!(
        on(&german, Context::Main, '-', NONE),
        Some(intent(Intent::Filter))
    );
    assert_eq!(keypad(&german, '-'), None);
    // Nor does the keypad's `.` on a Russian keyboard, nor on Dvorak, where
    // a main-block `.` would be the US `e`.
    assert_eq!(keypad(&russian(), '.'), None);
    let dvorak = layout(&[(18, '.', '>')]);
    assert_eq!(
        on(&dvorak, Context::Main, '.', NONE),
        Some(intent(Intent::Edit))
    );
    assert_eq!(keypad(&dvorak, '.'), None);
    // A character the map binds means itself from the keypad too.
    assert_eq!(
        keypad(&german, '2'),
        Some(intent(Intent::Tab(Tab::Snapshots)))
    );
}

#[test]
fn caps_lock_on_a_digit_row_of_symbols_counts_as_the_digit() {
    let french = french();
    for (typed, tab) in [
        ('&', Tab::Boot),
        ('é', Tab::Snapshots),
        ('"', Tab::Pools),
        ('\'', Tab::Log),
        // Caps Lock: a capital no key types without it.
        ('É', Tab::Snapshots),
        // Shift: the digit, which the map binds.
        ('2', Tab::Snapshots),
    ] {
        assert_eq!(
            on(&french, Context::Main, typed, NONE),
            Some(intent(Intent::Tab(tab))),
            "{typed}"
        );
    }
}

#[test]
fn text_fields_review_pages_and_start_up_take_characters_as_typed() {
    let russian = russian();
    assert_eq!(
        on(&russian, Context::Text, 'л', NONE),
        Some(Input::Type('л'))
    );
    assert_eq!(
        on(&russian, Context::Review, 'л', NONE),
        Some(Input::Type('л'))
    );
    // At start-up, `,` would be the US `?`, help: it waits for a dialog.
    assert_eq!(on(&russian, Context::Startup, ',', NONE), None);
    assert_eq!(
        on(&russian, Context::Startup, '?', NONE),
        Some(intent(Intent::Help))
    );
}

/// A model of the demo machine on the Boot tab, with Russian too, its
/// keys typed on `typed_on`.
fn model(typed_on: Option<TypedOn>) -> Model {
    let mut snapshot = data::machine(&DemoOptions::default());
    snapshot.keyboard.layouts.push("ru".to_owned());
    snapshot
        .keyboard
        .positions
        .insert("ru".to_owned(), russian());
    snapshot.keyboard.local = true;
    let mut model = Model::new(snapshot, Theme::new(Capabilities::FULL));
    model.screen = Screen::Main;
    update(model, Message::TypedOn(typed_on)).0
}

fn layout_on(name: &str) -> TypedOn {
    TypedOn::Layout(name.to_owned())
}

fn key(model: Model, key: KeyPress) -> Model {
    update(model, Message::Key(key)).0
}

#[test]
fn a_terminal_s_characters_and_the_kernel_console_s_have_no_position() {
    let d: KeyPress = Key::Char('д').into();
    let typing = |typed_on| model(typed_on).typing(&d).is_some();
    assert!(typing(Some(layout_on("ru"))));
    for typed_on in [
        Some(TypedOn::Characters),
        None,
        Some(layout_on("us")),
        Some(layout_on("xx")),
    ] {
        assert!(!typing(typed_on.clone()), "{typed_on:?}");
    }

    // `д` on a Russian keyboard goes into the kernels column; from a
    // terminal it means nothing.
    let kernels = key(model(Some(layout_on("ru"))), d);
    assert_eq!(kernels.context(), Context::Kernels);
    let terminal = key(model(Some(TypedOn::Characters)), d);
    assert_eq!(terminal.context(), Context::Main);
    let console = key(model(None), d);
    assert_eq!(console.context(), Context::Main);
}

#[test]
fn after_ctrl_k_keys_count_by_their_place_in_the_chosen_layout() {
    // `Ctrl-K` in the unlock dialog chooses the layout after German.
    let mut unlock = model(Some(layout_on("de")));
    let prompt = Prompt::new("luks-tank".into(), Method::Passphrase, None, None);
    unlock.dialog = Some(Dialog::Unlock(prompt));
    let mut chosen = key(unlock, KeyPress::new(Key::Char('k')).with_ctrl());
    assert_eq!(chosen.layout(), Some("fr"));
    chosen.dialog = None;

    // The key of `2` on a German keyboard types `é` in French, which is the
    // key of the US `2`: the snapshots' tab.
    let two: KeyPress = Key::Char('2').into();
    let positions = &chosen.snapshot.keyboard.positions;
    let typing = chosen.typing(&two).map(|typing| typing.layout);
    assert_eq!(typing, Some(&positions["fr"]));
    // A chord is not translated, and its character is the German one.
    let z = KeyPress::new(Key::Char('z')).with_ctrl();
    let typing = chosen.typing(&z).map(|typing| typing.layout);
    assert_eq!(typing, Some(&positions["de"]));

    let snapshots = key(chosen, two);
    assert_eq!(snapshots.screen, Screen::Snapshots);
}

#[test]
fn an_image_without_the_us_keymap_looks_keys_up_on_the_decoder_s_us_layout() {
    let d: KeyPress = Key::Char('д').into();
    let mut model = model(Some(layout_on("ru")));
    model.snapshot.keyboard.positions.remove("us");

    let us = model.typing(&d).map(|typing| typing.us.clone());
    assert_eq!(us, Some(Positions::us()));
    assert_eq!(key(model, d).context(), Context::Kernels);
}
