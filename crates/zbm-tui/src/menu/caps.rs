//! What the terminal can draw, detected once at start from the environment
//! (`docs/dev/menu-design/visual-system.md`, "Terminal capabilities").

/// How the menu may use colour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Colours {
    /// The eight basic colours and their bright forms.
    Sixteen,
    /// Bold, underlined and reverse only.
    Monochrome,
}

/// What the terminal can draw.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capabilities {
    /// Colour, or monochrome.
    pub colours: Colours,
    /// Whether it can draw line characters; ASCII borders otherwise.
    pub line_drawing: bool,
    /// Whether it can run the menu at all; the plain console prompts
    /// otherwise.
    pub interactive: bool,
    /// The Linux console in colour: its background is black and its palette
    /// the VGA one, so the theme can use white, grey and dark grey as three
    /// levels of text.
    pub console: bool,
}

impl Capabilities {
    /// Everything: colour and line characters.
    pub const FULL: Capabilities = Capabilities {
        colours: Colours::Sixteen,
        line_drawing: true,
        interactive: true,
        console: false,
    };

    /// The Linux console: colour, line characters, its known palette.
    pub const CONSOLE: Capabilities = Capabilities {
        console: true,
        ..Capabilities::FULL
    };

    /// Monochrome, with line characters.
    pub const MONOCHROME: Capabilities = Capabilities {
        colours: Colours::Monochrome,
        line_drawing: true,
        interactive: true,
        console: false,
    };

    /// Monochrome, with ASCII borders.
    pub const ASCII: Capabilities = Capabilities {
        colours: Colours::Monochrome,
        line_drawing: false,
        interactive: true,
        console: false,
    };

    /// The capabilities the environment variables `var` describe.
    pub fn detect(var: impl Fn(&str) -> Option<String>) -> Capabilities {
        let term = var("TERM").unwrap_or_default();
        if term.is_empty() || term == "dumb" {
            return Capabilities {
                interactive: false,
                ..Capabilities::ASCII
            };
        }
        let old = term.starts_with("vt1") || term.starts_with("vt2") || term == "ansi";
        let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .find_map(|name| var(name).filter(|value| !value.is_empty()))
            .is_some_and(|locale| {
                let locale = locale.to_ascii_lowercase();
                locale.contains("utf-8") || locale.contains("utf8")
            });
        let line_drawing = !old && (term == "linux" || utf8);
        let no_colour = var("NO_COLOR").is_some();
        let colours = if no_colour || old {
            Colours::Monochrome
        } else {
            Colours::Sixteen
        };
        Capabilities {
            colours,
            line_drawing,
            interactive: true,
            console: term == "linux" && colours == Colours::Sixteen,
        }
    }

    /// The capabilities `zbm.theme=` asks for: `colour`, `console` (the
    /// Linux console's palette, to preview it elsewhere), `mono` or `ascii`.
    pub fn named(name: &str) -> Option<Capabilities> {
        match name {
            "colour" | "color" => Some(Capabilities::FULL),
            "console" => Some(Capabilities::CONSOLE),
            "mono" => Some(Capabilities::MONOCHROME),
            "ascii" => Some(Capabilities::ASCII),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn detect(vars: &[(&str, &str)]) -> Capabilities {
        Capabilities::detect(|name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        })
    }

    #[test]
    fn terminals_are_recognized() {
        let cases = [
            (
                "linux console",
                vec![("TERM", "linux")],
                Capabilities::CONSOLE,
            ),
            (
                "xterm in UTF-8",
                vec![("TERM", "xterm-256color"), ("LANG", "en_US.UTF-8")],
                Capabilities::FULL,
            ),
            (
                "xterm without UTF-8",
                vec![("TERM", "xterm"), ("LANG", "C")],
                Capabilities {
                    colours: Colours::Sixteen,
                    ..Capabilities::ASCII
                },
            ),
            (
                "vt100",
                vec![("TERM", "vt100"), ("LANG", "C.UTF-8")],
                Capabilities::ASCII,
            ),
            ("vt220", vec![("TERM", "vt220")], Capabilities::ASCII),
            (
                "NO_COLOR",
                vec![("TERM", "linux"), ("NO_COLOR", "")],
                Capabilities::MONOCHROME,
            ),
            (
                "LC_ALL wins over LANG",
                vec![("TERM", "screen"), ("LC_ALL", "C"), ("LANG", "en_US.UTF-8")],
                Capabilities {
                    colours: Colours::Sixteen,
                    ..Capabilities::ASCII
                },
            ),
        ];

        for (name, vars, expected) in cases {
            assert_eq!(detect(&vars), expected, "{name}");
        }
    }

    #[test]
    fn a_dumb_terminal_cannot_run_the_menu() {
        assert!(!detect(&[("TERM", "dumb")]).interactive);
        assert!(!detect(&[]).interactive);
    }

    #[test]
    fn themes_are_named() {
        assert_eq!(Capabilities::named("ascii"), Some(Capabilities::ASCII));
        assert_eq!(Capabilities::named("color"), Some(Capabilities::FULL));
        assert_eq!(Capabilities::named("console"), Some(Capabilities::CONSOLE));
        assert_eq!(Capabilities::named("mono"), Some(Capabilities::MONOCHROME));
        assert_eq!(Capabilities::named("loud"), None);
    }
}
