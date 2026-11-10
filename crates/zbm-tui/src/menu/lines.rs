//! The line view of the menu (ROADMAP.md UI8, ADR 0011): the same model and
//! updates as the screen, printed as lines that a screen reader reads in
//! order, and typed as lines.
//!
//! A screen prints as its title, its rows (numbered when they are items to
//! choose), what happened last, the dialog over it with its choices
//! numbered, and what can be typed; it prints again only when that
//! changed, never in place, and the countdown says its seconds once a
//! second. A line typed is a number, which chooses that item, a number and
//! a key (`2 k`), one of the key map's keys, or `ok`, `back` and `help`,
//! and in the workspace's views with the tab bar `boot`, `snapshots`,
//! `pools` and `log` for its tabs; in a field that takes text, the line is
//! the text, and a blank line goes back. A review page takes the target's
//! name as a field does; the line view says the first rows of what it
//! loses, and cannot scroll it or take its safer action. The terminal
//! keeps its own line editing and echo, which is off while a secret is
//! typed.

use std::io;
use std::time::Duration;

use zbm_display::keys::{Key, KeyPress};
use zbm_platform::Clock;
use zbm_platform::console::{Console, Timed};

use super::frontend::TICK;
use super::keymap::{self, Context, Intent, Map, Tab};
use super::machine::{Machine, RealMachine};
use super::model::Model;
use super::screen::MenuScreen;
use super::secret::is_asking_secret;
use super::session::Session;
use super::theme::Theme;
use super::view::words;
use crate::text::{self, Id, Language, id, text};

/// What a typed line asks for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Typed {
    /// These key presses.
    Keys(Vec<KeyPress>),
    /// The page again.
    Again,
    /// Nothing the menu knows: the line, to say so.
    Unknown(String),
}

/// The keys of choosing item `number`, from 1, of a list or a dialog: the
/// first, then down to it.
fn choose(number: usize) -> Vec<KeyPress> {
    let mut keys = vec![KeyPress::new(Key::Home)];
    keys.extend((1..number).map(|_| KeyPress::new(Key::Down)));
    keys
}

/// The key of a word or a character typed alone in `context`: `ok`, `back`,
/// `help`, the name of a tab where the key map `map` binds a key to it
/// there, or one of the key map's characters. A word is the same in any
/// case (`OK`), as a capital is its letter, and in English as in the
/// menu's language (rule 4 of decision 18).
fn key_of(word: &str, context: Context, map: Map) -> Option<KeyPress> {
    let is = |known: Id| is_word(word, known);
    if is(id!("lines-word-ok")) {
        return Some(KeyPress::new(Key::Enter));
    }
    if is(id!("lines-word-back")) {
        return Some(KeyPress::new(Key::Escape));
    }
    if is(id!("lines-word-help")) {
        return Some(KeyPress::new(Key::Char('?')));
    }
    if let Some(tab) = Tab::ALL.into_iter().find(|tab| is(tab_word(*tab))) {
        return map
            .binding_in(Intent::Tab(tab), context)
            .map(|binding| keymap::press(binding.key, binding.modifiers));
    }
    let mut chars = word.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(KeyPress::new(Key::Char(c))),
        _ => None,
    }
}

/// Whether `word` is the word `known`, in any case, in the menu's language
/// or in English.
fn is_word(word: &str, known: Id) -> bool {
    let word = word.to_lowercase();
    let is = |known: String| word == known.to_lowercase();
    is(text::get(known))
        || (text::language() != Language::English
            && is(text::in_language(Language::English, || text::get(known))))
}

/// The word that chooses `tab`: `pools`.
fn tab_word(tab: Tab) -> Id {
    match tab {
        Tab::Boot => id!("lines-word-boot"),
        Tab::Snapshots => id!("lines-word-snapshots"),
        Tab::Pools => id!("lines-word-pools"),
        Tab::Log => id!("lines-word-log"),
    }
}

/// What `line` asks for, typed in `context` with the key map `map`.
pub fn typed(line: &str, context: Context, map: Map) -> Typed {
    if keymap::is_text(context) {
        let text = line.trim_end_matches(['\r', '\n']);
        if text.is_empty() {
            return Typed::Keys(vec![KeyPress::new(Key::Escape)]);
        }
        // The field is emptied first: a name may be offered in it.
        let mut keys = vec![KeyPress::new(Key::Char('u')).with_ctrl()];
        keys.extend(text.chars().map(|c| KeyPress::new(Key::Char(c))));
        keys.push(KeyPress::new(Key::Enter));
        return Typed::Keys(keys);
    }
    let words: Vec<&str> = line.split_whitespace().collect();
    let number = |word: &str| word.parse::<usize>().ok().filter(|number| *number > 0);
    match words.as_slice() {
        [] => Typed::Again,
        [word] if word.parse::<usize>().is_ok() => match number(word) {
            Some(number) => {
                let mut keys = choose(number);
                keys.push(KeyPress::new(Key::Enter));
                Typed::Keys(keys)
            }
            None => Typed::Unknown(line.trim().to_owned()),
        },
        [word] => match number(word) {
            Some(number) => {
                let mut keys = choose(number);
                keys.push(KeyPress::new(Key::Enter));
                Typed::Keys(keys)
            }
            None => key_of(word, context, map).map_or_else(
                || Typed::Unknown(line.trim().to_owned()),
                |key| Typed::Keys(vec![key]),
            ),
        },
        [first, second] => match (number(first), key_of(second, context, map)) {
            (Some(number), Some(key)) => {
                let mut keys = choose(number);
                keys.push(key);
                Typed::Keys(keys)
            }
            _ => Typed::Unknown(line.trim().to_owned()),
        },
        _ => Typed::Unknown(line.trim().to_owned()),
    }
}

/// `items` numbered from 1, indented, but for those at `headers`, a
/// sorted list of their indexes, which are indented alone: a number
/// counts the items to choose.
fn numbered(items: &[String], headers: &[usize]) -> Vec<String> {
    let mut number = 0;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            if headers.binary_search(&index).is_ok() {
                return format!("  {item}");
            }
            number += 1;
            format!("  {number}) {item}")
        })
        .collect()
}

/// Whether the menu takes text where `model` is: a field, a filter, the
/// palette, the command line.
fn is_text(model: &Model) -> bool {
    matches!(model.context(), Context::Text | Context::Command)
}

/// Whether the names of the tabs choose them where `model` is: in the
/// workspace, in a view whose digits choose a tab, which start-up, the Boot
/// tab, is not.
fn has_tab_words(model: &Model) -> bool {
    model
        .map
        .binding_in(Intent::Tab(Tab::Boot), model.context())
        .is_some()
}

/// The page of `model`: everything the line view says of it, but the
/// countdown, which it says apart.
pub fn page(model: &Model) -> Vec<String> {
    let mut page = vec![words::title(model)];
    let (rows, is_numbered) = words::rows(model);
    if is_numbered {
        if rows.is_empty() {
            page.push(text!("lines-none"));
        }
        page.extend(numbered(&rows, &words::headers(model)));
    } else {
        page.extend(rows.into_iter().map(|row| format!("  {row}")));
    }
    page.extend(words::notification(model));
    let dialog = words::dialog(model);
    if let Some(said) = &dialog {
        page.push(String::new());
        page.push(said.title.clone());
        page.extend(said.lines.iter().map(|line| format!("  {line}")));
        page.extend(numbered(&said.choices, &said.headers));
    }
    page.push(String::new());
    if model.context() == Context::Review {
        page.push(text!("lines-review"));
    } else if is_text(model) {
        page.push(text!("lines-text"));
    } else {
        let workspace = model.map == Map::Workspace;
        if is_numbered || dialog.is_some_and(|said| !said.choices.is_empty()) {
            page.push(if workspace {
                text!("lines-number-workspace")
            } else {
                text!("lines-number")
            });
        }
        let keys = words::commands(model).join(", ");
        if !keys.is_empty() {
            page.push(text!("lines-keys", keys = keys));
        }
        page.push(if has_tab_words(model) {
            text!("lines-words-workspace")
        } else {
            text!("lines-words")
        });
    }
    page
}

/// The line view on a console: what it printed last.
pub struct LineFront<'a> {
    console: &'a dyn Console,
    clock: &'a dyn Clock,
    /// The page printed last; empty to print it again.
    shown: Vec<String>,
    /// The countdown said last.
    countdown: Option<String>,
}

impl<'a> LineFront<'a> {
    /// The line view on `console`, whose time passes on `clock`.
    pub fn new(console: &'a dyn Console, clock: &'a dyn Clock) -> LineFront<'a> {
        LineFront {
            console,
            clock,
            shown: Vec::new(),
            countdown: None,
        }
    }

    /// Writes `text`.
    fn write(&self, text: &str) -> io::Result<()> {
        self.console.write(text.as_bytes())
    }

    /// Prints the page of `model` when it changed, then the prompt; and
    /// the countdown's seconds when they changed.
    ///
    /// # Errors
    ///
    /// When the console cannot be written.
    pub fn show(&mut self, model: &Model) -> io::Result<()> {
        let page = page(model);
        if page != self.shown {
            let mut text = String::from("\n");
            for line in &page {
                text.push_str(line);
                text.push('\n');
            }
            self.write(&text)?;
            self.shown = page;
            // The countdown, if it runs, is said again after the page.
            if self.countdown.is_some() {
                self.countdown = Some(String::new());
            }
        }
        let countdown = words::countdown(model);
        if countdown != self.countdown {
            match &countdown {
                Some(said) => self.write(&format!("{said}\n"))?,
                None => self.write(&format!("{}\n", text!("lines-countdown-stopped")))?,
            }
            self.countdown = countdown;
        }
        Ok(())
    }

    /// Reads a line within `timeout`, with echo off while `model` asks for
    /// a secret: what it asks for; `None` when nothing came.
    ///
    /// # Errors
    ///
    /// When the console cannot be read, or its input ended.
    fn read(&self, model: &Model, timeout: Duration) -> io::Result<Option<Typed>> {
        let secret = is_asking_secret(model);
        if secret {
            self.console.set_echo(false);
        }
        let read = self.console.read_secret_within(timeout);
        if secret {
            self.console.set_echo(true);
        }
        match read? {
            Timed::TimedOut => Ok(None),
            Timed::Ended => Err(io::ErrorKind::UnexpectedEof.into()),
            Timed::Read(line) => {
                if secret {
                    // The person's Enter did not show.
                    self.write("\n")?;
                }
                let text = String::from_utf8_lossy(line.expose());
                Ok(Some(typed(&text, model.context(), model.map)))
            }
        }
    }

    /// Runs `session` until the menu ends: pages out, lines in, time and
    /// what the machine sends; why it ended.
    ///
    /// # Errors
    ///
    /// When the console cannot be written or read.
    pub fn run<M: Machine>(&mut self, session: &mut Session<M>) -> io::Result<String> {
        let mut last_tick = self.clock.now();
        loop {
            if let Some(why) = session.leaving() {
                let why = why.to_owned();
                if !why.is_empty() {
                    self.write(&format!("{why}\n"))?;
                }
                return Ok(why);
            }
            self.show(session.model())?;
            let passed = self.clock.now().saturating_sub(last_tick);
            let typed = self.read(session.model(), TICK.saturating_sub(passed))?;
            session.poll();
            match typed {
                Some(Typed::Keys(keys)) => keys.into_iter().for_each(|key| session.press(key)),
                Some(Typed::Again) => self.shown.clear(),
                Some(Typed::Unknown(line)) => {
                    self.write(&format!("{}\n", text!("lines-unknown", typed = line)))?;
                }
                None => {}
            }
            let passed = self.clock.now().saturating_sub(last_tick);
            if passed >= TICK {
                last_tick = self.clock.now();
                session.tick(u64::try_from(passed.as_millis()).unwrap_or(u64::MAX));
            }
            while let Some(action) = session.console_action() {
                session.perform(action);
                // The action wrote on the console: the page again.
                self.shown.clear();
            }
        }
    }
}

/// The menu's screen as lines (ADR 0011), for the session's menu: each
/// round goes on from the model the last one left.
pub struct LineScreen<'a> {
    console: &'a dyn Console,
    clock: &'a dyn Clock,
    theme: Theme,
    map: Map,
    model: Option<Model>,
}

impl<'a> LineScreen<'a> {
    /// The line view on `console`, whose time passes on `clock`, in `theme`
    /// with the key map `map`.
    pub fn new(
        console: &'a dyn Console,
        clock: &'a dyn Clock,
        theme: Theme,
        map: Map,
    ) -> LineScreen<'a> {
        LineScreen {
            console,
            clock,
            theme,
            map,
            model: None,
        }
    }
}

impl MenuScreen for LineScreen<'_> {
    fn run(&mut self, machine: RealMachine<'_, '_>) -> io::Result<String> {
        let mut session = match self.model.take() {
            Some(mut model) => {
                model.begin_round();
                Session::with_model(machine, model)
            }
            None => Session::on(machine, self.theme, self.map),
        };
        let why = LineFront::new(self.console, self.clock).run(&mut session);
        self.model = Some(session.into_model());
        why
    }
}

/// Whether the menu shows as lines: as `asked` (`zbm.menu`, `Some(true)`
/// for lines), or, unless it asks for the screen, on a terminal that runs
/// no full-screen program (`TERM` dumb or unset: not `interactive`), or
/// where `speakup` reads the console.
pub fn is_wanted(asked: Option<bool>, interactive: bool, speakup: bool) -> bool {
    asked.unwrap_or(!interactive || speakup)
}

#[cfg(test)]
mod tests;
