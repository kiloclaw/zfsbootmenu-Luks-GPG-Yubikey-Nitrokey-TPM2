use std::fs;

use pretty_assertions::assert_eq;

use super::*;
use crate::record::Test;
use crate::testing::{Step, cleanup, scripted, settings};

/// No pauses.
const NOW: Pace = Pace {
    pause: Duration::ZERO,
    patience: 0,
};

/// A menu that shows `screens` in turn: Down shows the next, until the
/// last; it notes every key pressed.
struct Fake {
    screens: Vec<&'static str>,
    at: usize,
    pressed: Vec<Key>,
}

impl Fake {
    fn new(screens: &[&'static str]) -> Fake {
        Fake {
            screens: screens.to_vec(),
            at: 0,
            pressed: Vec::new(),
        }
    }
}

impl Driver for Fake {
    fn press(&mut self, _: &mut Vm<'_>, key: Key) -> Result<()> {
        self.pressed.push(key);
        if key == Key::Down {
            self.at = (self.at + 1).min(self.screens.len().saturating_sub(1));
        }
        Ok(())
    }

    fn text(&mut self, _: &mut Vm<'_>, _: &str) -> Result<()> {
        Ok(())
    }

    fn screen(&mut self, _: &mut Vm<'_>, pattern: &str, _: u64) -> Result<Option<String>> {
        let screen = self.screens.get(self.at).copied().unwrap_or_default();
        Ok(regex(pattern)?.is_match(screen).then(|| screen.to_owned()))
    }
}

/// Runs `select` for `name` on the menu `fake`, with a scripted machine
/// that is not used, in the work directory of the test `label`: tests run
/// in parallel, and each removes its own.
fn select_on(label: &str, fake: &mut Fake, name: &str, max: usize) -> bool {
    let settings = settings(&format!("drive-select-{label}"));
    let test = Test::new(&settings, "drive-select").unwrap();
    let (mut vm, guest) = scripted(&test, Vec::new());
    let selected = select(fake, &mut vm, name, max).unwrap();
    drop(vm);
    assert_eq!(guest.finish(), Vec::<String>::new());
    cleanup(&settings);
    selected
}

/// The snapshots of `tmenu/ROOT/be` beside its list, `two-clone` then
/// `two` selected.
const CLONE: &str = "│ ▶ tmenu/ROOT/be  ││   one          3 h ago │\n\
                     │   tother         ││ ▶ two-clone    2 h ago │\n";
const TWO: &str = "│ ▶ tmenu/ROOT/be  ││   two-clone    2 h ago │\n\
                   │   tother         ││ ▶ two          1 h ago │\n";

#[test]
fn each_key_has_its_serial_bytes_and_its_qemu_name() {
    let keys = [
        (Key::Escape, "\x1b", "esc"),
        (Key::Enter, "\r", "ret"),
        (Key::Up, "\x1b[A", "up"),
        (Key::Down, "\x1b[B", "down"),
        (Key::Left, "\x1b[D", "left"),
        (Key::Right, "\x1b[C", "right"),
        (Key::Delete, "\x1b[3~", "delete"),
        (Key::Tab, "\t", "tab"),
        (Key::BackTab, "\x1b[Z", "shift-tab"),
        (Key::CtrlU, "\x15", "ctrl-u"),
        (Key::Char('n'), "n", "n"),
        (Key::Char('Q'), "Q", "shift-q"),
        (Key::Char(':'), ":", "shift-semicolon"),
        (Key::Char('_'), "_", "shift-minus"),
    ];

    for (key, bytes, name) in keys {
        assert_eq!(key.serial(), bytes, "{key:?}");
        assert_eq!(key.qemu_name().as_deref(), Some(name), "{key:?}");
    }
    assert_eq!(Key::Char('~').qemu_name(), None);
}

#[test]
fn a_name_is_selected_as_a_whole_word_only() {
    assert!(!has_selected(CLONE, "two"));
    assert!(has_selected(CLONE, "two-clone"));
    assert!(has_selected(TWO, "two"));
    assert!(has_selected(TWO, "tmenu/ROOT/be"));
    assert!(!has_selected(TWO, "be"));
    assert!(!has_selected(TWO, "tother"));
    assert!(!has_selected(TWO, ""));
    // A sheet over the list, its border right after the row.
    let sheet = "│ ▶ tmenu/┌──────────────┐\n│        │ ▶ Roll Back to It… │\n";
    assert!(has_selected(sheet, "Roll Back to It…"));
    assert!(!has_selected(sheet, "It"));
    // ASCII, with a marked row and a path whose `>` marks nothing.
    let ascii = " tmenu > tmenu/ROOT/be\n| > + two      1 h ago |\n|   three |\n";
    assert!(has_selected(ascii, "two"));
    assert!(!has_selected(ascii, "tmenu/ROOT/be"));
    assert!(!has_selected(ascii, "three"));
}

#[test]
fn select_presses_down_until_the_name_is_selected() {
    let mut fake = Fake::new(&[CLONE, TWO]);

    assert!(select_on("until", &mut fake, "two", 5));
    assert_eq!(fake.pressed, [Key::Down]);
}

#[test]
fn select_stops_where_the_list_ends() {
    let mut fake = Fake::new(&[CLONE]);

    assert!(!select_on("ends", &mut fake, "two", 5));
    assert_eq!(fake.pressed, [Key::Down]);
}

#[test]
fn select_presses_down_at_most_max_times() {
    let mut fake = Fake::new(&[CLONE, CLONE.replace("clone", "other").leak(), TWO]);

    assert!(!select_on("max", &mut fake, "two", 1));
    assert_eq!(fake.pressed, [Key::Down]);
    assert!(select_on("max", &mut fake, "two", 1));
    assert!(!select_on("max", &mut Fake::new(&["no list"]), "two", 5));
}

#[test]
fn select_takes_the_name_already_selected() {
    let mut fake = Fake::new(&[TWO]);

    assert!(select_on("already", &mut fake, "two", 0));
    assert!(select_on("already", &mut fake, "two", 3));
    assert_eq!(fake.pressed, []);
}

#[test]
fn the_serial_driver_sends_bytes_and_reads_the_terminal_s_screen() {
    let settings = settings("drive-serial");
    let test = Test::new(&settings, "drive-serial").unwrap();
    let script = vec![
        Step::Keys("\x1b"),
        Step::Keys("\x1b[3~"),
        Step::Keys("be\r"),
        Step::Say("\x1b[H\x1b[2J│ ▶ tmenu/ROOT/be │\r\n"),
        Step::Pause(1500),
    ];
    let (mut vm, guest) = scripted(&test, script);
    let mut serial = Serial { pace: NOW };

    serial.press(&mut vm, Key::Escape).unwrap();
    serial.press(&mut vm, Key::Delete).unwrap();
    serial.text(&mut vm, "be\n").unwrap();
    let shown = serial.screen(&mut vm, SELECTION, 5).unwrap().unwrap();
    let absent = serial.screen(&mut vm, "nowhere", 0).unwrap();

    assert!(shown.contains("▶ tmenu/ROOT/be"), "{shown:?}");
    assert_eq!(absent, None);
    assert!(serial.screen(&mut vm, "(", 0).is_err());
    assert!(serial.screen(&mut vm, "nowhere", 5).is_err());
    assert_eq!(guest.finish(), Vec::<String>::new());
    cleanup(&settings);
}

/// Presses nothing.
fn no_key(_: &Vm<'_>, _: &str) -> Result<()> {
    Ok(())
}

/// Takes no screenshot.
fn no_dump(_: &Vm<'_>, _: &Path) -> Result<()> {
    Ok(())
}

#[test]
fn the_display_driver_presses_keys_and_reads_the_mirror_s_last_screen() {
    let settings = settings("drive-display");
    let test = Test::new(&settings, "drive-display").unwrap();
    let mirror = test.dir().join("mirror.txt");
    fs::write(&mirror, "\x0c│ ▶ one │\r\n\x0c│ ▶ two │\r\n\x0c\n").unwrap();
    let (mut vm, guest) = scripted(&test, Vec::new());
    let monitor = Monitor {
        dump: no_dump,
        key: no_key,
    };
    let mut display = Display {
        monitor: &monitor,
        mirror: &mirror,
        pace: NOW,
    };

    display.press(&mut vm, Key::Down).unwrap();
    assert!(display.press(&mut vm, Key::Char('~')).is_err());
    display.text(&mut vm, "be\n").unwrap();
    assert_eq!(
        display.screen(&mut vm, "two", 0).unwrap().as_deref(),
        Some("│ ▶ two │\n")
    );
    assert_eq!(display.screen(&mut vm, "one", 0).unwrap(), None);
    assert!(display.screen(&mut vm, "(", 0).is_err());
    assert_eq!(
        display.screen(&mut vm, "two", 1).unwrap().as_deref(),
        Some("│ ▶ two │\n")
    );
    let mut fake = Fake::new(&[]);
    assert!(!select(&mut fake, &mut vm, "two", 1).unwrap());
    assert!(select(&mut display, &mut vm, "two", 1).unwrap());
    drop(vm);
    assert_eq!(guest.finish(), Vec::<String>::new());
    cleanup(&settings);
}
