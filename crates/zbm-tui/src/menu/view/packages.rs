//! The packages segment of the changes since a snapshot
//! (`docs/dev/menu-design/boot-health.md`): each package with what happened
//! to it and its versions, and a preview with its changed files.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::lists;
use crate::menu::format::{capitalised, fit};
use crate::menu::model::{DiffView, Model};
use crate::menu::theme::{Role, Theme};
use crate::menu::world::{PackageChange, PackageKind};

/// The style of what happened to a package.
fn role(kind: PackageKind) -> Role {
    match kind {
        PackageKind::Removed => Role::Danger,
        PackageKind::Installed => Role::Good,
        PackageKind::Upgraded => Role::Accent,
        PackageKind::Downgraded => Role::Warning,
    }
}

/// The packages: a mark, the name, the versions right-aligned.
pub(super) fn render_list(model: &Model, frame: &mut Frame<'_>, area: Rect, diff: &DiffView) {
    let theme = model.theme;
    let ascii = theme.is_ascii();
    let width = lists::row_width(area).saturating_sub(2);
    let packages = diff.package_changes();
    let rows = diff
        .package_list
        .matches
        .iter()
        .filter_map(|ranked| {
            let package = packages.get(ranked.index)?;
            let versions = package.versions(ascii);
            let facts = vec![Span::styled(versions.clone(), theme.style(Role::Muted))];
            let room = width.saturating_sub(versions.chars().count() + 2);
            let name = fit(&package.name, room, ascii);
            let mut line = lists::row(theme, &name, &ranked.positions, width, facts);
            let mark = format!("{} ", package.kind.mark(ascii));
            line.spans
                .insert(0, Span::styled(mark, theme.style(role(package.kind))));
            Some(line)
        })
        .collect();
    let header = match &diff.packages {
        Some(packages) => format!("{} Packages since {}", packages.manager, diff.from.name()),
        None => format!("Packages since {}", diff.from.name()),
    };
    let empty = if diff.packages.is_some() {
        ("No Package Changes", "The snapshot has the same packages.")
    } else {
        (
            "No Package Database",
            "No xbps, dpkg, pacman or rpm database was found in the snapshot.",
        )
    };
    let total = (header.as_str(), packages.len());
    lists::render(model, frame, area, total, &diff.package_list, rows, empty);
}

/// The preview of the selected package: what happened, and its files.
pub(super) fn render_preview(theme: Theme, frame: &mut Frame<'_>, area: Rect, diff: &DiffView) {
    let Some(package) = diff.selected_package() else {
        frame.render_widget(lists::block(theme, "Package"), area);
        return;
    };
    let block = lists::counted(theme, "Package", package.kind.as_str());
    let paragraph = Paragraph::new(preview_lines(theme, package))
        .wrap(Wrap { trim: false })
        .block(block);
    frame.render_widget(paragraph, area);
}

/// What happened to `package`, how to undo it, and its changed files.
fn preview_lines(theme: Theme, package: &PackageChange) -> Vec<Line<'static>> {
    let ascii = theme.is_ascii();
    let versions = package.versions(ascii);
    let what = match package.kind {
        PackageKind::Installed => format!("Installed since the snapshot: {versions}."),
        PackageKind::Removed => format!(
            "Removed since the snapshot, which has {versions}. Booting a clone of the \
             snapshot, or rolling back, brings it back with its package record."
        ),
        kind => format!("{} {versions}.", capitalised(kind.as_str())),
    };
    let mut lines = vec![
        Line::styled(package.name.clone(), theme.style(Role::Title)),
        Line::styled(what, theme.style(Role::Text)),
    ];
    if package.files.is_empty() {
        lines.push(Line::styled(
            "None of the files that changed belongs to it.",
            theme.style(Role::Muted),
        ));
        return lines;
    }
    lines.push(Line::default());
    lines.push(Line::styled("CHANGED FILES", theme.style(Role::Header)));
    lines.extend(
        package
            .files
            .iter()
            .map(|file| Line::styled(format!("  {file}"), theme.style(Role::Text))),
    );
    lines.push(Line::styled(
        "Enter shows them among the files.",
        theme.style(Role::Muted),
    ));
    lines
}
