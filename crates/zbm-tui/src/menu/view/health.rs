//! Boot health in the main view (`docs/dev/menu-design/boot-health.md`):
//! the banner of a fallback or a one-time boot, the labels of rows whose
//! checks found problems, and the lines of the inspector.

use ratatui::text::{Line, Span};

use crate::menu::format::fit;
use crate::menu::model::Model;
use crate::menu::theme::{Glyph, Role, Theme};
use crate::menu::world::{Environment, Target, Verdict, worst};

/// The banner: its forms, longest first, and its role; `None` when there
/// is nothing to say.
pub(super) fn banner(model: &Model) -> Option<(Vec<String>, Role)> {
    let seconds = model.countdown_seconds();
    if let Some(fallback) = &model.snapshot.fallback {
        let failed = &fallback.failed;
        let attempts = fallback.attempts;
        let Some(seconds) = seconds else {
            let forms = vec![
                format!(" {failed} failed to start {attempts} times; its boot tries are used up."),
                format!(" {failed} failed to start {attempts} times."),
            ];
            return Some((forms, Role::Warning));
        };
        let target = match &fallback.target {
            Target::Environment(name) => format!("{name}, the last good one"),
            Target::Snapshot(name) => format!("a clone of {}", name.name()),
        };
        let forms = vec![
            format!(" {failed} failed to start {attempts} times. Booting {target} in {seconds} s."),
            format!(" {failed} failed {attempts} times. Booting {target} in {seconds} s."),
            format!(" {failed} failed {attempts} times. Fallback in {seconds} s."),
            format!(" Fallback in {seconds} s."),
        ];
        return Some((forms, Role::Warning));
    }
    let seconds = seconds?;
    let name = &model.selected_environment()?.name;
    if model.snapshot.next_boot.as_ref() == Some(name) {
        let forms = vec![
            format!(" Booting {name} once, as the running system asked, in {seconds} s."),
            format!(" Booting {name} once in {seconds} s."),
            format!(" Booting in {seconds} s."),
        ];
        return Some((forms, Role::Accent));
    }
    let forms = vec![
        format!(" Booting {name} in {seconds} s. Press any key to choose another."),
        format!(" Booting {name} in {seconds} s."),
        format!(" Booting in {seconds} s."),
    ];
    Some((forms, Role::Accent))
}

/// The banner's `text` as a line. A countdown is plain text with the
/// environment in bold, the seconds in the accent colour and the hint
/// secondary, so the eye goes to what will boot and when; the banner of a
/// fallback stays a warning throughout.
pub(super) fn banner_line(model: &Model, text: &str, role: Role) -> Line<'static> {
    let theme = model.theme;
    if role != Role::Accent {
        return Line::styled(text.to_owned(), theme.style(role));
    }
    let mut marks = Vec::new();
    if let Some(environment) = model.selected_environment() {
        marks.push((environment.name.to_string(), Role::Title));
    }
    if let Some(seconds) = model.countdown_seconds() {
        marks.push((format!("{seconds} s"), Role::Accent));
    }
    marks.push(("Press any key to choose another.".to_owned(), Role::Muted));
    emphasised(theme, text, &marks)
}

/// `text` in the text role, with each of `marks` in its own role.
fn emphasised(theme: Theme, text: &str, marks: &[(String, Role)]) -> Line<'static> {
    let mut spans = Vec::new();
    let mut rest = text;
    while let Some((at, mark, role)) = marks
        .iter()
        .filter(|(mark, _)| !mark.is_empty())
        .filter_map(|(mark, role)| Some((rest.find(mark.as_str())?, mark, *role)))
        .min_by_key(|(at, ..)| *at)
    {
        if at > 0 {
            spans.push(Span::styled(rest[..at].to_owned(), theme.style(Role::Text)));
        }
        spans.push(Span::styled(mark.clone(), theme.style(role)));
        rest = &rest[at + mark.len()..];
    }
    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_owned(), theme.style(Role::Text)));
    }
    Line::from(spans)
}

/// The labels of a row: problems its checks found, and `next boot`.
pub(super) fn badges(model: &Model, environment: &Environment) -> Vec<Span<'static>> {
    let theme = model.theme;
    let mut badges = Vec::new();
    if let Some((verdict, count)) = worst(&environment.checks) {
        let (noun, role) = match verdict {
            Verdict::Failure => ("problem", Role::Danger),
            _ => ("warning", Role::Warning),
        };
        let plural = if count == 1 { "" } else { "s" };
        badges.push(Span::styled(
            format!("{count} {noun}{plural}  "),
            theme.style(role),
        ));
    }
    if model.snapshot.next_boot.as_ref() == Some(&environment.name) {
        badges.push(Span::styled("next boot  ", theme.style(Role::Accent)));
    }
    badges
}

/// The glyph and role of a verdict.
fn mark(theme: Theme, verdict: Verdict) -> (&'static str, Role) {
    match verdict {
        Verdict::Pass => (theme.glyph(Glyph::Done), Role::Good),
        Verdict::Warning => (theme.glyph(Glyph::Warning), Role::Warning),
        Verdict::Failure => (theme.glyph(Glyph::Failed), Role::Danger),
    }
}

/// The inspector's facts on boot health: the checks in one line, boot
/// counting, the one-time boot; as (label, value).
pub(super) fn facts(
    model: &Model,
    environment: &Environment,
) -> Vec<(&'static str, Vec<Span<'static>>)> {
    let theme = model.theme;
    let checks = &environment.checks;
    let summary = match worst(checks) {
        _ if checks.is_empty() => vec![Span::styled("not checked", theme.style(Role::Muted))],
        None => {
            let (glyph, role) = mark(theme, Verdict::Pass);
            vec![Span::styled(
                format!("{glyph} all {} passed", checks.len()),
                theme.style(role),
            )]
        }
        Some((verdict, _)) => {
            let (glyph, role) = mark(theme, verdict);
            let first = checks
                .iter()
                .find(|c| c.verdict == verdict)
                .map(|c| c.detail.clone());
            vec![Span::styled(
                format!("{glyph} {}", first.unwrap_or_default()),
                theme.style(role),
            )]
        }
    };
    let mut facts = vec![("Checks", summary)];
    if let Some(tries) = environment.tries {
        let role = if tries.left == 0 {
            Role::Danger
        } else {
            Role::Muted
        };
        let text = format!("{} of {} left", tries.left, tries.total);
        facts.push(("Boot Tries", vec![Span::styled(text, theme.style(role))]));
    }
    if model.snapshot.next_boot.as_ref() == Some(&environment.name) {
        let text = "once, as the running system asked";
        facts.push((
            "Next Boot",
            vec![Span::styled(text, theme.style(Role::Accent))],
        ));
    }
    facts
}

/// When the pane is tall: every check, with what to do about the ones
/// that did not pass.
pub(super) fn check_lines(
    model: &Model,
    environment: &Environment,
    width: usize,
) -> Vec<Line<'static>> {
    let theme = model.theme;
    let mut lines = vec![Line::styled("CHECKS", theme.style(Role::Header))];
    for check in &environment.checks {
        let (glyph, role) = mark(theme, check.verdict);
        let text = fit(
            &format!("{}: {}", check.name, check.detail),
            width.saturating_sub(4),
            theme.is_ascii(),
        );
        lines.push(Line::from(vec![
            Span::styled(format!("  {glyph} "), theme.style(role)),
            Span::raw(text),
        ]));
        if let Some(fix) = &check.fix {
            let fix = fit(fix, width.saturating_sub(4), theme.is_ascii());
            lines.push(Line::styled(format!("    {fix}"), theme.style(Role::Muted)));
        }
    }
    lines
}
