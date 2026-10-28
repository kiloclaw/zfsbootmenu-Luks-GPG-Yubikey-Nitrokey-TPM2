//! Numbers as the shell's `test` reads them, which the options of the
//! kernel command line always were: `[ "${zbm_retry_delay}" -gt 0 ]`.

/// The value of `text` as an integer operand of `test` (`[ text -eq 0 ]`),
/// or `None` when `test` rejects it.
///
/// Like Bash, this accepts leading white space, a sign and decimal digits
/// followed by blanks; anything else, an empty string and values outside
/// the 64-bit range are rejected.
///
/// ```
/// use zbm_types::number::shell_integer;
///
/// assert_eq!(shell_integer(" +7\t"), Some(7));
/// assert_eq!(shell_integer("-1"), Some(-1));
/// assert_eq!(shell_integer("7s"), None);
/// assert_eq!(shell_integer(""), None);
/// ```
pub fn shell_integer(text: &str) -> Option<i64> {
    let number = text.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let digits_start = usize::from(number.starts_with(['+', '-']));
    let digits_len = number
        .get(digits_start..)
        .unwrap_or_default()
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    if digits_len == 0 {
        return None;
    }
    let (value, rest) = number.split_at_checked(digits_start + digits_len)?;
    if !rest.bytes().all(|byte| byte == b' ' || byte == b'\t') {
        return None;
    }
    value.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_are_read_as_test_reads_them() {
        for (text, value) in [
            ("5", Some(5)),
            ("  5", Some(5)),
            ("5 \t", Some(5)),
            ("\n-3", Some(-3)),
            ("+0", Some(0)),
            ("5\n", None),
            ("5x", None),
            ("x", None),
            ("-", None),
            ("", None),
            ("1.5", None),
            ("99999999999999999999", None),
        ] {
            assert_eq!(shell_integer(text), value, "{text:?}");
        }
    }
}
