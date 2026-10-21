//! Bash semantics that the legacy code relies on.
//!
//! The legacy code passes values through `test`, `echo` and `read -r`, each of which changes some values in ways
//! that are easy to miss: `[ x -gt 0 ]` accepts `+5` but not `5x`, `echo "$arg"` prints nothing when the argument is
//! `-n`, and `read -r` drops surrounding blanks. The ports call these functions where the legacy code uses the
//! builtins, so they treat every value the same way.

/// The value of `text` as an integer operand of `test` (`[ text -eq 0 ]`), or `None` when `test` rejects it.
///
/// Like Bash, this accepts leading white space, a sign and decimal digits followed by blanks; anything else, an
/// empty string and values outside the 64-bit range are rejected.
pub fn test_integer(text: &str) -> Option<i64> {
    let number = text.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let digits_start = usize::from(number.starts_with(['+', '-']));
    let digits_len = number[digits_start..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    if digits_len == 0 {
        return None;
    }
    let (value, rest) = number.split_at(digits_start + digits_len);
    if !rest.bytes().all(|byte| byte == b' ' || byte == b'\t') {
        return None;
    }
    value.parse().ok()
}

/// What Bash's builtin `echo "$arg"` prints for a single argument.
///
/// An argument made only of `-` and the option letters `n`, `e` and `E` is taken as options: `-n` prints nothing,
/// and `-e` or `-E` print just a newline. Any other argument is printed followed by a newline.
pub fn echo(arg: &str) -> String {
    match arg.strip_prefix('-') {
        Some(options) if !options.is_empty() && options.chars().all(|c| "neE".contains(c)) => {
            if options.contains('n') {
                String::new()
            } else {
                "\n".to_owned()
            }
        }
        _ => format!("{arg}\n"),
    }
}

/// The lines `while read -r line` reads from `text`, with the default `IFS`.
///
/// Each line loses its leading and trailing spaces and tabs, and any NUL bytes, which `read` drops. A last line
/// without a newline is not read: `read` stores it but fails, so the loop ends before it.
pub fn read_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| trim_ifs(&line.replace('\0', "")).to_owned())
        .collect();
    // The part after the last newline is either empty or a line that `read` does not deliver.
    lines.pop();
    lines
}

/// The first line `read -r value < file` stores, with the default `IFS`; an empty file gives an empty value.
pub fn read_first_line(text: &str) -> String {
    let line = text
        .split('\n')
        .next()
        .unwrap_or_default()
        .replace('\0', "");
    trim_ifs(&line).to_owned()
}

/// Removes the characters of the default `IFS` (space, tab, newline) from both ends.
pub fn trim_ifs(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\n'])
}

/// The text a command substitution `$(...)` produces from `output`: trailing newlines are removed.
pub fn command_substitution(output: &str) -> &str {
    output.trim_end_matches('\n')
}

/// Quotes `text` as one shell word, in single quotes, with each `'` written as `'\''`.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_round_trips_through_sh() {
        for text in ["", "a b", "it's", "$HOME", "a\nb", "'\\'", "*"] {
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("printf %s {}", quote(text)))
                .output()
                .unwrap();
            assert_eq!(String::from_utf8(output.stdout).unwrap(), text);
        }
    }

    #[test]
    fn test_integers_follow_bash() {
        assert_eq!(test_integer("5"), Some(5));
        assert_eq!(test_integer("+5"), Some(5));
        assert_eq!(test_integer("-12"), Some(-12));
        assert_eq!(test_integer("007"), Some(7));
        assert_eq!(test_integer(" \t5 \t"), Some(5));
        assert_eq!(test_integer("\n5"), Some(5));
        assert_eq!(test_integer("5\n"), None);
        assert_eq!(test_integer(""), None);
        assert_eq!(test_integer("-"), None);
        assert_eq!(test_integer("5x"), None);
        assert_eq!(test_integer("0x10"), None);
        assert_eq!(test_integer("9223372036854775807"), Some(i64::MAX));
        assert_eq!(test_integer("9223372036854775808"), None);
        assert_eq!(test_integer("-9223372036854775808"), Some(i64::MIN));
    }

    #[test]
    fn echo_takes_option_letters_as_options() {
        assert_eq!(echo("value"), "value\n");
        assert_eq!(echo(""), "\n");
        assert_eq!(echo("-"), "-\n");
        assert_eq!(echo("-n"), "");
        assert_eq!(echo("-e"), "\n");
        assert_eq!(echo("-E"), "\n");
        assert_eq!(echo("-neE"), "");
        assert_eq!(echo("-x"), "-x\n");
        assert_eq!(echo("-n x"), "-n x\n");
        assert_eq!(echo("--n"), "--n\n");
        assert_eq!(echo(r"a\nb"), "a\\nb\n");
    }

    #[test]
    fn read_drops_blanks_and_the_unterminated_line() {
        assert_eq!(read_lines("a\n  b c \t\n\nlast"), ["a", "b c", ""]);
        assert_eq!(read_lines("a\n"), ["a"]);
        assert!(read_lines("").is_empty());
        assert!(read_lines("only").is_empty());
        assert_eq!(read_lines("qu\0iet\0\n"), ["quiet"]);
        assert_eq!(read_first_line("  first \nsecond\n"), "first");
        assert_eq!(read_first_line("a\0b"), "ab");
        assert_eq!(read_first_line(""), "");
        assert_eq!(command_substitution("x\n\n"), "x");
    }
}
