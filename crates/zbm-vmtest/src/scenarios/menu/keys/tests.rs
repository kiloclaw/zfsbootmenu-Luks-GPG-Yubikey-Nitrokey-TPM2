//! The keys test's own tests, on the fake display of `menu-display`'s
//! tests, whose menu draws in the mirror the screen each key brings.

use std::time::Duration;

use pretty_assertions::assert_eq;

use super::*;
use crate::expect::regex;
use crate::scenarios::display::Pace;
use crate::scenarios::menu::display::tests::{COUNTING, Event, on_fake, shown, started};
use crate::scenarios::menu::tests::{all_ok, expected};
use crate::testing::{checks, cleanup, scripted, settings};

/// The status line of `layout`, with Caps Lock when `caps_lock`.
fn status_line(layout: &str, caps_lock: bool) -> String {
    let caps = if caps_lock { "  Caps" } else { "" };
    format!(" [1 Boot]                    {layout}{caps}  08:00 UTC ")
}

/// Start-up, whose workspace shows the status line of `layout`.
fn started_on(layout: &str) -> Vec<Event> {
    let mut script = started(false);
    if let Some(Event::Draw(screen)) = script.last_mut() {
        screen.push('\n');
        screen.push_str(&status_line(layout, false));
    }
    script
}

/// What the console and the menu do for the rounds of `boot`: the status
/// line names each layout as Super+Space brings it, and says Caps Lock
/// is on through a round with it.
fn script_of(boot: &Boot) -> Vec<Event> {
    let mut script = started_on(boot.layout);
    let mut layout = boot.layout;
    let keyed = |script: &mut Vec<Event>, key: Key, line: String| {
        script.push(Event::Key(key.qemu_name().unwrap()));
        script.push(Event::Draw(line));
    };
    for round in &boot.rounds {
        if round.layout != layout {
            layout = round.layout;
            keyed(&mut script, NEXT_LAYOUT, status_line(layout, false));
        }
        if round.caps_lock {
            keyed(&mut script, CAPS_LOCK, status_line(layout, true));
        }
        script.extend(shown(round.steps));
        if round.caps_lock {
            keyed(&mut script, CAPS_LOCK, status_line(layout, false));
        }
    }
    script
}

/// Plays `boot` on the fake display, whose menu draws `script`, with a
/// patience of `patience` seconds; returns what the play returned, how
/// many events of the script were left, and the checks.
fn played(
    name: &str,
    boot: &Boot,
    script: Vec<Event>,
    patience: u64,
) -> (bool, usize, Vec<String>) {
    let settings = settings(&format!("menu-keys-{name}"));
    let test = Test::new(&settings, "menu-keys").unwrap();
    let (mut vm, guest) = scripted(&test, Vec::new());
    let pace = Pace {
        pause: Duration::from_millis(5),
        patience,
    };

    let (done, left) = on_fake(
        (&test, &mut vm),
        COUNTING,
        script,
        pace,
        |test, display, vm| play(test, display, vm, boot),
    );

    drop(vm);
    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    (done.is_ok(), left, found)
}

/// How many checks `boot` makes: the menu shows and its status line
/// names the first layout; then, for each round, the status line names
/// its layout when it changes, says Caps Lock is on and then off for a
/// round with it, and the screens of its steps.
fn checks_of(boot: &Boot) -> usize {
    let mut count = 2;
    let mut layout = boot.layout;
    for round in &boot.rounds {
        count += usize::from(round.layout != layout) + 2 * usize::from(round.caps_lock);
        count += expected(round.steps);
        layout = round.layout;
    }
    count
}

#[test]
fn the_keys_of_six_layouts_reach_the_same_views() {
    let [boot] = boots(true).try_into().unwrap();

    let (done, left, found) = played("six", &boot, script_of(&boot), 30);

    assert!(done, "{found:#?}");
    assert_eq!(left, 0);
    assert_eq!(found.len(), checks_of(&boot), "{found:#?}");
    assert!(all_ok(&found), "{found:#?}");
    let names = |named: &str| found.iter().any(|line| line.ends_with(named));
    for layout in LAYOUTS {
        assert!(names(&format!(
            "- the status line names {layout}, Caps Lock off"
        )));
    }
    assert!(names("- the status line names fr, Caps Lock on"));
    assert!(names("- the status line names ru, Caps Lock on"));
}

#[test]
fn a_layout_switch_waits_for_its_name() {
    let [boot] = boots(true).try_into().unwrap();
    let mut script = started_on("us");
    script.extend(shown(SAME_PLACES));
    // The console did not switch: its status line still names us.
    script.push(Event::Key("meta_l-spc".into()));
    script.push(Event::Draw(status_line("us", false)));

    let (done, left, found) = played("unswitched", &boot, script, 1);

    assert!(!done);
    assert_eq!(left, 0);
    assert_eq!(
        found.last().map(String::as_str),
        Some("FAIL - the status line names de, Caps Lock off"),
        "{found:#?}"
    );
}

#[test]
fn a_round_with_caps_lock_waits_for_the_status_line_to_say_it() {
    let boot = Boot {
        label: "keys-fr".into(),
        append: String::new(),
        layout: "fr",
        rounds: vec![Round::on("fr", FRENCH_CAPS).with_caps_lock()],
    };
    let mut script = started_on("fr");
    script.push(Event::Key("caps_lock".into()));
    script.push(Event::Draw(status_line("fr", false)));

    let (done, _, found) = played("no-caps", &boot, script, 1);

    assert!(!done);
    assert_eq!(
        found.last().map(String::as_str),
        Some("FAIL - the status line names fr, Caps Lock on"),
        "{found:#?}"
    );
}

#[test]
fn the_rounds_follow_the_image_s_layouts_and_press_keys_by_their_place() {
    let mut layouts: Vec<&str> = ROUNDS.iter().map(|round| round.layout).collect();
    layouts.dedup();
    assert_eq!(layouts, LAYOUTS);
    // A round with Caps Lock follows one on its layout without it.
    for pair in ROUNDS.windows(2) {
        if let [before, round] = pair
            && round.caps_lock
        {
            assert_eq!(before.layout, round.layout);
            assert!(!before.caps_lock);
        }
    }
    // A key by its place, never a character the US layout types.
    for step in ROUNDS.iter().flat_map(|round| round.steps) {
        for act in step.acts {
            match *act {
                Act::Press(Key::Qemu(_) | Key::Escape) | Act::Select(_) => {}
                other => panic!("{}: {other:?}", step.what),
            }
        }
    }
}

#[test]
fn one_boot_switches_the_layouts_and_the_fallback_boots_once_per_layout() {
    let [one] = boots(true).try_into().unwrap();
    assert_eq!((one.label.as_str(), one.layout), ("keys", "us"));
    assert_eq!(one.append, format!("{APPEND} zbm.console_status=on"));
    assert_eq!(one.rounds.len(), ROUNDS.len());

    let each = boots(false);
    let labels: Vec<&str> = each.iter().map(|boot| boot.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "keys-us",
            "keys-de",
            "keys-fr",
            "keys-gb",
            "keys-ru",
            "keys-us-dvorak"
        ]
    );
    for boot in &each {
        let chosen = format!(
            "{APPEND} zbm.console_status=on zbm.keyboard_layout={}",
            boot.layout
        );
        assert_eq!(boot.append, chosen);
        assert!(boot.rounds.iter().all(|round| round.layout == boot.layout));
    }
    let rounds: usize = each.iter().map(|boot| boot.rounds.len()).sum();
    assert_eq!(rounds, ROUNDS.len());
}

#[test]
fn a_boot_of_the_fallback_plays_its_layout_without_super_space() {
    let boot = boots(false)
        .into_iter()
        .find(|boot| boot.layout == "ru")
        .unwrap();
    assert_eq!(boot.rounds.len(), 2);

    let (done, left, found) = played("fallback-ru", &boot, script_of(&boot), 30);

    assert!(done, "{found:#?}");
    assert_eq!(left, 0);
    assert_eq!(found.len(), checks_of(&boot), "{found:#?}");
    assert!(all_ok(&found), "{found:#?}");
}

#[test]
fn the_status_line_names_the_layout_and_says_caps_lock_apart() {
    let us = regex(&status("us", false)).unwrap();
    assert!(us.is_match(" ZFSBootMenu            us  08:00 UTC "));
    assert!(us.is_match("x\nus  Num  08:00 UTC\n"));
    assert!(!us.is_match(" us(dvorak)  08:00 UTC "));
    assert!(!us.is_match(" us  Caps  08:00 UTC "));
    assert!(!us.is_match(" ru  08:00 UTC "));
    let dvorak = regex(&status("us(dvorak)", false)).unwrap();
    assert!(dvorak.is_match(" us(dvorak)  08:00 UTC "));
    let caps = regex(&status("fr", true)).unwrap();
    assert!(caps.is_match(" fr  Caps  Num  08:00 UTC "));
    assert!(!caps.is_match(" fr  08:00 UTC "));
}
