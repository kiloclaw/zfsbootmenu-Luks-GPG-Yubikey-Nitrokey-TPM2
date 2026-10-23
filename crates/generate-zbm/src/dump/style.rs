//! How a scalar is quoted: the choice of style and the text of each quoting
//! style, as YAML::PP's emitter writes them in block context.

/// How a scalar is written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Style {
    Plain,
    Single,
    Double,
    Literal,
}

/// Characters YAML::PP writes only in double quotes.
fn is_control(c: char) -> bool {
    matches!(
        c,
        '\u{0}'..='\u{8}'
            | '\u{b}'
            | '\u{c}'
            | '\u{e}'..='\u{1f}'
            | '\u{7f}'..='\u{84}'
            | '\u{86}'..='\u{9f}'
            | '\u{fffe}'
            | '\u{ffff}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{85}'
            | '\u{a0}'
    )
}

/// Characters that YAML allows in a plain scalar.
fn is_printable(c: char) -> bool {
    matches!(
        c,
        '\u{20}'..='\u{7e}'
            | '\u{85}'
            | '\u{a0}'..='\u{d7ff}'
            | '\u{e000}'..='\u{fefe}'
            | '\u{ff00}'..='\u{fffd}'
            | '\u{10000}'..='\u{10ffff}'
    )
}

/// `_find_best_scalar_style` for block context.
pub(super) fn best_style(value: &str, requested: Option<Style>) -> Style {
    let mut style = requested;
    if value.is_empty() {
        style = style.or(Some(Style::Single));
    } else if value.chars().any(is_control) {
        style = Some(Style::Double);
    }
    let mut style = style.unwrap_or(Style::Plain);
    let first = value.chars().next();
    match style {
        Style::Single => {
            if value.contains(" \n")
                || value.contains("\n ")
                || value.starts_with('\n')
                || value.ends_with('\n')
            {
                style = Style::Double;
            }
        }
        Style::Plain if !value.is_empty() => {
            style = if value.contains(['\t', '\r']) || value == "\n" {
                Style::Double
            } else if value.chars().all(|c| c == ' ') {
                Style::Single
            } else if value.chars().all(|c| c == ' ' || c == '\n') {
                Style::Double
            } else if value.contains('\n') {
                Style::Literal
            } else if first.is_some_and(|c| "!&*{}[]|>@`\"'#%, ".contains(c))
                || value.starts_with("---")
                || value.starts_with("...")
                || value.contains(": ")
                || value.contains(" #")
                || value.ends_with([':', ' ', '\t'])
                || !value.chars().all(is_printable)
                || (first.is_some_and(|c| "?-:".contains(c))
                    && value.chars().nth(1).is_none_or(char::is_whitespace))
            {
                Style::Single
            } else {
                Style::Plain
            };
        }
        _ => {}
    }
    if style == Style::Single && requested.is_none() && value.contains('\'') && !value.contains('"')
    {
        style = Style::Double;
    }
    style
}

pub(super) fn single_quoted(value: &str, continuation: &str) -> String {
    // Runs of line breaks are doubled, as single-quoted scalars fold single breaks.
    let mut folded = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            let mut run = 1;
            while chars.peek() == Some(&'\n') {
                chars.next();
                run += 1;
            }
            folded.push_str(&"\n".repeat(run + 1));
        } else {
            folded.push(c);
        }
    }
    let lines: Vec<String> = folded
        .split('\n')
        .enumerate()
        .map(|(index, line)| {
            if index > 0 && !line.is_empty() {
                format!("{continuation}{line}")
            } else {
                line.to_owned()
            }
        })
        .collect();
    format!("'{}'", lines.join("\n").replace('\'', "''"))
}

/// A literal block scalar and whether it leaves the document open-ended (kept
/// trailing line breaks).
pub(super) fn literal(value: &str, indent: &str) -> (String, bool) {
    let mut indicators = String::new();
    if value.trim_start_matches('\n').starts_with(' ') {
        indicators.push('2');
    }
    let mut text = value.to_owned();
    let mut open_ended = false;
    if !text.ends_with('\n') {
        indicators.push('-');
        text.push('\n');
    } else if text.ends_with("\n\n") || text == "\n" {
        indicators.push('+');
        open_ended = true;
    }
    let body: String = text
        .split_inclusive('\n')
        .map(|line| {
            if line.starts_with('\n') {
                line.to_owned()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect();
    (format!("|{indicators}\n{body}"), open_ended)
}

pub(super) fn double_quoted(value: &str) -> String {
    let mut text = String::from('"');
    for c in value.chars() {
        let escaped = match c {
            '\n' => "\\n".to_owned(),
            '\t' => "\\t".to_owned(),
            '\r' => "\\r".to_owned(),
            '\\' => "\\\\".to_owned(),
            '"' => "\\\"".to_owned(),
            '\u{0}' => "\\0".to_owned(),
            '\u{7}' => "\\a".to_owned(),
            '\u{8}' => "\\b".to_owned(),
            '\u{b}' => "\\v".to_owned(),
            '\u{c}' => "\\f".to_owned(),
            '\u{1b}' => "\\e".to_owned(),
            '\u{85}' => "\\N".to_owned(),
            '\u{a0}' => "\\_".to_owned(),
            '\u{2028}' => "\\L".to_owned(),
            '\u{2029}' => "\\P".to_owned(),
            '\u{fffe}' | '\u{ffff}' => format!("\\u{:04x}", u32::from(c)),
            c if is_control(c) => format!("\\x{:02x}", u32::from(c)),
            c => c.to_string(),
        };
        text.push_str(&escaped);
    }
    text.push('"');
    text
}
