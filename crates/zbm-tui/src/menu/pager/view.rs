//! Drawing the pager: its topic in the top border and the lines shown on
//! its right, the filter's field on top while it is typed or kept, the
//! lines wrapped to the width in columns with their control characters
//! escaped, and the toolbar of its keys at the bottom.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use zbm_types::Printable;

use super::Reading;
use crate::menu::keymap::{self, Context, Intent, Map, describe};
use crate::menu::theme::{Role, Theme};
use crate::menu::view::{lists, toolbar};
use crate::text::{self, id, text};

/// Draws `reading` in `theme`, its toolbar with the keys of the key map
/// `map`; the area its lines have, which the moves measure.
pub(super) fn draw(
    frame: &mut Frame<'_>,
    reading: &Reading<'_>,
    (theme, map): (Theme, Map),
) -> Rect {
    let area = frame.area();
    frame.buffer_mut().set_style(area, theme.style(Role::Text));
    let [body, bottom] = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(area);
    let title = reading.pager.topic.title();
    let inner = lists::block(theme, &title).inner(body);
    let searching = reading.typing || !reading.filter.is_empty();
    let [search, text] =
        Layout::vertical([Constraint::Length(u16::from(searching)), Constraint::Min(1)])
            .areas(inner);
    let kept = reading.kept();
    let scroll = reading.scroll.min(kept.len().saturating_sub(1));
    let (rows, shown) = rows(kept.get(scroll..).unwrap_or_default(), text);
    let (from, to) = if shown == 0 {
        (0, 0)
    } else {
        (scroll + 1, scroll + shown)
    };
    let position = text!("pager-position", from = from, to = to, count = kept.len());
    frame.render_widget(lists::counted(theme, &title, &position), body);
    if searching {
        search_field(frame, search, reading, theme);
    }
    if kept.is_empty() && searching {
        let filter = Printable(&reading.filter).to_string();
        let quoted = if theme.is_ascii() {
            text!("quoted-ascii", text = filter)
        } else {
            text!("quoted", text = filter)
        };
        let why = text!("pager-no-match", filter = quoted);
        lists::empty_state(theme, frame, text, (&text!("list-no-matches"), &why));
    } else {
        frame.render_widget(Paragraph::new(rows), text);
    }
    footer(frame, bottom, reading.context(), (theme, map));
    text
}

/// The rows of `lines` in `area`: each line printable, wrapped to the
/// width, as many as fit; and how many lines they show, the last perhaps
/// in part.
fn rows(lines: &[&str], area: Rect) -> (Vec<Line<'static>>, usize) {
    let width = usize::from(area.width);
    let height = usize::from(area.height);
    let mut rows = Vec::new();
    let mut shown = 0;
    for line in lines {
        if rows.len() >= height {
            break;
        }
        shown += 1;
        let parts = wrapped(line, width);
        rows.extend(parts.into_iter().take(height - rows.len()).map(Line::raw));
    }
    (rows, shown)
}

/// How many rows `line` takes `width` columns wide.
pub(super) fn height(line: &str, width: usize) -> usize {
    wrapped(line, width).len()
}

/// `line` printable in rows of `width` columns at most, a wide character
/// counting two: a row breaks before the character that would not fit,
/// so none is cut. One empty row for an empty line.
fn wrapped(line: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let mut row = String::new();
    let mut used = 0;
    for c in Printable(line).to_string().chars() {
        let columns = Span::raw(&*c.encode_utf8(&mut [0; 4])).width();
        if used > 0 && used + columns > width {
            rows.push(std::mem::take(&mut row));
            used = 0;
        }
        row.push(c);
        used += columns;
    }
    rows.push(row);
    rows
}

/// The filter's field: `/` and what it keeps, and on the right the key
/// that clears it; the cursor after the filter while it is typed.
fn search_field(frame: &mut Frame<'_>, area: Rect, reading: &Reading<'_>, theme: Theme) {
    let query = format!("/ {}", Printable(&reading.filter));
    let key = text!("key-esc");
    let clear = format!(" {}", text::title(id!("pager-filter")));
    let hint = key.chars().count() + clear.chars().count();
    let gap = usize::from(area.width).saturating_sub(query.chars().count() + hint);
    let line = Line::from(vec![
        Span::styled(query.clone(), theme.style(Role::Text)),
        Span::raw(" ".repeat(gap)),
        Span::styled(key, theme.style(Role::Key)),
        Span::styled(clear, theme.style(Role::Muted)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
    if reading.typing {
        let column = u16::try_from(query.chars().count()).unwrap_or(u16::MAX);
        frame.set_cursor_position(Position::new(area.x.saturating_add(column), area.y));
    }
}

/// The toolbar of `context`, the pager or its filter's field, in the key
/// map `map`: the moves scroll in both, and help, which does nothing in
/// the pager, is not offered.
fn footer(frame: &mut Frame<'_>, area: Rect, context: Context, (theme, map): (Theme, Map)) {
    let width = usize::from(area.width).saturating_sub(2);
    let offered = |intent| intent != Intent::Help;
    let said = |intent| match intent {
        Intent::Up | Intent::Down => describe(Context::Pager, intent),
        _ => describe(context, intent),
    };
    let tools = keymap::footer(map, context, width, &offered, &said, &|_| false);
    frame.render_widget(Paragraph::new(toolbar(theme, tools, width)), area);
}
