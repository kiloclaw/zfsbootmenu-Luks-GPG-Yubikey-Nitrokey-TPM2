//! The columns of the Boot tab (ADR 0007, decisions 2 and 8): the boot
//! environments grouped by pool, then the kernels, of the environment
//! open or else of the one selected, and the inspector of the deepest
//! column. Beside the sidebar, from 120 columns, the inspector is to their
//! right; narrower it is below them, under both columns from 80 columns,
//! and under the deepest column alone at 52.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use zbm_types::PoolName;

use super::super::{lists, main_view, screens};
use crate::menu::format::fit;
use crate::menu::model::{Model, Screen};
use crate::menu::theme::Role;
use crate::text::text;

/// From this width the environments and the kernels show side by side;
/// narrower, the deepest column alone.
const TWO_FROM: u16 = 80;

/// The width of the kernels column beside the environments.
const KERNELS: u16 = 28;

/// From this width, beside the sidebar, the inspector shows to the right
/// of the columns, the environments as wide as at 80 columns; narrower it
/// shows below them.
const BESIDE_FROM: u16 = 94;

/// From this height, the path's row included, the inspector below the
/// columns takes more lines.
const TALL: u16 = 17;

/// Draws the columns and the inspector in `area`, below the path: the
/// inspector beside them when `beside` and there is room, else below them.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect, beside: bool) {
    if beside && area.width >= BESIDE_FROM {
        let [environments, kernels, inspector] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(KERNELS),
            Constraint::Fill(1),
        ])
        .areas(area);
        render_environments(model, frame, environments, false);
        render_kernels(model, frame, kernels);
        render_inspector(model, frame, inspector);
        return;
    }
    let tall = area.height + super::path::height(model) >= TALL;
    let lines = if tall { 9 } else { 5 };
    let [columns, inspector] =
        Layout::vertical([Constraint::Min(4), Constraint::Length(lines)]).areas(area);
    if columns.width >= TWO_FROM {
        let [environments, kernels] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(KERNELS)]).areas(columns);
        render_environments(model, frame, environments, false);
        render_kernels(model, frame, kernels);
    } else if model.screen == Screen::Kernels {
        render_kernels(model, frame, columns);
    } else {
        render_environments(model, frame, columns, true);
    }
    render_inspector(model, frame, inspector);
}

/// The inspector of the deepest column: the kernel selected once the
/// kernels are open, else the environment.
fn render_inspector(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    if model.screen == Screen::Kernels {
        main_view::render_kernel_detail(model, frame, area);
    } else {
        main_view::render_detail(model, frame, area);
    }
}

/// The boot environments, under a header for each pool, which is not an
/// item: the selection moves from environment to environment. Each says
/// the kernel that boots only `with_kernel`, as the kernels column does
/// when it shows beside.
fn render_environments(model: &Model, frame: &mut Frame<'_>, area: Rect, with_kernel: bool) {
    let theme = model.theme;
    let environments = &model.snapshot.environments;
    let width = lists::row_width(area);
    let mut rows = Vec::new();
    let mut selected = None;
    for (position, ranked) in model.environments.matches.iter().enumerate() {
        let Some(environment) = environments.get(ranked.index) else {
            continue;
        };
        if let Some(pool) = model.group_at(position) {
            rows.push(pool_header(model, &pool));
        }
        if position == model.environments.selected {
            selected = Some(rows.len());
        }
        let facts = if with_kernel {
            main_view::facts(model, environment)
        } else {
            trimmed(main_view::labels(model, environment))
        };
        let room = width.saturating_sub(facts.iter().map(Span::width).sum::<usize>() + 2);
        let name = fit(environment.name.as_str(), room, theme.is_ascii());
        rows.push(lists::row(theme, &name, &ranked.positions, width, facts));
    }
    let (title, hint) = (text!("environments-none"), text!("environments-none.text"));
    let empty = (title.as_str(), hint.as_str());
    let header = text!("view-main");
    let total = (header.as_str(), environments.len());
    let rows = (selected, rows);
    lists::render_rows(model, frame, area, total, &model.environments, rows, empty);
}

/// `labels` without the blanks after the last, which ends the row.
fn trimmed(mut labels: Vec<Span<'static>>) -> Vec<Span<'static>> {
    if let Some(last) = labels.last_mut() {
        *last = Span::styled(last.content.trim_end().to_owned(), last.style);
    }
    labels
}

/// The header of the environments of `pool`: its name, and its health
/// when it is imported.
fn pool_header(model: &Model, pool: &PoolName) -> Line<'static> {
    let theme = model.theme;
    let health = model
        .snapshot
        .pools
        .iter()
        .find(|known| known.imported && &known.name == pool)
        .map(|known| format!("{}{}", theme.dot(), known.health.as_str()));
    let header = format!("{pool}{}", health.unwrap_or_default());
    Line::styled(header, theme.style(Role::Header))
}

/// The kernels: those of the environment open, with its selection, once
/// they are; else those of the environment selected, none of them
/// selected.
fn render_kernels(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    if model.screen == Screen::Kernels {
        screens::kernels(model, frame, area);
        return;
    }
    let width = lists::row_width(area);
    let kernels = model
        .selected_environment()
        .map(|environment| environment.kernels.as_slice())
        .unwrap_or_default();
    let rows = kernels
        .iter()
        .map(|kernel| screens::kernel_row(model, kernel, &[], width))
        .collect();
    let (title, hint) = (text!("kernels-none"), text!("kernels-none.text"));
    let empty = (title.as_str(), hint.as_str());
    let header = text!("kernels-title");
    let total = (header.as_str(), kernels.len());
    let unfiltered = crate::menu::model::ListState::default();
    lists::render_rows(model, frame, area, total, &unfiltered, (None, rows), empty);
}
