//! A device's kernel messages in the workspace's Pools tab (ADR 0016,
//! point 5): a panel below the tree, the newest that fit, each on a line
//! of its own and drawn printable, as the kernel's text is the devices';
//! or why there are none. Messages that name only a port are never among
//! them, which the panel's last line says.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use zbm_types::Printable;

use super::lists;
use crate::menu::format::fit;
use crate::menu::model::{Model, disk_name};
use crate::menu::theme::Role;
use crate::menu::world::DeviceLog;
use crate::text::text;

/// The fewest rows the panel takes, with its borders.
pub(super) const LEAST: u16 = 5;

/// Draws the panel of `log` in `area`.
pub(super) fn render(model: &Model, log: &DeviceLog, frame: &mut Frame<'_>, panel: Rect) {
    let theme = model.theme;
    // No selection marker: the borders and the margins alone.
    let width = usize::from(panel.width.saturating_sub(4));
    let room = usize::from(panel.height.saturating_sub(3));
    let mut lines: Vec<Line<'static>> = said(log)
        .into_iter()
        .map(|said| match said {
            Said::Note(note) => muted(model, note),
            Said::Line(line) => {
                Line::styled(fit(&line, width, theme.is_ascii()), theme.style(Role::Text))
            }
        })
        .collect();
    let newest = lines.len().saturating_sub(room);
    lines.drain(..newest);
    let ports = fit(&text!("device-log-ports"), width, theme.is_ascii());
    lines.push(muted(model, ports));
    let block = lists::counted(theme, &title(log), &log.lines.len().to_string());
    frame.render_widget(Paragraph::new(lines).block(block), panel);
}

/// The title of the panel of `log`: the device's name.
pub(super) fn title(log: &DeviceLog) -> String {
    text!("device-log-title", device = disk_name(log.device.get()))
}

/// A line the panel says.
pub(super) enum Said {
    /// Why there is no message.
    Note(String),
    /// A message of the kernel, drawn printable.
    Line(String),
}

/// What the panel of `log` says, oldest first: its messages, or why there
/// are none.
pub(super) fn said(log: &DeviceLog) -> Vec<Said> {
    if log.names.is_empty() {
        let device = Printable(log.device.get()).to_string();
        return vec![Said::Note(text!("device-log-unknown", device = device))];
    }
    if log.lines.is_empty() {
        let names: Vec<String> = log
            .names
            .iter()
            .map(|n| n.printable().to_string())
            .collect();
        return vec![Said::Note(text!(
            "device-log-none",
            names = names.join(", ")
        ))];
    }
    log.lines
        .iter()
        .map(|line| Said::Line(line.printable().to_string()))
        .collect()
}

/// `text` as a muted line.
fn muted(model: &Model, text: String) -> Line<'static> {
    Line::styled(text, model.theme.style(Role::Muted))
}
