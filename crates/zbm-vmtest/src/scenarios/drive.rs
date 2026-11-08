//! The menu driven on either console: the menu tests press the same keys
//! and wait for the same screens on the serial console and on the display,
//! each through its [`Driver`].
//!
//! - [`Serial`] sends the bytes of a key on the serial console, and reads
//!   the screen of its terminal emulator, 80x25, as the serial scenarios
//!   do: a pattern matches once the screen stayed the same for a moment.
//! - [`Display`] presses keys on QEMU's keyboard through its monitor, and
//!   reads the console's last screen in its mirror.
//!
//! A row is chosen with [`select`]: Down until the selected row holds the
//! name, or with [`select_row`], until it reads the name alone. Filtering
//! and Enter is not used, as the filter's best match may be another row.
//!
//! The mirror holds the text the display shows, drawn as on the serial
//! console: the same box lines and the same `▶`, as the console's
//! programs draw lines whatever the locale. Its screen is larger than
//! 80x25 on most displays, and from about 100 columns the workspace draws
//! its panes side by side: the inspector beside a list, on its lines.

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::expect::{SCREEN, regex};
use crate::vm::Vm;

use super::display::{Monitor, Pace, key_names, last_drawn, press_keys, type_text};

/// The selection's mark after the border of a list: `▶`, or `>` in ASCII.
const SELECTION: &str = r"[│|] [▶>] ";

/// How long [`select`] waits for a screen with a selection, in seconds.
const SELECTION_PATIENCE: u64 = 30;

/// How many settled screens [`select`] looks at for the selection to move
/// after Down, before it takes the list as ended.
const MOVE_LOOKS: usize = 10;

/// How long one of those looks may wait, in seconds.
const MOVE_PATIENCE: u64 = 5;

/// A key the menu tests press.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Key {
    /// Escape.
    Escape,
    /// Enter.
    Enter,
    /// The arrow down.
    Down,
    /// Delete.
    Delete,
    /// Home: the top of a list.
    Home,
    /// Ctrl-U, which clears a field.
    CtrlU,
    /// A character, Shift with a capital.
    Char(char),
    /// A key by QEMU's name, which presses it by its place whatever the
    /// layout: `slash`, `shift-slash`, `meta_l-spc`. A serial terminal has
    /// no such key.
    Qemu(&'static str),
}

impl Key {
    /// The bytes a serial terminal sends for it; `None` for a key by
    /// QEMU's name.
    pub(super) fn serial(self) -> Option<String> {
        let bytes = match self {
            Key::Escape => "\x1b",
            Key::Enter => "\r",
            Key::Down => "\x1b[B",
            Key::Delete => "\x1b[3~",
            Key::Home => "\x1b[H",
            Key::CtrlU => "\x15",
            Key::Char(c) => return Some(c.to_string()),
            Key::Qemu(_) => return None,
        };
        Some(bytes.to_owned())
    }

    /// QEMU's name of it, on the US layout; `None` for a character without
    /// one.
    pub(super) fn qemu_name(self) -> Option<String> {
        let name = match self {
            Key::Escape => "esc",
            Key::Enter => "ret",
            Key::Down => "down",
            Key::Delete => "delete",
            Key::Home => "home",
            Key::CtrlU => "ctrl-u",
            Key::Char(c) => return key_names(&c.to_string())?.pop(),
            Key::Qemu(name) => name,
        };
        Some(name.to_owned())
    }
}

/// How a test presses keys in the menu and reads its screen.
pub(crate) trait Driver {
    /// Presses `key`.
    ///
    /// # Errors
    ///
    /// When the key cannot be pressed.
    fn press(&mut self, vm: &mut Vm<'_>, key: Key) -> Result<()>;

    /// Types `text`, a newline as Enter.
    ///
    /// # Errors
    ///
    /// When it cannot be typed.
    fn text(&mut self, vm: &mut Vm<'_>, text: &str) -> Result<()>;

    /// Waits at most `seconds` until the screen, once it stays the same,
    /// matches `pattern`; returns that screen, or `None` when it did not.
    ///
    /// # Errors
    ///
    /// When the pattern is invalid, or, on the serial console, the machine
    /// stopped: the mirror of the display does not tell, and its screen is
    /// then `None` once the time is up.
    fn screen(&mut self, vm: &mut Vm<'_>, pattern: &str, seconds: u64) -> Result<Option<String>>;
}

/// The serial console, at `pace`.
pub(crate) struct Serial {
    /// The pause after Escape.
    pub(crate) pace: Pace,
}

impl Driver for Serial {
    /// Sends the key's bytes. Escape is followed by a pause, longer than the
    /// menu waits for the rest of a sequence, so that the next key is not
    /// read as one with Alt. A key by QEMU's name has no bytes.
    fn press(&mut self, vm: &mut Vm<'_>, key: Key) -> Result<()> {
        let bytes = key
            .serial()
            .ok_or_else(|| Error::Setup(format!("no serial bytes for {key:?}")))?;
        vm.send(&bytes)?;
        if key == Key::Escape {
            thread::sleep(self.pace.pause);
        }
        Ok(())
    }

    fn text(&mut self, vm: &mut Vm<'_>, text: &str) -> Result<()> {
        vm.send(&text.replace('\n', "\r"))
    }

    /// The terminal emulator's screen, through a screen pattern.
    fn screen(&mut self, vm: &mut Vm<'_>, pattern: &str, seconds: u64) -> Result<Option<String>> {
        match vm.expect(&[&format!("{SCREEN}{pattern}")], seconds) {
            Ok(_) => Ok(Some(vm.console().screen())),
            Err(Error::Timeout { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// The console on the display: keys through `monitor`, screens from the
/// mirror at `mirror`, at `pace`.
pub(crate) struct Display<'m> {
    /// How keys are pressed.
    pub(crate) monitor: &'m Monitor,
    /// The console's mirror.
    pub(crate) mirror: &'m Path,
    /// A key each fifth of its pause; the mirror is read as often.
    pub(crate) pace: Pace,
}

impl Driver for Display<'_> {
    fn press(&mut self, vm: &mut Vm<'_>, key: Key) -> Result<()> {
        let name = key
            .qemu_name()
            .ok_or_else(|| Error::Setup(format!("no key presses {key:?}")))?;
        press_keys(vm, self.monitor, &[name], self.pace.pause)
    }

    fn text(&mut self, vm: &mut Vm<'_>, text: &str) -> Result<()> {
        type_text(vm, self.monitor, text, self.pace.pause)
    }

    /// The mirror's last screen, once two reads a key's time apart find it
    /// the same, or when the time is up.
    fn screen(&mut self, _: &mut Vm<'_>, pattern: &str, seconds: u64) -> Result<Option<String>> {
        let pattern = regex(pattern)?;
        let start = Instant::now();
        let mut before: Option<String> = None;
        loop {
            let screen = last_drawn(self.mirror);
            let is_late = start.elapsed() >= Duration::from_secs(seconds);
            let is_settled = before.as_deref() == Some(screen.as_str());
            if pattern.is_match(&screen) && (is_settled || is_late) {
                return Ok(Some(screen));
            }
            if is_late {
                return Ok(None);
            }
            before = Some(screen);
            thread::sleep(self.pace.pause / 5);
        }
    }
}

/// Whether `c` borders a list: a box line, or `|` in ASCII.
fn is_border(c: char) -> bool {
    c == '|' || ('\u{2500}'..='\u{257f}').contains(&c)
}

/// The selected rows of `screen`: after each selection mark, up to the
/// next border. A screen may show several, such as the environment's in
/// the list beside the snapshots, or under a sheet.
fn selected_rows(screen: &str) -> Vec<&str> {
    screen
        .lines()
        .flat_map(|line| {
            line.match_indices(['│', '|'])
                .filter_map(move |(at, border)| {
                    let rest = line.get(at + border.len()..)?;
                    let row = rest
                        .strip_prefix(" ▶ ")
                        .or_else(|| rest.strip_prefix(" > "))?;
                    let end = row.find(is_border).unwrap_or(row.len());
                    Some(row.get(..end)?.trim_end())
                })
        })
        .collect()
}

/// Whether `row` holds `name` as a whole word: between blanks or the row's
/// ends, so that `two` is not found in `two-clone` or `tmenu/two`.
pub(super) fn has_word(row: &str, name: &str) -> bool {
    !name.is_empty()
        && row.match_indices(name).any(|(at, _)| {
            let before = row.get(..at).and_then(|text| text.chars().next_back());
            let after = row
                .get(at + name.len()..)
                .and_then(|text| text.chars().next());
            before.is_none_or(char::is_whitespace) && after.is_none_or(char::is_whitespace)
        })
}

/// Whether a selected row of `screen` holds `name` as a whole word.
fn has_selected(screen: &str, name: &str) -> bool {
    selected_rows(screen).iter().any(|row| has_word(row, name))
}

/// Presses Down, at most `max` times, until a selected row holds `name` as
/// a whole word; whether one does. It stops when Down no longer moves the
/// selection: the list ended. Not for the Log tab: in ASCII its running
/// glyph is `>` too, so that a row of the log reads as selected.
///
/// # Errors
///
/// When a key cannot be pressed or the screen read.
pub(crate) fn select(
    driver: &mut dyn Driver,
    vm: &mut Vm<'_>,
    name: &str,
    max: usize,
) -> Result<bool> {
    select_by(driver, vm, max, &|screen| has_selected(screen, name))
}

/// Presses Down as [`select`] does, until a selected row reads `row`, all
/// of it: the palette's `Reboot`, beside `Reboot into Firmware Setup`,
/// which holds the word too.
///
/// # Errors
///
/// When a key cannot be pressed or the screen read.
pub(crate) fn select_row(
    driver: &mut dyn Driver,
    vm: &mut Vm<'_>,
    row: &str,
    max: usize,
) -> Result<bool> {
    select_by(driver, vm, max, &|screen| {
        selected_rows(screen).contains(&row)
    })
}

/// Presses Down, at most `max` times, until `is_it` takes the screen;
/// whether it does, before the list ended.
fn select_by(
    driver: &mut dyn Driver,
    vm: &mut Vm<'_>,
    max: usize,
    is_it: &dyn Fn(&str) -> bool,
) -> Result<bool> {
    let Some(mut shown) = driver.screen(vm, SELECTION, SELECTION_PATIENCE)? else {
        return Ok(false);
    };
    for _ in 0..max {
        if is_it(&shown) {
            return Ok(true);
        }
        let before: Vec<String> = selected_rows(&shown)
            .into_iter()
            .map(str::to_owned)
            .collect();
        driver.press(vm, Key::Down)?;
        let mut moved = None;
        for _ in 0..MOVE_LOOKS {
            let screen = driver.screen(vm, SELECTION, MOVE_PATIENCE)?;
            if let Some(screen) = screen.filter(|screen| selected_rows(screen) != before) {
                moved = Some(screen);
                break;
            }
        }
        let Some(screen) = moved else {
            return Ok(false);
        };
        shown = screen;
    }
    Ok(is_it(&shown))
}

#[cfg(test)]
mod tests;
