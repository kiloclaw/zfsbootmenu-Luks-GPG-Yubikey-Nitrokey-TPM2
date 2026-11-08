//! The steps of the menu tests, which the serial console and the display
//! share: the keys of each step, the screen it then waits for, and what
//! that screen must show. The keys are the workspace's
//! (`crates/zbm-tui/src/menu/keymap/workspace.rs` and `keymap.rs`), and the
//! texts are the menu's at 80x25 (`crates/zbm-tui/locales/en/menu.ftl`, the
//! wireframes `docs/dev/menu-design/wireframes/workspace-*-80x25.snap`).
//!
//! A row is chosen by its name, Down from the top of its list ([`select`]):
//! the order of the rows is the menu's to choose. A screen is matched with
//! any blank between its words, as a dialog may wrap a sentence.

use crate::boot::dialogs::words;
use crate::error::{Error, Result};
use crate::record::Test;
use crate::vm::Vm;

use super::super::drive::{Driver, Key, has_word, select, select_row};
mod tables;

pub(super) use tables::{
    BOOT_CLONE, BOOT_KERNEL, BOOT_TAB, BOOT_TOUR, POOLS, POWER_OFF, REBOOT, SHELL,
    SNAPSHOTS_CHANGES, SNAPSHOTS_DESTROY, SNAPSHOTS_REFUSED,
};

/// How many rows [`select`] may go down a list.
const ROWS: usize = 40;

/// How long a step may wait for its screen, in seconds: an action may
/// import a pool, or copy a boot environment.
const PATIENCE: u64 = 300;

/// The notification of an action's outcome: done, or failed.
const OUTCOME: &str = r"Done: |Failed: ";

/// The word of an outcome that is done.
const DONE: &str = "Done:";

/// What a step does before it looks at the screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Act {
    /// Presses a key.
    Press(Key),
    /// Types text: a name, a passphrase, the palette's filter.
    Type(&'static str),
    /// Goes to the top of the list (Home, which a dialog and a field take
    /// for nothing), then selects the row that holds the name.
    Select(&'static str),
    /// Goes to the top of the list, then selects the row that reads the
    /// name alone.
    SelectRow(&'static str),
}

/// What a step waits for once it acted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Await {
    /// Nothing: the step leaves the menu, for a boot, the shell or power.
    Nothing,
    /// A screen that shows the text.
    Text(&'static str),
    /// The notification of the action's outcome, which must be done.
    Done,
    /// A row that holds the name as a whole word, then the text before
    /// the row's border: a pool and its state, and not the inspector's
    /// text beside the list on a wide screen.
    Row(&'static str, &'static str),
}

/// A step of a menu test.
#[derive(Clone, Copy, Debug)]
pub(super) struct Step {
    /// What it does, for the log and its checks.
    pub(super) what: &'static str,
    /// The keys, in order.
    pub(super) acts: &'static [Act],
    /// The screen it waits for.
    pub(super) awaits: Await,
    /// Texts that screen must show.
    pub(super) shows: &'static [&'static str],
    /// Texts it must not show.
    pub(super) hides: &'static [&'static str],
    /// Names that rows of it must hold as whole words.
    pub(super) rows: &'static [&'static str],
}

impl Step {
    /// The step `what`: `acts`, then a wait for `awaits`.
    pub(super) const fn new(what: &'static str, acts: &'static [Act], awaits: Await) -> Step {
        Step {
            what,
            acts,
            awaits,
            shows: &[],
            hides: &[],
            rows: &[],
        }
    }

    /// The step, whose screen shows `shows`.
    pub(super) const fn showing(self, shows: &'static [&'static str]) -> Step {
        Step { shows, ..self }
    }

    /// The step, whose screen does not show `hides`.
    const fn hiding(self, hides: &'static [&'static str]) -> Step {
        Step { hides, ..self }
    }

    /// The step, whose screen has rows that hold `rows`.
    const fn listing(self, rows: &'static [&'static str]) -> Step {
        Step { rows, ..self }
    }

    /// The pattern of the screen it waits for, if any.
    pub(super) fn pattern(&self) -> Option<String> {
        match self.awaits {
            Await::Nothing => None,
            Await::Text(text) => Some(loose(text)),
            Await::Done => Some(OUTCOME.to_owned()),
            Await::Row(name, text) => Some(format!(
                r"(?m)(?:^|\s){}\s[^│|\n]*{}",
                regex::escape(name),
                loose(text)
            )),
        }
    }
}

/// `text` as a pattern, any blank between its words.
fn loose(text: &str) -> String {
    let words: Vec<String> = text.split_whitespace().map(regex::escape).collect();
    words.join(r"\s+")
}

/// Presses the character `c`.
const fn key(c: char) -> Act {
    Act::Press(Key::Char(c))
}

/// Runs `steps` through `driver`.
///
/// # Errors
///
/// When a key cannot be pressed, a row is not found, or a screen does not
/// come: the menu is then not where the next step expects it.
pub(super) fn run(
    test: &Test,
    driver: &mut dyn Driver,
    vm: &mut Vm<'_>,
    steps: &[Step],
) -> Result<()> {
    for step in steps {
        test.step(step.what);
        for act in step.acts {
            act_on(test, driver, vm, *act, step.what)?;
        }
        let Some(pattern) = step.pattern() else {
            continue;
        };
        let shown = driver.screen(vm, &pattern, PATIENCE)?;
        if step.awaits != Await::Done {
            let what = format!("{}: the menu shows {pattern:?}", step.what);
            test.check(shown.is_some(), &what);
        }
        match shown {
            Some(screen) => check(test, step, &screen),
            None => {
                if step.awaits == Await::Done {
                    test.check(false, &format!("{}: done", step.what));
                }
                return Err(Error::Setup(format!(
                    "{}: the menu did not show {pattern:?}",
                    step.what
                )));
            }
        }
    }
    Ok(())
}

/// Does `act` of the step `what`.
fn act_on(
    test: &Test,
    driver: &mut dyn Driver,
    vm: &mut Vm<'_>,
    act: Act,
    what: &str,
) -> Result<()> {
    match act {
        Act::Press(key) => driver.press(vm, key),
        Act::Type(text) => driver.text(vm, text),
        Act::Select(name) => select_from_top(test, driver, vm, (name, what), select),
        Act::SelectRow(name) => select_from_top(test, driver, vm, (name, what), select_row),
    }
}

/// How a row of a name is selected: [`select`] or [`select_row`].
type Selects = fn(&mut dyn Driver, &mut Vm<'_>, &str, usize) -> Result<bool>;

/// Goes to the top of the list, then selects the row of `name` with
/// `selects`, for the step `what`.
fn select_from_top(
    test: &Test,
    driver: &mut dyn Driver,
    vm: &mut Vm<'_>,
    (name, what): (&str, &str),
    selects: Selects,
) -> Result<()> {
    driver.press(vm, Key::Home)?;
    if selects(driver, vm, name, ROWS)? {
        return Ok(());
    }
    test.check(false, &format!("{what}: a row of {name} is selected"));
    Err(Error::Setup(format!("{what}: no row of {name}")))
}

/// Checks what the step's `screen` shows.
fn check(test: &Test, step: &Step, screen: &str) {
    let said = words(screen);
    let what = step.what;
    if step.awaits == Await::Done {
        let done = said.split(' ').any(|word| word == DONE);
        test.check(done, &format!("{what}: done"));
    }
    for text in step.shows {
        let shown = said.contains(text);
        test.check(shown, &format!("{what}: the menu shows {text:?}"));
    }
    for text in step.hides {
        let hidden = !said.contains(text);
        test.check(hidden, &format!("{what}: the menu does not show {text:?}"));
    }
    for name in step.rows {
        let listed = screen.lines().any(|line| has_word(line, name));
        test.check(listed, &format!("{what}: the menu lists {name}"));
    }
}

#[cfg(test)]
mod tests;
