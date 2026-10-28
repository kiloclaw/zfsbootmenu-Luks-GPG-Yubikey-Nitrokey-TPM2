//! The packages segment of the changes since a snapshot
//! (`docs/dev/menu-design/boot-health.md`): each package with what happened
//! to it and its versions, and a preview with its changed files.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use zbm_types::Printable;

use super::lists;
use crate::menu::format::fit;
use crate::menu::model::{DiffView, Model};
use crate::menu::theme::{Role, Theme};
use crate::menu::world::{PackageChange, PackageKind};
use crate::text::text;

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
            let name = fit(&Printable(&package.name).to_string(), room, ascii);
            let mut line = lists::row(theme, &name, &ranked.positions, width, facts);
            let mark = format!("{} ", package.kind.mark(ascii));
            line.spans
                .insert(0, Span::styled(mark, theme.style(role(package.kind))));
            Some(line)
        })
        .collect();
    let snapshot = diff.from.name();
    let header = match &diff.packages {
        Some(packages) => text!(
            "packages-since.manager",
            manager = packages.manager.as_str(),
            snapshot = snapshot
        ),
        None => text!("packages-since", snapshot = snapshot),
    };
    let (title, hint) = if diff.packages.is_some() {
        (text!("packages-none"), text!("packages-none.text"))
    } else {
        (
            text!("packages-no-database"),
            text!("packages-no-database.text"),
        )
    };
    let empty = (title.as_str(), hint.as_str());
    let total = (header.as_str(), packages.len());
    lists::render(model, frame, area, total, &diff.package_list, rows, empty);
}

/// The preview of the selected package: what happened, and its files.
pub(super) fn render_preview(theme: Theme, frame: &mut Frame<'_>, area: Rect, diff: &DiffView) {
    let Some(package) = diff.selected_package() else {
        frame.render_widget(lists::block(theme, &text!("package-title")), area);
        return;
    };
    let block = lists::counted(theme, &text!("package-title"), &kind_word(package.kind));
    let paragraph = Paragraph::new(preview_lines(theme, package))
        .wrap(Wrap { trim: false })
        .block(block);
    frame.render_widget(paragraph, area);
}

/// What happened to a package, in a word.
fn kind_word(kind: PackageKind) -> String {
    match kind {
        PackageKind::Installed => text!("package-installed"),
        PackageKind::Removed => text!("package-removed"),
        PackageKind::Upgraded => text!("package-upgraded"),
        PackageKind::Downgraded => text!("package-downgraded"),
    }
}

/// What happened to `package`, how to undo it, and its changed files.
fn preview_lines(theme: Theme, package: &PackageChange) -> Vec<Line<'static>> {
    let ascii = theme.is_ascii();
    let versions = package.versions(ascii);
    let versions = versions.as_str();
    let what = match package.kind {
        PackageKind::Installed => text!("package-installed.since", versions = versions),
        PackageKind::Removed => text!("package-removed.since", versions = versions),
        PackageKind::Upgraded => text!("package-upgraded.since", versions = versions),
        PackageKind::Downgraded => text!("package-downgraded.since", versions = versions),
    };
    let mut lines = vec![
        Line::styled(
            Printable(&package.name).to_string(),
            theme.style(Role::Title),
        ),
        Line::styled(what, theme.style(Role::Text)),
    ];
    if package.files.is_empty() {
        lines.push(Line::styled(
            text!("package-no-files"),
            theme.style(Role::Muted),
        ));
        return lines;
    }
    lines.push(Line::default());
    lines.push(Line::styled(
        text!("package-changed-files"),
        theme.style(Role::Header),
    ));
    lines.extend(
        package
            .files
            .iter()
            .map(|file| Line::styled(format!("  {}", Printable(file)), theme.style(Role::Text))),
    );
    lines.push(Line::styled(
        text!("package-files-hint"),
        theme.style(Role::Muted),
    ));
    lines
}
