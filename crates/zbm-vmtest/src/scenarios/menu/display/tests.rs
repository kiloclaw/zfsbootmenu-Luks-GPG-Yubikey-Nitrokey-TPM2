//! The display test's own tests, on a fake keyboard that draws in the
//! mirror the screen each key brings, as the menu would.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use pretty_assertions::assert_eq;

use super::*;
use crate::scenarios::display::key_names;
use crate::scenarios::menu::steps::{Act, Step};
use crate::scenarios::menu::tests::{TABLES, all_ok, awaited, checked, expected};
use crate::testing::{Step as Play, checks, cleanup, scripted, settings};

/// Keys and reads of the mirror a millisecond apart.
const SOON: Pace = Pace {
    pause: Duration::from_millis(5),
    patience: 30,
};

/// The countdown, as start-up draws it.
const COUNTING: &str = " Booting tmenu/ROOT/be in 30 s. Press any key to choose another.";

/// Start-up's dialog of the LUKS volume.
const ASKING: &str = "│ Unlock luks-0e8a5e1d │\n│ Passphrase:       │";

/// The workspace's tab bar.
const TABS: &str = " [1 Boot] 2 Snapshots  3 Pools  4 Log";

/// What the fake keyboard expects, and what it draws.
#[derive(Debug)]
enum Event {
    /// A key, by QEMU's name.
    Key(String),
    /// A screen, drawn when the key before it is pressed.
    Draw(String),
}

/// The menu behind the fake keyboard: the mirror it draws in, what it
/// expects, and the keys pressed that it did not expect.
#[derive(Default)]
struct Player {
    mirror: PathBuf,
    script: VecDeque<Event>,
    wrong: Vec<String>,
}

thread_local! {
    /// The menu of the test running on this thread.
    static PLAYER: RefCell<Player> = RefCell::new(Player::default());
}

impl Player {
    /// Takes the key `name`, then draws the screens that follow it.
    fn pressed(&mut self, name: &str) {
        match self.script.pop_front() {
            Some(Event::Key(expected)) if expected == name => {}
            other => self
                .wrong
                .push(format!("{name} pressed, {other:?} expected")),
        }
        while let Some(Event::Draw(_)) = self.script.front() {
            if let Some(Event::Draw(screen)) = self.script.pop_front() {
                draw(&self.mirror, &screen);
            }
        }
    }
}

/// Writes `screen` to the mirror at `path`, after a form feed.
fn draw(path: &Path, screen: &str) {
    let mut mirror = fs::File::options()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(mirror, "\x0c{screen}").unwrap();
}

/// Presses `name` on the fake keyboard.
fn key(_: &Vm<'_>, name: &str) -> Result<()> {
    PLAYER.with(|player| player.borrow_mut().pressed(name));
    Ok(())
}

/// Takes no screenshot.
fn no_dump(_: &Vm<'_>, _: &Path) -> Result<()> {
    Ok(())
}

/// The keys that type `text`.
fn typed(text: &str) -> Vec<Event> {
    key_names(text)
        .unwrap()
        .into_iter()
        .map(Event::Key)
        .collect()
}

/// What the menu expects and draws for `steps`: a screen whose selected
/// row holds each name selected, and the screen each step waits for. A
/// step that presses nothing waits for the menu to change by itself: its
/// screen is drawn with the one before.
fn shown(steps: &[Step]) -> Vec<Event> {
    let mut script = Vec::new();
    for step in steps {
        for act in step.acts {
            match *act {
                Act::Press(key) => script.push(Event::Key(key.qemu_name().unwrap())),
                Act::Type(text) => script.extend(typed(text)),
                Act::Select(name) | Act::SelectRow(name) => {
                    script.push(Event::Key("home".into()));
                    script.push(Event::Draw(format!("│ ▶ {name} │")));
                }
            }
        }
        let Some(lines) = awaited(step) else {
            continue;
        };
        let lines = lines.join("\n");
        match script.last_mut() {
            Some(Event::Draw(before)) if step.acts.is_empty() => {
                before.push('\n');
                before.push_str(&lines);
            }
            _ => script.push(Event::Draw(lines)),
        }
    }
    script
}

/// Start-up on the display, with its dialog of the LUKS volume when
/// `luks`: Enter skips it, and Escape stops the countdown.
fn started(luks: bool) -> Vec<Event> {
    let mut script = Vec::new();
    if luks {
        script.extend([Event::Key("ret".into()), Event::Draw(COUNTING.into())]);
    }
    script.extend([Event::Key("esc".into()), Event::Draw(TABS.into())]);
    script
}

/// The display of the test, at `pace`, on the mirror at `mirror`.
fn display<'m>(monitor: &'m Monitor, mirror: &'m Path, pace: Pace) -> Display<'m> {
    Display {
        monitor,
        mirror,
        pace,
    }
}

/// Runs `drive` on a display whose menu starts with its LUKS dialog when
/// `luks`, then plays `script`, while the serial console says `said`;
/// returns the checks, once the menu took every key it expected.
fn played(
    name: &str,
    luks: bool,
    script: Vec<Event>,
    said: Vec<Play>,
    drive: Drive,
) -> Vec<String> {
    let found = checked(&format!("display-{name}"), said, |test, vm| {
        let mirror = test.dir().join("mirror.txt");
        draw(&mirror, if luks { ASKING } else { COUNTING });
        PLAYER.with(|player| {
            *player.borrow_mut() = Player {
                mirror: mirror.clone(),
                script: script.into(),
                wrong: Vec::new(),
            };
        });
        let monitor = Monitor { dump: no_dump, key };
        drive(test, &mut display(&monitor, &mirror, SOON), vm)
    });
    let player = PLAYER.with(RefCell::take);
    assert_eq!(player.wrong, Vec::<String>::new());
    assert!(player.script.is_empty(), "{:?}", player.script);
    found
}

#[test]
fn every_key_of_the_tables_is_pressed_on_the_display_s_keyboard() {
    for step in TABLES.iter().flat_map(|(_, table)| table.iter()) {
        for act in step.acts {
            let pressed = match *act {
                Act::Press(key) => key.qemu_name().is_some(),
                Act::Type(text) => key_names(text).is_some(),
                Act::Select(_) | Act::SelectRow(_) => true,
            };
            assert!(pressed, "{}: {act:?}", step.what);
        }
    }
}

#[test]
fn the_snapshot_tasks_play_on_the_display_from_the_boot_tab_each_time() {
    let tables = [
        steps::SNAPSHOTS_CHANGES,
        steps::BOOT_TAB,
        steps::SNAPSHOTS_DESTROY,
        steps::BOOT_TAB,
        steps::SNAPSHOTS_REFUSED,
        steps::POWER_OFF,
    ];
    let mut script = started(false);
    script.extend(tables.iter().flat_map(|table| shown(table)));
    let said = vec![Play::Say("reboot: Power down\r\n")];

    let found = played("snapshots", false, script, said, snapshots);

    let screens: usize = tables.iter().map(|table| expected(table)).sum();
    assert_eq!(found.len(), 1 + screens + 1, "{found:#?}");
    assert!(all_ok(&found), "{found:#?}");
}

#[test]
fn the_pool_tasks_play_on_the_display_after_the_luks_dialog() {
    let mut script = started(true);
    script.extend(shown(steps::POOLS));
    script.extend(shown(steps::POWER_OFF));
    let said = vec![Play::Say("reboot: Power down\r\n")];

    let found = played("pools", true, script, said, pools);

    assert_eq!(found.len(), 1 + expected(steps::POOLS) + 1, "{found:#?}");
    assert!(all_ok(&found), "{found:#?}");
}

#[test]
fn the_palette_and_the_edited_command_line_play_on_the_display() {
    let mut script = started(false);
    script.extend(shown(steps::BOOT_TOUR));
    script.extend(shown(steps::BOOT_KERNEL));
    let said = vec![Play::Say(
        "BE-INIT-START\r\nBE-CMDLINE: console=ttyS0 loglevel=4 zbmtest.edited=1\r\n\
         BE-INIT-DONE\r\n",
    )];

    let found = played("kernel", false, script, said, kernel);

    let screens = expected(steps::BOOT_TOUR) + expected(steps::BOOT_KERNEL);
    assert_eq!(found.len(), 1 + screens + 1, "{found:#?}");
    assert!(all_ok(&found), "{found:#?}");
    assert!(
        found.last().unwrap().contains("holds the edit"),
        "{found:#?}"
    );
}

#[test]
fn a_clone_boots_from_the_display() {
    let mut script = started(false);
    script.extend(shown(steps::BOOT_CLONE));
    let said = vec![Play::Say(
        "BE-INIT-START\r\nBE-ROOT: tmenu/ROOT/booted\r\nBE-INIT-DONE\r\n",
    )];

    let found = played("clone", false, script, said, clone);

    assert_eq!(
        found.len(),
        1 + expected(steps::BOOT_CLONE) + 1,
        "{found:#?}"
    );
    assert!(all_ok(&found), "{found:#?}");
}

#[test]
fn the_palette_s_reboot_resets_the_machine() {
    let mut script = started(false);
    script.extend(shown(steps::REBOOT));
    let said = vec![Play::Say("sysrq: Resetting\r\n")];

    let found = played("reboot", false, script, said, reboot);

    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(all_ok(&found), "{found:#?}");
    assert!(found[1].contains("Reboot resets"), "{found:#?}");
}

#[test]
fn a_reboot_the_kernel_does_not_report_as_a_reset_fails() {
    let mut script = started(false);
    script.extend(shown(steps::REBOOT));
    let said = vec![Play::Say("reboot: Restarting system\r\n")];

    let found = played("reboot-unsaid", false, script, said, reboot);

    assert_eq!(
        found.last().map(String::as_str),
        Some("FAIL - the palette's Reboot resets the machine"),
        "{found:#?}"
    );
}

#[test]
fn a_panic_of_the_boot_environment_ends_the_wait_for_its_report_and_is_noted() {
    let settings = settings("menu-display-be-panic");
    let test = Test::new(&settings, "menu-display").unwrap();
    let said = vec![Play::Say("Kernel panic - not syncing: VFS\r\n")];
    let (mut vm, guest) = scripted(&test, said);

    let report = be_report(&test, &mut vm).unwrap();

    drop(vm);
    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(report.prompts, ["be-panic"]);
    assert_eq!(report.be_line("BE-CMDLINE"), "");
    let steps = fs::read_to_string(test.dir().join("steps.log")).unwrap();
    assert!(
        steps.contains("note - the boot environment stopped: be-panic"),
        "{steps}"
    );
    cleanup(&settings);
}

/// Starts the menu on a display whose mirror shows `first`, then each
/// screen of `script` as its keys are pressed, with a patience of
/// `patience` seconds; returns whether it started, how long that took,
/// and the checks.
fn start_on(
    name: &str,
    first: &str,
    script: Vec<Event>,
    patience: u64,
) -> (bool, Duration, Vec<String>) {
    let settings = settings(&format!("menu-display-{name}"));
    let test = Test::new(&settings, "menu-display").unwrap();
    let mirror = test.dir().join("mirror.txt");
    draw(&mirror, first);
    PLAYER.with(|player| {
        *player.borrow_mut() = Player {
            mirror: mirror.clone(),
            script: script.into(),
            wrong: Vec::new(),
        };
    });
    let (mut vm, guest) = scripted(&test, Vec::new());
    let monitor = Monitor { dump: no_dump, key };
    let pace = Pace { patience, ..SOON };
    let begun = Instant::now();

    let started = start(&test, &mut display(&monitor, &mirror, pace), &mut vm).is_ok();

    let took = begun.elapsed();
    drop(vm);
    assert_eq!(guest.finish(), Vec::<String>::new());
    let player = PLAYER.with(RefCell::take);
    assert_eq!(player.wrong, Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    (started, took, found)
}

#[test]
fn a_display_that_shows_no_countdown_fails_the_start() {
    let (started, _, found) = start_on("no-countdown", "zbm console: on /dev/dri/card0", vec![], 0);

    assert!(!started);
    assert_eq!(found, ["FAIL - the countdown shows on the display"]);
}

#[test]
fn a_display_that_shows_nothing_fails_the_start_after_one_patience() {
    let (started, took, found) = start_on("blank", "", vec![], 1);

    assert!(!started);
    assert!(took < Duration::from_millis(1900), "{took:?}");
    assert_eq!(found, ["FAIL - the countdown shows on the display"]);
}

#[test]
fn a_luks_dialog_followed_by_no_countdown_fails_the_start() {
    let script = vec![
        Event::Key("ret".into()),
        Event::Draw("zbm: no pools".into()),
    ];

    let (started, _, found) = start_on("luks-no-countdown", ASKING, script, 0);

    assert!(!started);
    assert_eq!(found, ["FAIL - the countdown shows on the display"]);
}
