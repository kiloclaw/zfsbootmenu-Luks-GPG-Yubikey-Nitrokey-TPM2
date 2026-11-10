//! The command palette, over the dimmed view: the query, then the best
//! matches of each section, under its heading, each with its keys and
//! costs on the right.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{HighlightSpacing, List, ListItem, ListState, Paragraph};

use super::dialogs::centred;
use super::lists;
use crate::menu::format::fit;
use crate::menu::keymap;
use crate::menu::model::{Costs, Model, Palette, PaletteItem, Section};
use crate::menu::theme::{Glyph, Role};
use crate::text::text;

/// The command palette: the query, then the best matches of each section,
/// under its heading.
pub(super) fn palette(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let Some(palette) = &model.palette else {
        return;
    };
    let theme = model.theme;
    let items = &palette.items;
    let height = area.height.saturating_sub(2).min(16);
    let width = area.width.saturating_sub(4).min(70);
    let rect = centred(theme, frame, area, (width, height));
    let count = text!(
        "list-count-of",
        shown = palette.matches.len(),
        total = items.len()
    );
    let block = lists::counted(theme, &title(palette), &count);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let [query, list] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    frame.render_widget(Paragraph::new(format!(": {}", palette.query)), query);
    let column = u16::try_from(palette.query.chars().count() + 2).unwrap_or(u16::MAX);
    frame.set_cursor_position(Position::new(query.x.saturating_add(column), query.y));
    let room = usize::from(list.width).saturating_sub(2);
    let (rows, selected) = rows(model, palette, usize::from(list.height), room);
    let widget = List::new(rows)
        .highlight_symbol(Line::from(format!("{} ", theme.glyph(Glyph::Selected))))
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_style(theme.style(Role::Selected));
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(widget, list, &mut state);
}

/// The rows of the palette's matches, each section's heading before its
/// first, as many as fill `height` rows and reach the selection, each
/// `room` columns wide with its keys and costs; and the row of the
/// selection, which is never a heading.
fn rows(
    model: &Model,
    palette: &Palette,
    height: usize,
    room: usize,
) -> (Vec<ListItem<'static>>, usize) {
    let theme = model.theme;
    let headings = (0..=palette.selected)
        .filter(|at| palette.heading_at(*at).is_some())
        .count();
    let selected = palette.selected + headings;
    let wanted = height.max(selected + 1);
    let words = CostWords::new();
    let mut rows = Vec::new();
    for (at, ranked) in palette.matches.iter().enumerate() {
        if rows.len() >= wanted {
            break;
        }
        if let Some(section) = palette.heading_at(at) {
            let heading = section_name(section).to_uppercase();
            let heading = fit(&heading, room, theme.is_ascii());
            rows.push(ListItem::new(Line::styled(
                heading,
                theme.style(Role::Header),
            )));
        }
        let Some(item) = palette.items.get(ranked.index) else {
            continue;
        };
        rows.push(ListItem::new(row(
            model,
            item,
            &ranked.positions,
            room,
            &words,
        )));
    }
    (rows, selected)
}

/// The row of `item`, `room` columns wide: its label, the characters at
/// `positions` highlighted, then on the right its keys, muted, and its
/// costs, which take at most half the row ([`CostWords::fitted`]).
fn row(
    model: &Model,
    item: &PaletteItem,
    positions: &[usize],
    room: usize,
    words: &CostWords,
) -> Line<'static> {
    let theme = model.theme;
    let ascii = theme.is_ascii();
    let (keys, costs) = words.fitted(&item.keys, item.costs, room / 2, ascii);
    let gap = if keys.is_empty() || costs.is_empty() {
        ""
    } else {
        " "
    };
    let right = format!("{gap}{costs}");
    let wide = keys.chars().count() + right.chars().count();
    let label = match wide {
        0 => fit(&item.label, room, ascii),
        _ => fit(&item.label, room.saturating_sub(wide + 1), ascii),
    };
    let mut spans = lists::highlighted(theme, &label, positions);
    if wide > 0 {
        let gap = room.saturating_sub(label.chars().count() + wide).max(1);
        spans.push(Span::raw(" ".repeat(gap)));
        spans.push(Span::styled(keys, theme.style(Role::Muted)));
        spans.push(Span::styled(right, theme.style(Role::Warning)));
    }
    Line::from(spans)
}

/// The words of the costs, made once for each drawing, not for each row
/// (B5): what a review page asks, the read/write import, the key, then the
/// PIN, which is the lock's mark (ADR 0010).
pub(super) struct CostWords {
    /// A review page asks for the target's name.
    review: String,
    /// The pool is imported read/write first.
    writes: String,
    /// The environment's key may be asked for.
    key: String,
    /// The PIN is asked: the lock's mark.
    pin: String,
}

impl CostWords {
    /// The words, in the menu's language.
    pub(super) fn new() -> CostWords {
        CostWords {
            review: text!("cost-review"),
            writes: text!("cost-writes"),
            key: text!("cost-key"),
            pin: keymap::lock_mark(),
        }
    }

    /// `keys` and the words of `costs` in `width` columns, by a space: the
    /// PIN, the lock's mark, last and kept whole while it fits; the other
    /// words are cut before it, and the keys before them.
    pub(super) fn fitted(
        &self,
        keys: &str,
        costs: Costs,
        width: usize,
        ascii: bool,
    ) -> (String, String) {
        let others = self.of(Costs {
            pin: false,
            ..costs
        });
        let costs = if costs.pin && self.pin.chars().count() <= width {
            let room = width.saturating_sub(self.pin.chars().count() + 1);
            match fit(&others, room, ascii) {
                others if others.is_empty() => self.pin.clone(),
                others => format!("{others} {}", self.pin),
            }
        } else {
            fit(&self.of(costs), width, ascii)
        };
        let room = match costs.chars().count() {
            0 => width,
            used => width.saturating_sub(used + 1),
        };
        (fit(keys, room, ascii), costs)
    }

    /// The words of `costs`, by spaces; empty when it costs nothing.
    pub(super) fn of(&self, costs: Costs) -> String {
        let words = [
            (costs.review, &self.review),
            (costs.writes, &self.writes),
            (costs.key, &self.key),
            (costs.pin, &self.pin),
        ];
        let said: Vec<&str> = (words.iter())
            .filter(|(costs, _)| *costs)
            .map(|(_, word)| word.as_str())
            .collect();
        said.join(" ")
    }
}

/// The name of `section`, as its heading says it.
pub(super) fn section_name(section: Section) -> String {
    match section {
        Section::Recent => text!("section-recent"),
        Section::Actions => text!("section-actions"),
        Section::Items => text!("section-items"),
        Section::Help => text!("section-help"),
    }
}

/// The title of the palette: "Recent" while it shows only the entries
/// chosen before.
pub(super) fn title(palette: &Palette) -> String {
    if palette.recent {
        text!("palette-recent")
    } else {
        text!("palette-title")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::caps::Capabilities;
    use crate::menu::demo::{DemoOptions, Session};
    use crate::menu::model::{Section, Then};
    use crate::menu::theme::Theme;
    use pretty_assertions::assert_eq;

    /// What costs everything.
    const ALL: Costs = Costs {
        pin: true,
        writes: true,
        key: true,
        review: true,
    };

    #[test]
    fn the_mark_is_kept_and_the_keys_and_other_costs_cut_before_it() {
        let words = CostWords::new();
        let all = words.of(ALL);
        let wide = all.chars().count();
        let fitted = |keys: &str, costs, width| words.fitted(keys, costs, width, false);
        // Room for everything.
        assert_eq!(fitted("r", ALL, wide + 2), ("r".to_owned(), all.clone()));
        // The keys go first, then the other costs, never the mark.
        assert_eq!(fitted("r", ALL, wide), (String::new(), all.clone()));
        let (keys, costs) = fitted("r", ALL, 10);
        assert_eq!(keys, "");
        assert!(costs.ends_with(&format!("… {}", words.pin)), "{costs}");
        assert_eq!(costs.chars().count(), 10);
        assert_eq!(fitted("r", ALL, 3), (String::new(), words.pin.clone()));
        // Too narrow for the mark itself.
        assert_eq!(fitted("r", ALL, 2), (String::new(), "r…".to_owned()));
        // Without the mark, the costs are cut from the end, after the keys.
        let open = Costs { pin: false, ..ALL };
        assert_eq!(fitted("Enter, b", open, 8).0, "");
        assert_eq!(fitted("Enter, b", Costs::default(), 6).0, "Enter…");
    }

    #[test]
    fn a_label_with_no_keys_or_costs_takes_the_whole_row() {
        let session = Session::new(&DemoOptions::default(), Theme::new(Capabilities::FULL));
        let item = PaletteItem {
            label: "x".repeat(20),
            also: String::new(),
            section: Section::Actions,
            then: Then::Open(crate::menu::model::Screen::Log),
            keys: String::new(),
            costs: Costs::default(),
        };
        let words = CostWords::new();
        let line = row(session.model(), &item, &[], 20, &words);
        assert_eq!(line.to_string(), "x".repeat(20));
        let keyed = PaletteItem {
            keys: "l".to_owned(),
            ..item
        };
        let line = row(session.model(), &keyed, &[], 20, &words);
        assert_eq!(line.to_string(), format!("{}… l", "x".repeat(17)));
    }
}
