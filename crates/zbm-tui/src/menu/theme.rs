//! The visual system: every styled piece of text has a [`Role`], and the
//! [`Theme`] draws each role in colour or in monochrome, with line or ASCII
//! borders (`docs/dev/menu-design/visual-system.md`).
//!
//! Views never pick colours; meaning is never carried by colour alone, so
//! each role that means something also has a word or a mark in the text.

use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;

use super::caps::{Capabilities, Colours};

/// What a piece of text is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    /// Ordinary text.
    Text,
    /// Secondary facts: sizes, ages, origins.
    Muted,
    /// View and pane titles.
    Title,
    /// The selected line of a list.
    Selected,
    /// Characters the filter matched.
    Match,
    /// The default environment or kernel.
    Default,
    /// `ONLINE`, `open`, `done`.
    Good,
    /// `DEGRADED`, read-only, warnings.
    Warning,
    /// `FAULTED`, failures, destructive actions.
    Danger,
    /// Encrypted and not unlocked.
    Locked,
    /// Key names in the footer and help.
    Key,
    /// Borders and separators.
    Border,
    /// The status bar.
    StatusBar,
    /// The tint colour of interactive things: the back label, the banner.
    Accent,
    /// Section headers: upper case, secondary.
    Header,
}

/// The glyphs of the menu (`docs/dev/menu-design/apple-hig.md`, rule 15):
/// each in the console fonts of the image, with an ASCII form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Glyph {
    /// The back label of the navigation bar.
    Back,
    /// The selected row.
    Selected,
    /// A marked row.
    Marked,
    /// A step or action done.
    Done,
    /// A step running.
    Running,
    /// A step or action failed.
    Failed,
    /// A step skipped.
    Skipped,
    /// A warning.
    Warning,
    /// More input follows (an action that asks first).
    Ellipsis,
    /// A list item in a message.
    Bullet,
}

impl Glyph {
    /// Every glyph, for tests.
    pub const ALL: [Glyph; 10] = [
        Glyph::Back,
        Glyph::Selected,
        Glyph::Marked,
        Glyph::Done,
        Glyph::Running,
        Glyph::Failed,
        Glyph::Skipped,
        Glyph::Warning,
        Glyph::Ellipsis,
        Glyph::Bullet,
    ];

    /// Its Unicode form.
    fn unicode(self) -> &'static str {
        match self {
            Glyph::Back => "‹",
            Glyph::Selected => "▶",
            Glyph::Marked | Glyph::Bullet => "•",
            Glyph::Done => "√",
            Glyph::Running | Glyph::Ellipsis => "…",
            Glyph::Failed => "×",
            Glyph::Skipped => "–",
            Glyph::Warning => "!",
        }
    }

    /// Its ASCII form.
    fn ascii(self) -> &'static str {
        match self {
            Glyph::Back => "<",
            Glyph::Marked => "+",
            Glyph::Done => "*",
            Glyph::Selected | Glyph::Running => ">",
            Glyph::Failed => "x",
            Glyph::Skipped | Glyph::Bullet => "-",
            Glyph::Warning => "!",
            Glyph::Ellipsis => "...",
        }
    }
}

/// Borders of ASCII characters, for terminals without line drawing.
pub const ASCII_BORDER: border::Set<'static> = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

/// How the menu draws.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Theme {
    capabilities: Capabilities,
}

impl Theme {
    /// The theme for a terminal with `capabilities`.
    pub fn new(capabilities: Capabilities) -> Theme {
        Theme { capabilities }
    }

    /// The style of `role`.
    pub fn style(self, role: Role) -> Style {
        match self.capabilities.colours {
            Colours::Sixteen => colour(role),
            Colours::Monochrome => monochrome(role),
        }
    }

    /// The border characters.
    pub fn border(self) -> border::Set<'static> {
        if self.capabilities.line_drawing {
            border::PLAIN
        } else {
            ASCII_BORDER
        }
    }

    /// The character between panes: a vertical line.
    pub fn separator(self) -> &'static str {
        if self.capabilities.line_drawing {
            "│"
        } else {
            "|"
        }
    }

    /// The separator between facts on one line: a middle dot, or `|`.
    pub fn dot(self) -> &'static str {
        if self.capabilities.line_drawing {
            " · "
        } else {
            " | "
        }
    }

    /// The form of `glyph` this terminal draws.
    pub fn glyph(self, glyph: Glyph) -> &'static str {
        if self.capabilities.line_drawing {
            glyph.unicode()
        } else {
            glyph.ascii()
        }
    }

    /// Whether it draws with ASCII characters only.
    pub fn is_ascii(self) -> bool {
        !self.capabilities.line_drawing
    }
}

/// The 16-colour style of `role`.
fn colour(role: Role) -> Style {
    let bold = Modifier::BOLD;
    match role {
        Role::Text => Style::new(),
        Role::Muted | Role::Border | Role::Header => Style::new().fg(Color::DarkGray),
        Role::Title => Style::new().add_modifier(bold),
        Role::Selected => Style::new().fg(Color::Black).bg(Color::Cyan),
        Role::Match => Style::new().fg(Color::Yellow).add_modifier(bold),
        Role::Default => Style::new().fg(Color::Green).add_modifier(bold),
        Role::Good => Style::new().fg(Color::Green),
        Role::Warning => Style::new().fg(Color::Yellow),
        Role::Danger => Style::new().fg(Color::Red).add_modifier(bold),
        Role::Locked => Style::new().fg(Color::Magenta),
        Role::Key | Role::Accent => Style::new().fg(Color::Cyan).add_modifier(bold),
        Role::StatusBar => Style::new().fg(Color::Black).bg(Color::White),
    }
}

/// The monochrome style of `role`: bold, underlined and reverse only.
fn monochrome(role: Role) -> Style {
    let style = Style::new();
    match role {
        Role::Text | Role::Muted | Role::Border | Role::Good | Role::Header => style,
        Role::Title | Role::Default | Role::Warning | Role::Key | Role::Accent => {
            style.add_modifier(Modifier::BOLD)
        }
        Role::Selected | Role::StatusBar => style.add_modifier(Modifier::REVERSED),
        Role::Match => style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        Role::Danger => style.add_modifier(Modifier::BOLD | Modifier::REVERSED),
        Role::Locked => style.add_modifier(Modifier::UNDERLINED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROLES: [Role; 15] = [
        Role::Text,
        Role::Muted,
        Role::Title,
        Role::Selected,
        Role::Match,
        Role::Default,
        Role::Good,
        Role::Warning,
        Role::Danger,
        Role::Locked,
        Role::Key,
        Role::Border,
        Role::StatusBar,
        Role::Accent,
        Role::Header,
    ];

    #[test]
    fn monochrome_styles_use_no_colour() {
        let theme = Theme::new(Capabilities::MONOCHROME);

        for role in ROLES {
            let style = theme.style(role);
            assert_eq!((style.fg, style.bg), (None, None), "{role:?}");
        }
    }

    #[test]
    fn colour_styles_use_the_basic_colours_only() {
        let theme = Theme::new(Capabilities::FULL);

        for role in ROLES {
            let style = theme.style(role);
            for colour in [style.fg, style.bg].into_iter().flatten() {
                assert!(
                    !matches!(colour, Color::Rgb(..) | Color::Indexed(_)),
                    "{role:?}: {colour:?}"
                );
            }
        }
    }

    #[test]
    fn ascii_terminals_get_ascii_borders() {
        assert_eq!(Theme::new(Capabilities::ASCII).border(), ASCII_BORDER);
        assert_eq!(Theme::new(Capabilities::ASCII).separator(), "|");
        assert!(Theme::new(Capabilities::ASCII).is_ascii());
        assert_eq!(Theme::new(Capabilities::FULL).border(), border::PLAIN);
        assert_eq!(Theme::new(Capabilities::FULL).separator(), "│");
    }

    #[test]
    fn ascii_terminals_get_ascii_glyphs() {
        let ascii = Theme::new(Capabilities::ASCII);
        let full = Theme::new(Capabilities::FULL);

        for glyph in Glyph::ALL {
            assert!(ascii.glyph(glyph).is_ascii(), "{glyph:?}");
            assert!(!full.glyph(glyph).is_empty(), "{glyph:?}");
        }
        assert_eq!(full.glyph(Glyph::Back), "‹");
    }
}
