//! The pager of the recovery shell (ADR 0018, point 15): a text, such as a
//! topic of `zbm help` or the kernel log, on the terminal, read with the
//! keys of the key map's `Pager` context ([`keymap`]). Its
//! lines scroll, `/` filters them, and `q`, `Esc` and `Ctrl-C` leave it.
//! Without a terminal to read keys from and draw on, or with `TERM=dumb`,
//! the lines are printed as they are, for the line view's users, a screen
//! reader, a Braille display and `grep`
//! (`docs/dev/adr/0011-accessibility.md`).

use std::io::{self, IsTerminal as _, Write};

use ratatui::layout::Rect;
use zbm_display::keys::KeyPress;
use zbm_platform::env::{Environment as _, ProcessEnvironment};

use super::caps::Capabilities;
use super::frontend::{Frontend, Input, Surface, TICK};
use super::keymap::{self, Context, Intent, Map};
use super::term_keys::SERIAL_ESCAPE_WAIT;
use super::terminal;
use super::theme::Theme;
use crate::text::{self, Id, id};

mod view;

/// What a pager shows, which its title names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Topic {
    /// The recovery shell's commands.
    Commands,
    /// The menu's keys.
    Keys,
    /// The kernel command line's options.
    Options,
    /// The kernel log's errors and warnings.
    Logs,
    /// The debug lines of the kernel log.
    Trace,
}

impl Topic {
    /// Its title, as the pager's border shows it.
    pub fn title(self) -> String {
        text::get(self.id())
    }

    /// The message of its title.
    fn id(self) -> Id {
        match self {
            Topic::Commands => id!("pager-title-commands"),
            Topic::Keys => id!("pager-title-keys"),
            Topic::Options => id!("pager-title-options"),
            Topic::Logs => id!("pager-title-logs"),
            Topic::Trace => id!("pager-title-trace"),
        }
    }
}

/// A text to page through: its topic and its lines. The pager draws every
/// line printable, its control characters escaped; printed without a
/// terminal, the lines are written as they are, so text from outside
/// ZFSBootMenu comes escaped already, as the kernel log's lines do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pager {
    /// What the lines are.
    pub topic: Topic,
    /// The lines, without their line ends.
    pub lines: Vec<String>,
}

/// The size drawn in when the terminal does not say its own.
const FALLBACK_SIZE: (u16, u16) = (80, 24);

/// Shows `pager` on the terminal, its keys read with the key map `map`,
/// until it is left; or prints its lines where standard input or output is
/// no terminal, or `TERM` is empty or `dumb`. The rest of a key's sequence may
/// take as long as on a serial console, as the shell may be reached
/// through one, or through SSH.
///
/// # Errors
///
/// When the terminal cannot be set up, read or drawn on, or the lines
/// cannot be written.
pub fn show(pager: &Pager, map: Map) -> io::Result<()> {
    let capabilities = Capabilities::detect(|name| ProcessEnvironment.var(name));
    let stdout = io::stdout();
    if !is_drawn(
        capabilities,
        io::stdin().is_terminal(),
        stdout.is_terminal(),
    ) {
        return print(pager, &mut stdout.lock());
    }
    let size = terminal::size(FALLBACK_SIZE);
    let mut frontend = terminal::on_terminal(size, SERIAL_ESCAPE_WAIT)?;
    run(&mut frontend, pager, (Theme::new(capabilities), map))
}

/// Whether the pager is drawn: on a terminal that can draw, as
/// `capabilities` say, its keys read from standard input and its frames
/// drawn on standard output, both the terminal; else its lines are
/// printed.
fn is_drawn(capabilities: Capabilities, keys_on_terminal: bool, out_on_terminal: bool) -> bool {
    capabilities.interactive && keys_on_terminal && out_on_terminal
}

/// Writes the lines of `pager` to `out`, one a line, as they are.
fn print(pager: &Pager, out: &mut dyn Write) -> io::Result<()> {
    for line in &pager.lines {
        writeln!(out, "{line}")?;
    }
    out.flush()
}

/// Shows `pager` on `frontend` in `theme`, reading keys with the key map
/// `map`, until it is left; then gives the terminal back, as it was, also
/// when it could not be read or drawn on.
fn run<B: Surface, I: Input>(
    frontend: &mut Frontend<B, I>,
    pager: &Pager,
    look: (Theme, Map),
) -> io::Result<()>
where
    B::Error: Send + Sync + 'static,
{
    let read = read(frontend, &mut Reading::new(pager), look);
    let back = frontend.give_back();
    read.and(back)
}

/// Draws `reading` and reads keys into it until it is left.
fn read<B: Surface, I: Input>(
    frontend: &mut Frontend<B, I>,
    reading: &mut Reading<'_>,
    (theme, map): (Theme, Map),
) -> io::Result<()>
where
    B::Error: Send + Sync + 'static,
{
    loop {
        let mut area = Rect::default();
        frontend.draw_with(|frame| area = view::draw(frame, reading, (theme, map)))?;
        reading.area = area;
        if let Some((key, _)) = frontend.key(TICK, None)?
            && reading.press(map, key) == After::Leave
        {
            return Ok(());
        }
    }
}

/// What a key leaves the pager doing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum After {
    /// Showing its text.
    Stay,
    /// Gone.
    Leave,
}

/// A pager being read: its filter, and how far it is scrolled.
struct Reading<'a> {
    pager: &'a Pager,
    /// What the lines kept hold, whatever its case.
    filter: String,
    /// Whether the filter is being typed.
    typing: bool,
    /// The first line shown, of those the filter keeps.
    scroll: usize,
    /// The area the last frame gave the lines: what the moves measure.
    area: Rect,
}

impl<'a> Reading<'a> {
    /// `pager` at its top, without a filter.
    fn new(pager: &'a Pager) -> Reading<'a> {
        Reading {
            pager,
            filter: String::new(),
            typing: false,
            scroll: 0,
            area: Rect::default(),
        }
    }

    /// Where its keys go: the filter's field while it is typed, else the
    /// pager.
    fn context(&self) -> Context {
        if self.typing {
            Context::Text
        } else {
            Context::Pager
        }
    }

    /// The lines the filter keeps: those that hold it, whatever the case;
    /// every line without one.
    fn kept(&self) -> Vec<&'a str> {
        let filter = self.filter.to_lowercase();
        let lines = self.pager.lines.iter().map(String::as_str);
        lines
            .filter(|line| filter.is_empty() || line.to_lowercase().contains(&filter))
            .collect()
    }

    /// `press`, read with the key map `map`.
    fn press(&mut self, map: Map, press: KeyPress) -> After {
        match keymap::input(map, self.context(), press) {
            Some(keymap::Input::Type(c)) => self.filter.push(c),
            Some(keymap::Input::Erase) => {
                self.filter.pop();
            }
            Some(keymap::Input::Clear) => self.filter.clear(),
            Some(keymap::Input::Intent(intent)) => return self.ask(intent),
            None => return After::Stay,
        }
        self.scroll = 0;
        After::Stay
    }

    /// What `intent` does: `Back` clears the filter, or leaves without
    /// one; `/` types the filter, which `Enter` keeps; the moves scroll. The
    /// pager does nothing with the others, help and the tabs among them.
    fn ask(&mut self, intent: Intent) -> After {
        match intent {
            Intent::Back if self.typing || !self.filter.is_empty() => {
                self.typing = false;
                self.filter.clear();
                self.scroll = 0;
            }
            Intent::Back => return After::Leave,
            Intent::Filter => self.typing = true,
            Intent::Primary => self.typing = false,
            Intent::Up
            | Intent::Down
            | Intent::PageUp
            | Intent::PageDown
            | Intent::First
            | Intent::Last => self.scroll = self.scrolled(intent),
            _ => {}
        }
        After::Stay
    }

    /// The first line shown after the move `intent`, measured in the rows
    /// the lines take wrapped: no further than the last page, the one
    /// whose lines end the text whole; a page down starts at the line the
    /// page cut, and a page up shows the lines whole above the first.
    fn scrolled(&self, intent: Intent) -> usize {
        let (width, height) = (usize::from(self.area.width), usize::from(self.area.height));
        let rows: Vec<usize> = self
            .kept()
            .iter()
            .map(|line| view::height(line, width))
            .collect();
        let (above, below) = rows.split_at(self.scroll.min(rows.len()));
        let last = rows
            .len()
            .saturating_sub(fitting(rows.iter().rev(), height));
        match intent {
            Intent::Up => self.scroll.saturating_sub(1),
            Intent::Down => (self.scroll + 1).min(last),
            Intent::PageUp => self
                .scroll
                .saturating_sub(fitting(above.iter().rev(), height)),
            Intent::PageDown => (self.scroll + fitting(below.iter(), height)).min(last),
            Intent::First => 0,
            _ => last,
        }
    }
}

/// How many lines, of those taking `rows` each in turn, `height` rows hold
/// whole; one at least, so that a move moves past a line taller than the
/// area.
fn fitting<'r>(rows: impl Iterator<Item = &'r usize>, height: usize) -> usize {
    let mut used = 0;
    let whole = rows
        .take_while(|rows| {
            used += **rows;
            used <= height
        })
        .count();
    whole.max(1)
}

#[cfg(test)]
pub(crate) mod tests;
