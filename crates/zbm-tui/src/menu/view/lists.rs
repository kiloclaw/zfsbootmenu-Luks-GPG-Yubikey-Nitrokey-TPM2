//! Grouped lists (`docs/dev/menu-design/apple-hig.md`, rules 3, 5 and 11):
//! a box with the section header and the count in its top border, one
//! column of margin, value rows, the selection highlighted and marked, the
//! search field on top while searching, and an empty state.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, HighlightSpacing, List, ListItem, ListState as WidgetState, Padding, Paragraph,
};

use crate::menu::format::fit;
use crate::menu::model::{ListState, Model};
use crate::menu::theme::{Glyph, Role, Theme};

/// What a list says when it is empty: a title and a sentence.
pub(super) type Empty<'a> = (&'a str, &'a str);

/// A group: a box with `header` in upper case in its top border, and one
/// column of margin inside.
pub(super) fn block(theme: Theme, header: &str) -> Block<'static> {
    Block::bordered()
        .border_set(theme.border())
        .border_style(theme.style(Role::Border))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", header.to_uppercase()),
            theme.style(Role::Header),
        ))
}

/// A group with `count` on the right of its top border.
pub(super) fn counted(theme: Theme, header: &str, count: &str) -> Block<'static> {
    block(theme, header).title_top(
        Line::from(Span::styled(
            format!(" {count} "),
            theme.style(Role::Header),
        ))
        .right_aligned(),
    )
}

/// `text` as spans, with the characters at `positions` in the match style.
pub(super) fn highlighted(theme: Theme, text: &str, positions: &[usize]) -> Vec<Span<'static>> {
    if positions.is_empty() {
        return vec![Span::raw(text.to_owned())];
    }
    text.chars()
        .enumerate()
        .map(|(index, c)| {
            let role = if positions.contains(&index) {
                Role::Match
            } else {
                Role::Text
            };
            Span::styled(c.to_string(), theme.style(role))
        })
        .collect()
}

/// The width a row has in a group drawn in `area`: less the borders, the
/// margins and the selection marker.
pub(super) fn row_width(area: Rect) -> usize {
    usize::from(area.width).saturating_sub(6)
}

/// A list in a group: `rows` in the order of `state.matches` out of `total`,
/// the selection marked, the search field on top while searching, `empty`
/// when there is nothing to show.
pub(super) fn render(
    model: &Model,
    frame: &mut Frame<'_>,
    area: Rect,
    (header, total): (&str, usize),
    state: &ListState,
    rows: Vec<Line<'static>>,
    empty: Empty<'_>,
) {
    let theme = model.theme;
    let searching = state.filtering || !state.filter.is_empty();
    let count = if searching {
        format!("{} of {total}", state.matches.len())
    } else {
        total.to_string()
    };
    let group = counted(theme, header, &count);
    let inner = group.inner(area);
    frame.render_widget(group, area);
    let [search, list_area] =
        Layout::vertical([Constraint::Length(u16::from(searching)), Constraint::Min(1)])
            .areas(inner);
    if searching {
        search_field(model, frame, search, state);
    }
    if rows.is_empty() {
        let quoted = format!(
            "Nothing matches {}. Esc clears the search.",
            quote(theme, &state.filter)
        );
        let shown = if searching {
            ("No Matches", quoted.as_str())
        } else {
            empty
        };
        empty_state(theme, frame, list_area, shown);
        return;
    }
    let items: Vec<ListItem<'static>> = rows.into_iter().map(ListItem::new).collect();
    let list = List::new(items)
        .highlight_symbol(Line::from(format!("{} ", theme.glyph(Glyph::Selected))))
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_style(theme.style(Role::Selected));
    let mut widget = WidgetState::default().with_selected(Some(state.selected));
    frame.render_stateful_widget(list, list_area, &mut widget);
}

/// `text` in quotation marks.
fn quote(theme: Theme, text: &str) -> String {
    if theme.is_ascii() {
        format!("\"{text}\"")
    } else {
        format!("“{text}”")
    }
}

/// The search field: `/` and the query, `Esc Cancel` on the right.
fn search_field(model: &Model, frame: &mut Frame<'_>, area: Rect, state: &ListState) {
    let theme = model.theme;
    let query = format!("/ {}", state.filter);
    let hint = "Esc Cancel";
    let gap = usize::from(area.width).saturating_sub(query.chars().count() + hint.len());
    let line = Line::from(vec![
        Span::styled(query.clone(), theme.style(Role::Text)),
        Span::raw(" ".repeat(gap)),
        Span::styled("Esc", theme.style(Role::Key)),
        Span::styled(" Cancel", theme.style(Role::Muted)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
    if state.filtering {
        let column = u16::try_from(query.chars().count()).unwrap_or(u16::MAX);
        frame.set_cursor_position(Position::new(area.x.saturating_add(column), area.y));
    }
}

/// An empty state: the title and the sentence, centred.
pub(super) fn empty_state(
    theme: Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    (title, text): Empty<'_>,
) {
    let width = usize::from(area.width);
    let lines = vec![
        Line::styled(
            fit(title, width, theme.is_ascii()),
            theme.style(Role::Title),
        )
        .centered(),
        Line::styled(text.to_owned(), theme.style(Role::Muted)).centered(),
    ];
    let height = u16::try_from(lines.len() + 1).unwrap_or(u16::MAX);
    let [middle] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let paragraph = Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(paragraph, middle);
}

/// A value row: a name with its matches on the left, `facts` right-aligned
/// in a row `width` columns wide.
pub(super) fn row(
    theme: Theme,
    name: &str,
    positions: &[usize],
    width: usize,
    facts: Vec<Span<'static>>,
) -> Line<'static> {
    let mut spans = highlighted(theme, name, positions);
    let used = name.chars().count() + facts.iter().map(Span::width).sum::<usize>();
    if !facts.is_empty() {
        spans.push(Span::raw(" ".repeat(width.saturating_sub(used).max(2))));
        spans.extend(facts);
    }
    Line::from(spans)
}

/// `line` cut to `width` columns, with a mark where it was cut.
pub(super) fn clip(line: Line<'static>, width: usize, ascii: bool) -> Line<'static> {
    if line.width() <= width {
        return line;
    }
    let mut room = width.saturating_sub(1);
    let mut spans = Vec::new();
    for span in line.spans {
        let length = span.content.chars().count();
        if length <= room {
            room -= length;
            spans.push(span);
            continue;
        }
        let kept: String = span.content.chars().take(room).collect();
        spans.push(Span::styled(kept, span.style));
        break;
    }
    spans.push(Span::raw(if ascii { "~" } else { "…" }));
    Line::from(spans)
}
