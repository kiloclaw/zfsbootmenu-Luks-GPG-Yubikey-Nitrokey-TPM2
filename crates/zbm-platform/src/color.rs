//! Colours: adding ANSI colour sequences to text and removing them. Both
//! functions only transform text.

/// `text` in `color`, followed by the reset sequence.
pub fn colorize(color: &str, text: &str) -> String {
    let code = match color {
        "black" => "0;30",
        "red" => "0;31",
        "green" => "0;32",
        "orange" => "0;33",
        "blue" => "0;34",
        "magenta" => "0;35",
        "cyan" => "0;36",
        "lightgray" => "0;37",
        "darkgray" => "1;30",
        "lightred" => "1;31",
        "lightgreen" => "1;32",
        "yellow" => "1;33",
        "lightblue" => "1;34",
        "lightmagenta" => "1;35",
        "lightcyan" => "1;36",
        "white" => "1;37",
        _ => "0",
    };
    format!("\x1b[{code}m{text}\x1b[0m")
}

/// The length of a colour sequence at the start of `text`: an escape (or the
/// four characters `\033`, or any control character) followed by `[0m` or
/// `[<0-1>;3<0-7>m`.
fn sequence_len(text: &[u8]) -> Option<usize> {
    let introducer = if text.starts_with(b"\\033") {
        4
    } else if text.first().is_some_and(u8::is_ascii_control) {
        1
    } else {
        return None;
    };
    let rest = text.get(introducer..)?;
    if rest.starts_with(b"[0m") {
        return Some(introducer + 3);
    }
    match rest {
        [b'[', bold, b';', b'3', color, b'm', ..]
            if matches!(bold, b'0' | b'1') && (b'0'..=b'7').contains(color) =>
        {
            Some(introducer + 6)
        }
        _ => None,
    }
}

/// `text` without colour sequences.
pub fn decolorize(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut kept = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some(&byte) = rest.first() {
        let len = sequence_len(rest).unwrap_or_else(|| {
            kept.push(byte);
            1
        });
        rest = rest.get(len..).unwrap_or_default();
    }
    String::from_utf8_lossy(&kept).into_owned()
}
