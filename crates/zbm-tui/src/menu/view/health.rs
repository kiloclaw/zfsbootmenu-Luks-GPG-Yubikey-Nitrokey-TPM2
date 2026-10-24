//! Boot health in the main view (`docs/dev/menu-design/boot-health.md`):
//! the banner of a fallback or a one-time boot, the labels of rows whose
//! checks found problems, and the lines of the inspector.

use ratatui::text::{Line, Span};

use crate::menu::format::fit;
use crate::menu::model::Model;
use crate::menu::theme::{Glyph, Role, Theme};
use crate::menu::world::{Environment, Target, Verdict, worst};
use crate::text::text;

/// The banner: its forms, longest first, and its role; `None` when there
/// is nothing to say.
pub(super) fn banner(model: &Model) -> Option<(Vec<String>, Role)> {
    let seconds = model.countdown_seconds();
    let spaced = |forms: Vec<String>| forms.into_iter().map(|form| format!(" {form}")).collect();
    if let Some(fallback) = &model.snapshot.fallback {
        let failed = fallback.failed.to_string();
        let attempts = fallback.attempts;
        let Some(seconds) = seconds else {
            let forms = vec![
                text!(
                    "banner-used-up",
                    failed = failed.as_str(),
                    attempts = attempts
                ),
                text!(
                    "banner-used-up.short",
                    failed = failed.as_str(),
                    attempts = attempts
                ),
            ];
            return Some((spaced(forms), Role::Warning));
        };
        let target = match &fallback.target {
            Target::Environment(name) => {
                text!(
                    "fallback-target-environment",
                    environment = name.to_string()
                )
            }
            Target::Snapshot(name) => text!("fallback-target-clone", snapshot = name.name()),
        };
        let forms = vec![
            text!(
                "banner-fallback",
                failed = failed.as_str(),
                attempts = attempts,
                target = target.as_str(),
                seconds = seconds
            ),
            text!(
                "banner-fallback.shorter",
                failed = failed.as_str(),
                attempts = attempts,
                target = target.as_str(),
                seconds = seconds
            ),
            text!(
                "banner-fallback.short",
                failed = failed.as_str(),
                attempts = attempts,
                seconds = seconds
            ),
            text!("banner-fallback.shortest", seconds = seconds),
        ];
        return Some((spaced(forms), Role::Warning));
    }
    let seconds = seconds?;
    let selected = &model.selected_environment()?.name;
    let name = selected.to_string();
    let environment = name.as_str();
    let forms = if model.snapshot.next_boot.as_ref() == Some(selected) {
        vec![
            text!("banner-once", environment = environment, seconds = seconds),
            text!(
                "banner-once.short",
                environment = environment,
                seconds = seconds
            ),
            text!("banner-once.shortest", seconds = seconds),
        ]
    } else {
        vec![
            text!(
                "banner-countdown",
                environment = environment,
                seconds = seconds
            ),
            text!(
                "banner-countdown.short",
                environment = environment,
                seconds = seconds
            ),
            text!("banner-countdown.shortest", seconds = seconds),
        ]
    };
    Some((spaced(forms), Role::Accent))
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
        marks.push((text!("countdown-seconds", seconds = seconds), Role::Accent));
    }
    marks.push((text!("countdown-hint"), Role::Muted));
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
        let (badge, role) = match verdict {
            Verdict::Failure => (text!("badge-problems", count = count), Role::Danger),
            _ => (text!("badge-warnings", count = count), Role::Warning),
        };
        badges.push(Span::styled(format!("{badge}  "), theme.style(role)));
    }
    if model.snapshot.next_boot.as_ref() == Some(&environment.name) {
        badges.push(Span::styled(
            format!("{}  ", text!("badge-next-boot")),
            theme.style(Role::Accent),
        ));
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
pub(super) fn facts(model: &Model, environment: &Environment) -> Vec<(String, Vec<Span<'static>>)> {
    let theme = model.theme;
    let checks = &environment.checks;
    let summary = match worst(checks) {
        _ if checks.is_empty() => {
            vec![Span::styled(text!("checks-none"), theme.style(Role::Muted))]
        }
        None => {
            let (glyph, role) = mark(theme, Verdict::Pass);
            vec![Span::styled(
                format!("{glyph} {}", text!("checks-passed", count = checks.len())),
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
    let mut facts = vec![(text!("fact-checks"), summary)];
    if let Some(tries) = environment.tries {
        let role = if tries.left == 0 {
            Role::Danger
        } else {
            Role::Muted
        };
        let left = text!("tries-left", left = tries.left, total = tries.total);
        facts.push((
            text!("fact-boot-tries"),
            vec![Span::styled(left, theme.style(role))],
        ));
    }
    if model.snapshot.next_boot.as_ref() == Some(&environment.name) {
        facts.push((
            text!("fact-next-boot"),
            vec![Span::styled(
                text!("next-boot-once"),
                theme.style(Role::Accent),
            )],
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
    let mut lines = vec![Line::styled(
        text!("details-checks"),
        theme.style(Role::Header),
    )];
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
