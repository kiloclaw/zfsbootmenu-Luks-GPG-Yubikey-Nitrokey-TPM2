//! The command palette, over the dimmed view: the query, then the best
//! matches of each section, under its heading.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::{HighlightSpacing, List, ListItem, ListState, Paragraph};

use super::dialogs::centred;
use super::lists;
use crate::menu::format::fit;
use crate::menu::model::{Model, Palette, Section};
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
/// `room` columns wide; and the row of the selection, which is never a
/// heading.
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
        let label = fit(&item.label, room, theme.is_ascii());
        let line = Line::from(lists::highlighted(theme, &label, &ranked.positions));
        rows.push(ListItem::new(line));
    }
    (rows, selected)
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
