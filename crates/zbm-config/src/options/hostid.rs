//! The host ID of `spl_hostid` on the command line, as the legacy start-up
//! reads it with Bash arithmetic and `printf`.

use zbm_types::number::shell_integer;

/// The host ID the legacy script derives from `spl_hostid`, or `None` when the
/// value is invalid.
///
/// A value that is a non-zero decimal number (`(( 10#value ))`) is printed with
/// `printf "%08x"`, which reads a leading 0 as octal; otherwise a non-zero hex
/// number, with or without `0x`, keeps its first eight digits; a zero in either
/// form becomes `0`. Anything else is invalid; the legacy script checks for
/// plain digits before it uses Bash arithmetic, which would evaluate
/// expressions such as `5-3`.
pub fn parse_spl_hostid(value: &str) -> Option<String> {
    if base_constant(value, 10).is_some_and(|number| number != 0) {
        return Some(printf_hex(value));
    }
    let hex = value.strip_prefix("0x").unwrap_or(value);
    if base_constant(hex, 16).is_some_and(|number| number != 0) {
        let digits: String = hex.chars().take(8).collect();
        return Some(printf_hex(&format!("0x{digits}")));
    }
    if shell_integer(hex) == Some(0) {
        return Some("0".to_owned());
    }
    None
}

/// The value of the Bash constant `base#digits`, with the wrap-around of Bash
/// arithmetic, or `None` when a digit is not valid in the base or there are
/// none.
fn base_constant(digits: &str, base: u32) -> Option<u64> {
    if digits.is_empty() {
        return None;
    }
    digits.chars().try_fold(0_u64, |value, digit| {
        let digit = digit.to_digit(base)?;
        Some(
            value
                .wrapping_mul(u64::from(base))
                .wrapping_add(u64::from(digit)),
        )
    })
}

/// What Bash's `printf "%08x" "$value"` prints for a number in C syntax (`0x`
/// hex, leading `0` octal, otherwise decimal), using the valid prefix of an
/// invalid number and the largest value on overflow, as `strtoumax` does.
fn printf_hex(value: &str) -> String {
    let text = value.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let (negative, text) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (base, digits) = if let Some(hex) = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .filter(|hex| hex.starts_with(|c: char| c.is_ascii_hexdigit()))
    {
        (16, hex)
    } else if text.starts_with('0') {
        (8, text)
    } else {
        (10, text)
    };
    let mut number: u64 = 0;
    let mut overflow = false;
    for digit in digits.chars().map_while(|c| c.to_digit(base)) {
        match number
            .checked_mul(u64::from(base))
            .and_then(|n| n.checked_add(u64::from(digit)))
        {
            Some(n) => number = n,
            None => overflow = true,
        }
    }
    if overflow {
        number = u64::MAX;
    }
    if negative {
        number = number.wrapping_neg();
    }
    format!("{number:08x}")
}
