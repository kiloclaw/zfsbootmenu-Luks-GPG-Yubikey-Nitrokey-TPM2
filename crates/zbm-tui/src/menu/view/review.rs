//! The review page of a change (ADR 0016, points 12 to 15), over the
//! whole view in the danger colour: its title; what stops it, first, in
//! place of the field; what it loses, which scrolls, its sizes
//! right-aligned under their total; the space it frees and what it keeps;
//! its steps, their commands muted; its undo, or none; the safer action;
//! who else may use the pool it imports; then the field of the name and
//! the buttons, at the bottom. Text from the machine is drawn printable.

mod steps;
#[cfg(test)]
mod tests;

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use zbm_types::{ImportPolicy, Printable};

use self::steps::{steps, stop};
use super::dialogs::{Kind, buttons, frame_block, verb, wrapped};
use super::lists;
use super::words::Said;
use crate::menu::format::{age, date, fit, size};
use crate::menu::model::{Focus, Lost, Model, Reviewing, host_text};
use crate::menu::theme::{Glyph, Role, Theme};
use crate::menu::world::{Action, Keep, Outcome, PoolUse, Safer};
use crate::text::text;

/// How many rows of what is lost the line view says, before how many
/// more there are.
const SAID: usize = 20;

/// A line of the page below what is lost: its text, and how it is drawn.
pub(crate) struct Part {
    /// The text, the names in it printable.
    pub(crate) text: String,
    /// How it is drawn.
    role: Role,
}

/// A [`Part`] of `text` drawn as `role`.
fn part(text: String, role: Role) -> Part {
    Part { text, role }
}

/// The title of the page of `reviewing`.
pub(crate) fn title(reviewing: &Reviewing) -> String {
    match &reviewing.action {
        Action::Rollback(snapshot) => text!(
            "review-rollback",
            environment = snapshot.dataset().to_string(),
            snapshot = snapshot.name()
        ),
        Action::Import(pool, options) if options.rewind => {
            text!("rewind.title", pool = pool.to_string())
        }
        Action::Import(pool, options) if options.policy == ImportPolicy::Force => {
            text!("force-import.title", pool = pool.to_string())
        }
        _ => text!("review-title"),
    }
}

/// Why the plan refuses the change: what stops it, as ZFS would refuse
/// it, else the step that would fail; nothing when it refuses nothing.
pub(crate) fn refusal(reviewing: &Reviewing) -> Vec<String> {
    let Some(plan) = reviewing.plan.as_ref().filter(|_| reviewing.is_refused()) else {
        return Vec::new();
    };
    if !plan.review.stops.is_empty() {
        return plan.review.stops.iter().map(stop).collect();
    }
    match &plan.outcome {
        Outcome::Failed { what, why, .. } => std::iter::once(what)
            .chain(why)
            .map(|line| Printable(line).to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// What the page asks, first: to wait for the plan; why it refuses the
/// change; or the name typed.
pub(crate) fn asks(reviewing: &Reviewing) -> Vec<String> {
    if reviewing.plan.is_none() {
        return vec![text!("review-planning")];
    }
    if reviewing.is_refused() {
        let mut lines = vec![text!("review-refused")];
        lines.extend(refusal(reviewing));
        return lines;
    }
    vec![text!("confirm-type", name = reviewing.name.as_str())]
}

/// Who else may use the pool a change imports.
fn pool_use(model: &Model, pool_use: &PoolUse) -> Vec<Part> {
    let mut parts = vec![part(text!("review-pool-use"), Role::Header)];
    if pool_use.this_host {
        parts.push(part(text!("review-this-host"), Role::Text));
    } else if let Some(host) = &pool_use.host {
        parts.push(part(
            text!("review-host", host = host_text(host)),
            Role::Text,
        ));
    } else {
        parts.push(part(text!("review-host-unknown"), Role::Warning));
    }
    if let Some(written) = pool_use.written {
        let when = text!(
            "review-written",
            age = age(model.clock(), written),
            date = date(written)
        );
        parts.push(part(when, Role::Text));
    }
    if pool_use.active {
        parts.push(part(text!("review-active"), Role::Danger));
    }
    parts
}

/// What the page says below what is lost: the space freed, what is kept,
/// the steps, the undo, the safer action, and who else may use the pool;
/// nothing before the plan.
pub(crate) fn after(model: &Model, reviewing: &Reviewing) -> Vec<Part> {
    let Some(plan) = &reviewing.plan else {
        return Vec::new();
    };
    let review = &plan.review;
    let mut parts = Vec::new();
    if let Some(freed) = review.freed.filter(|_| !review.loses.is_empty()) {
        parts.push(part(text!("review-freed", size = size(freed)), Role::Text));
    }
    let kept: Vec<String> = review
        .keeps
        .iter()
        .map(|keep| match keep {
            Keep::Snapshots(count) => text!("review-kept-snapshots", count = *count),
            Keep::Bookmarks(count) => text!("review-kept-bookmarks", count = *count),
        })
        .collect();
    if !kept.is_empty() {
        parts.push(part(
            text!("review-kept", kept = kept.join(", ")),
            Role::Text,
        ));
    }
    if !plan.steps.is_empty() {
        parts.push(part(text!("review-steps"), Role::Header));
        parts.extend(steps(&plan.steps));
    }
    parts.push(match review.undo.as_deref() {
        Some(Action::Export(pool)) => part(
            text!("review-undo-export", pool = pool.to_string()),
            Role::Text,
        ),
        Some(_) => part(text!("review-undo"), Role::Text),
        None => part(text!("review-no-undo"), Role::Warning),
    });
    match reviewing.safer(&model.snapshot.offers) {
        Some(Safer::Clone(snapshot)) => parts.push(part(
            text!("review-safer-clone", snapshot = snapshot.name()),
            Role::Good,
        )),
        Some(Safer::Import(_)) => parts.push(part(text!("review-safer-import"), Role::Good)),
        None => {}
    }
    parts.extend(
        review
            .pool_use
            .iter()
            .flat_map(|pool_use| self::pool_use(model, pool_use)),
    );
    parts
}

/// A row of what is lost in words: what, then its size when known.
fn lost_words(row: &Lost) -> String {
    match row.bytes {
        Some(bytes) => format!("{}, {}", row.what, size(bytes)),
        None => row.what.clone(),
    }
}

/// The page in words, for the line view and `Announce`: its title, what it
/// asks, the first [`SAID`] rows of what is lost and how many more, then
/// the rest.
pub(crate) fn said(model: &Model, reviewing: &Reviewing) -> Said {
    let mut lines = asks(reviewing);
    let lost = reviewing.lost();
    if reviewing.plan.is_some() {
        lines.push(text!("review-lost"));
        if lost.is_empty() {
            lines.push(text!("review-lost-none"));
        }
    }
    lines.extend(lost.iter().take(SAID).map(lost_words));
    if lost.len() > SAID {
        lines.push(text!("review-more", count = lost.len() - SAID));
    }
    lines.extend(after(model, reviewing).into_iter().map(|part| part.text));
    Said {
        title: title(reviewing),
        lines,
        choices: Vec::new(),
    }
}

/// The rows of `area`: the title, at the top; what the page asks first,
/// `head` rows; the middle; the field, `field` rows; and the buttons, at
/// the bottom. The title and the buttons always have their row, then the
/// field, then what the page asks.
fn split(area: Rect, head: u16, field: u16) -> [Rect; 5] {
    let row = |y: u16, height: u16| Rect::new(area.x, y, area.width, height);
    let bottom = area.y.saturating_add(area.height);
    let buttons = row(bottom.saturating_sub(1), u16::from(area.height > 1));
    let room = area.height.saturating_sub(2);
    let field = field.min(room);
    let field_area = row(buttons.y.saturating_sub(field), field);
    let head = head.min(room - field);
    let head_area = row(area.y.saturating_add(1), head);
    let middle_y = head_area.y.saturating_add(head);
    let middle = row(middle_y, field_area.y.saturating_sub(middle_y));
    [row(area.y, 1), head_area, middle, field_area, buttons]
}

/// Draws the page of `reviewing` over `area`, the whole view.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect, reviewing: &Reviewing) {
    let theme = model.theme;
    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.style(Role::Text));
    let block = frame_block(theme, true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let asked = asked_line(theme, reviewing);
    let field = if reviewing.has_field() {
        wrapped(std::slice::from_ref(&asked), inner.width).saturating_add(1)
    } else {
        0
    };
    // What the page asks first takes at most half the room the field
    // leaves, so that what is lost and the rest show below it.
    let room = inner.height.saturating_sub(2).saturating_sub(field);
    let head = head_lines(theme, reviewing, inner.width, room - room / 2);
    let height = wrapped(&head, inner.width);
    let [title_row, head_area, middle, field_area, button_row] = split(inner, height, field);
    let width = usize::from(inner.width);
    let heading = fit(&title(reviewing), width, theme.is_ascii());
    frame.render_widget(
        Paragraph::new(Line::styled(heading, theme.style(Role::Title)).centered()),
        title_row,
    );
    frame.render_widget(Paragraph::new(head).wrap(Wrap { trim: false }), head_area);
    render_middle(model, frame, middle, reviewing);
    if reviewing.has_field() {
        render_field(theme, frame, field_area, reviewing, asked);
    }
    let line = button_line(model, reviewing, width);
    frame.render_widget(Paragraph::new(line), button_row);
}

/// What the page asks first, as lines: to wait for the plan, or why it
/// refuses the change; nothing once it asks for the name. When they take
/// more than `rows` rows wrapped in `width` columns, the last row says
/// how many more there are.
fn head_lines(theme: Theme, reviewing: &Reviewing, width: u16, rows: u16) -> Vec<Line<'static>> {
    if reviewing.plan.is_some() && !reviewing.is_refused() {
        return Vec::new();
    }
    let mut lines: Vec<Line<'static>> = asks(reviewing)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let role = match (reviewing.plan.is_some(), index) {
                (false, _) => Role::Accent,
                (true, 0) => Role::Danger,
                (true, _) => Role::Text,
            };
            Line::styled(line, theme.style(role))
        })
        .collect();
    let all = lines.len();
    let more = |count: usize| {
        Line::styled(
            text!("review-more", count = count),
            theme.style(Role::Muted),
        )
    };
    if wrapped(&lines, width) <= rows {
        return lines;
    }
    while lines.len() > 1 {
        lines.pop();
        let mut shown = lines.clone();
        shown.push(more(all - lines.len()));
        if wrapped(&shown, width) <= rows {
            return shown;
        }
    }
    lines
}

/// The middle of the page: what is lost, under its header, from the row
/// scrolled to, which takes the room the rest leaves it; then the rest,
/// below it, each line wrapped while the rows of what is lost leave room,
/// else cut to the width.
fn render_middle(model: &Model, frame: &mut Frame<'_>, area: Rect, reviewing: &Reviewing) {
    if reviewing.plan.is_none() || area.height == 0 {
        return;
    }
    let theme = model.theme;
    let ascii = theme.is_ascii();
    let width = usize::from(area.width);
    let lost = reviewing.lost();
    let (after, rest) = rest_lines(model, reviewing, area, lost.len());
    let room = usize::from(area.height.saturating_sub(rest))
        .saturating_sub(1)
        .max(1);
    let first = reviewing.scroll.min(lost.len().saturating_sub(1));
    let shown: Vec<&Lost> = lost.iter().skip(first).take(room).collect();
    let mut lines = vec![lost_header(theme, &lost, (first, shown.len()), width)];
    if lost.is_empty() {
        lines.push(Line::from(fit(&text!("review-lost-none"), width, ascii)));
    }
    lines.extend(shown.iter().map(|row| {
        let bytes = row.bytes.map(size).unwrap_or_default();
        let room = width.saturating_sub(bytes.chars().count() + 2);
        let facts = vec![Span::styled(bytes, theme.style(Role::Muted))];
        lists::row(theme, &fit(&row.what, room, ascii), &[], width, facts)
    }));
    let top = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .min(area.height);
    frame.render_widget(
        Paragraph::new(lines),
        Rect {
            height: top,
            ..area
        },
    );
    let below = Rect {
        y: area.y.saturating_add(top),
        height: area.height - top,
        ..area
    };
    frame.render_widget(Paragraph::new(after).wrap(Wrap { trim: false }), below);
}

/// The lines below what is lost, and the rows they take in `area`: each
/// line wrapped, in order, while the `lost` rows of what is lost and the
/// lines after it keep a row each; else cut to the width.
fn rest_lines(
    model: &Model,
    reviewing: &Reviewing,
    area: Rect,
    lost: usize,
) -> (Vec<Line<'static>>, u16) {
    let theme = model.theme;
    let width = usize::from(area.width);
    let parts = after(model, reviewing);
    let ones = u16::try_from(parts.len()).unwrap_or(u16::MAX);
    let mut rest = ones.min(area.height.saturating_sub(2));
    let lost = u16::try_from(lost.max(1)).unwrap_or(u16::MAX);
    let mut spare = area
        .height
        .saturating_sub(rest)
        .saturating_sub(lost.saturating_add(1));
    let lines = parts
        .into_iter()
        .map(|part| {
            let style = theme.style(part.role);
            let whole = Line::styled(part.text.clone(), style);
            let more = wrapped(std::slice::from_ref(&whole), area.width).saturating_sub(1);
            if more <= spare {
                spare -= more;
                rest = rest.saturating_add(more);
                whole
            } else {
                Line::styled(fit(&part.text, width, theme.is_ascii()), style)
            }
        })
        .collect();
    (lines, rest)
}

/// The header of what is lost: which rows show, when not all do, and the
/// total of the sizes known, right-aligned over them.
fn lost_header(theme: Theme, lost: &[Lost], shown: (usize, usize), width: usize) -> Line<'static> {
    let (first, count) = shown;
    let mut header = text!("review-lost");
    if count < lost.len() {
        let rows = text!(
            "review-lost-rows",
            first = first + 1,
            last = first + count,
            count = lost.len()
        );
        header = format!("{header} {rows}");
    }
    let total = lost
        .iter()
        .filter_map(|row| row.bytes)
        .fold(0, u64::saturating_add);
    let facts = if lost.iter().any(|row| row.bytes.is_some()) {
        let total = text!("review-total", size = size(total));
        vec![Span::styled(total, theme.style(Role::Title))]
    } else {
        Vec::new()
    };
    let mut line = lists::row(theme, &header, &[], width, facts);
    if let Some(first) = line.spans.first_mut() {
        first.style = theme.style(Role::Header);
    }
    line
}

/// The sentence that names what to type, the name in the danger colour.
fn asked_line(theme: Theme, reviewing: &Reviewing) -> Line<'static> {
    let sentence = text!("confirm-type", name = "\u{1}");
    let (before, after) = sentence.split_once('\u{1}').unwrap_or((&sentence, ""));
    Line::from(vec![
        Span::raw(before.to_owned()),
        Span::styled(
            Printable(&reviewing.name).to_string(),
            theme.style(Role::Danger),
        ),
        Span::raw(after.to_owned()),
    ])
}

/// The field: `asked`, the sentence that names what to type, wrapped so
/// that the name shows whole, then what was typed, its end shown when it
/// is longer than the field, and the cursor there while the field has the
/// focus.
fn render_field(
    theme: Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    reviewing: &Reviewing,
    asked: Line<'static>,
) {
    let width = usize::from(area.width);
    let typed = reviewing.typed.as_deref().unwrap_or_default();
    let prompt = format!("{} ", theme.glyph(Glyph::Selected));
    let room = width.saturating_sub(prompt.chars().count() + 1);
    let skip = typed.chars().count().saturating_sub(room);
    let shown: String = typed.chars().skip(skip).collect();
    let column = prompt.chars().count() + shown.chars().count();
    let line = Line::from(vec![
        Span::styled(prompt, theme.style(Role::Accent)),
        Span::styled(shown, theme.style(Role::Title)),
    ]);
    let last = area.y.saturating_add(area.height.saturating_sub(1));
    let sentence = Rect {
        height: area.height.saturating_sub(1),
        ..area
    };
    let asked = Paragraph::new(asked).wrap(Wrap { trim: false });
    frame.render_widget(asked, sentence);
    frame.render_widget(
        Paragraph::new(line),
        Rect {
            y: last,
            height: 1,
            ..area
        },
    );
    if reviewing.focus == Focus::Field && area.height > 1 {
        let column = u16::try_from(column).unwrap_or(u16::MAX);
        frame.set_cursor_position(Position::new(area.x.saturating_add(column), last));
    }
}

/// The buttons: **OK** on a page that refuses the change; else
/// **Cancel**, the safer action, and the change, disabled until it is
/// planned and named; the one `Enter` presses shows it, and the change
/// shows the focus even disabled. **Cancel**, which `Esc` presses, goes
/// first when they do not fit in `width`.
fn button_line(model: &Model, reviewing: &Reviewing, width: usize) -> Line<'static> {
    let theme = model.theme;
    if reviewing.is_refused() {
        return buttons(theme, &[(text!("button-ok"), Kind::Default)]);
    }
    let mut all = vec![(text!("button-cancel"), Kind::Normal)];
    if let Some(safer) = reviewing.safer(&model.snapshot.offers) {
        let label = match safer {
            Safer::Clone(_) => text!("review-button-clone"),
            Safer::Import(_) => text!("review-button-read-only"),
        };
        let more = theme.glyph(Glyph::Ellipsis);
        let label = match safer {
            Safer::Clone(_) => format!("{label}{more}"),
            Safer::Import(_) => label,
        };
        let kind = if reviewing.focus == Focus::Safer {
            Kind::Default
        } else {
            Kind::Normal
        };
        all.push((label, kind));
    }
    // `Enter` presses the change from the field too, once it is ready.
    let kind = match (reviewing.is_ready(), reviewing.focus) {
        (true, Focus::Safer) => Kind::Normal,
        (true, _) => Kind::Destructive,
        (false, Focus::Verb) => Kind::Waiting,
        (false, _) => Kind::Disabled,
    };
    all.push((verb(&reviewing.action), kind));
    let used: usize = all.iter().map(|(label, _)| label.chars().count() + 6).sum();
    if used > width + 2 {
        all.remove(0);
    }
    buttons(theme, &all)
}
