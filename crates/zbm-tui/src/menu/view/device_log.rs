//! A device's kernel messages in the workspace's Pools tab (ADR 0016,
//! point 5): below the pools, the newest that fit, each on a line of its
//! own and drawn printable, as the kernel's text is the devices'; or why
//! there are none. Messages that name only a port are never among them,
//! which the panel's last line says. The workspace's device column, when
//! it comes, takes its place.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use zbm_types::Printable;

use super::{lists, screens};
use crate::menu::format::fit;
use crate::menu::model::{Model, disk_name};
use crate::menu::theme::Role;
use crate::menu::world::DeviceLog;
use crate::text::text;

/// The fewest rows the panel takes, with its borders.
const LEAST: u16 = 5;

/// Draws the pools, and `log` below them.
pub(super) fn render(model: &Model, log: &DeviceLog, frame: &mut Frame<'_>, area: Rect) {
    let rows = (area.height / 2).max(LEAST);
    let [list, panel] =
        Layout::vertical([Constraint::Min(4), Constraint::Length(rows)]).areas(area);
    screens::pools(model, frame, list);
    let theme = model.theme;
    // No selection marker: the borders and the margins alone.
    let width = usize::from(panel.width.saturating_sub(4));
    let room = usize::from(panel.height.saturating_sub(3));
    let mut lines: Vec<Line<'static>> = if log.names.is_empty() {
        let device = Printable(log.device.get()).to_string();
        vec![muted(model, text!("device-log-unknown", device = device))]
    } else if log.lines.is_empty() {
        let names: Vec<String> = log
            .names
            .iter()
            .map(|n| n.printable().to_string())
            .collect();
        vec![muted(
            model,
            text!("device-log-none", names = names.join(", ")),
        )]
    } else {
        let newest = log.lines.len().saturating_sub(room);
        log.lines
            .iter()
            .skip(newest)
            .map(|line| {
                let shown = fit(&line.printable().to_string(), width, theme.is_ascii());
                Line::styled(shown, theme.style(Role::Text))
            })
            .collect()
    };
    lines.truncate(room);
    let ports = fit(&text!("device-log-ports"), width, theme.is_ascii());
    lines.push(muted(model, ports));
    let title = text!("device-log-title", device = disk_name(log.device.get()));
    let block = lists::counted(theme, &title, &log.lines.len().to_string());
    frame.render_widget(Paragraph::new(lines).block(block), panel);
}

/// `text` as a muted line.
fn muted(model: &Model, text: String) -> Line<'static> {
    Line::styled(text, model.theme.style(Role::Muted))
}
