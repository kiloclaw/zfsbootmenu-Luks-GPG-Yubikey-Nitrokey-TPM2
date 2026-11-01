//! Small pieces of Perl's behaviour that `generate-zbm` relies on: path names,
//! splitting, `%current` substitutions, number formatting and `Data::Dumper`
//! output.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// `s{(.)/*\z}{$1}s`: removes trailing slashes, keeping a lone `/`.
fn strip_trailing_slashes(path: &str) -> &str {
    let stripped = path.trim_end_matches('/');
    if stripped.is_empty() && !path.is_empty() {
        "/"
    } else {
        stripped
    }
}

/// `File::Basename`'s `fileparse` without suffixes: the directory part, ending
/// with `/` (or `./`), and the rest.
fn fileparse(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(slash) => (&path[..=slash], &path[slash + 1..]),
        None => ("./", path),
    }
}

/// `File::Basename`'s `basename` on Unix.
pub fn basename(path: &str) -> String {
    let path = strip_trailing_slashes(path);
    let (dir, base) = fileparse(path);
    if base.is_empty() {
        dir.to_owned()
    } else {
        base.to_owned()
    }
}

/// `File::Basename`'s `dirname` on Unix.
pub fn dirname(path: &str) -> String {
    let (dir, base) = fileparse(path);
    let dir = strip_trailing_slashes(dir);
    if base.is_empty() {
        let (dir, _) = fileparse(dir);
        strip_trailing_slashes(dir).to_owned()
    } else {
        dir.to_owned()
    }
}

/// Perl's white space for `\s` on bytes.
pub fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// `split /\s+/, $text`: fields between runs of white space, with an empty
/// first field when the text starts with white space and without empty fields
/// at the end.
pub fn split_whitespace(text: &[u8]) -> Vec<&[u8]> {
    let mut fields = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.iter().position(|byte| is_space(*byte)) {
        let (field, after) = rest.split_at(at);
        fields.push(field);
        let spaces = after.iter().take_while(|byte| is_space(**byte)).count();
        rest = after.split_at(spaces).1;
    }
    fields.push(rest);
    while fields.last().is_some_and(|field| field.is_empty()) {
        fields.pop();
    }
    fields
}

/// `chomp`: removes one trailing newline.
pub fn chomp(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\n").unwrap_or(line)
}

/// Whether a character is a word character for `\b`.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The byte positions where `needle` occurs in `text`, ignoring the case of
/// ASCII letters.
fn find_ignoring_case(text: &str, needle: &str) -> impl Iterator<Item = usize> {
    let haystack = text.as_bytes().to_vec();
    let needle = needle.as_bytes().to_vec();
    (0..=haystack.len().saturating_sub(needle.len())).filter(move |&at| {
        haystack
            .get(at..at + needle.len())
            .is_some_and(|window| window.eq_ignore_ascii_case(&needle))
    })
}

/// `s/%current\b/%{current}/i`.
pub fn mark_current(text: &str) -> String {
    let found = find_ignoring_case(text, "%current").find(|&at| {
        text.is_char_boundary(at) && !text[at + 8..].chars().next().is_some_and(is_word)
    });
    match found {
        Some(at) => format!("{}%{{current}}{}", &text[..at], &text[at + 8..]),
        None => text.to_owned(),
    }
}

/// Whether the text contains `%{current}`, in any case.
pub fn has_current(text: &str) -> bool {
    find_ignoring_case(text, "%{current}").next().is_some()
}

/// `s/%\{current\}/$replacement/i`.
pub fn replace_current(text: &str, replacement: &str) -> String {
    match find_ignoring_case(text, "%{current}").find(|&at| text.is_char_boundary(at)) {
        Some(at) => format!("{}{replacement}{}", &text[..at], &text[at + 10..]),
        None => text.to_owned(),
    }
}

/// `s/-\Q$version\E$//`: removes `-<version>` from the end, or from before a
/// final newline.
pub fn strip_version_suffix(name: &str, version: &str) -> String {
    let suffix = format!("-{version}");
    if let Some(body) = name.strip_suffix(&format!("{suffix}\n")) {
        return format!("{body}\n");
    }
    name.strip_suffix(&suffix).unwrap_or(name).to_owned()
}

/// `sprintf("%08x", $text)`: the number at the start of the text in hex, as
/// Perl converts a string to an unsigned integer for `%x`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Perl's conversions: toward zero, saturating at the range"
)]
pub fn hex_of_numeric_text(text: &str) -> String {
    let number = numeric_prefix(text);
    let value = if number < 0.0 {
        // Perl converts a negative number to the unsigned integer with the same bits.
        u64::from_ne_bytes((number as i64).to_ne_bytes())
    } else {
        number as u64
    };
    format!("{value:08x}")
}

/// A string in single quotes, as `Data::Dumper` writes it.
fn dumper_quote(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// `Dumper($groups)` with `Indent = 1` and `Sortkeys = 1`, for a hash of arrays
/// of file names.
pub fn dump_groups(groups: &BTreeMap<String, Vec<String>>) -> String {
    if groups.is_empty() {
        return "$VAR1 = {};\n".to_owned();
    }
    let entries: Vec<String> = groups
        .iter()
        .map(|(key, files)| {
            if files.is_empty() {
                format!("  {} => []", dumper_quote(key))
            } else {
                let items: Vec<String> = files
                    .iter()
                    .map(|file| format!("    {}", dumper_quote(file)))
                    .collect();
                format!("  {} => [\n{}\n  ]", dumper_quote(key), items.join(",\n"))
            }
        })
        .collect();
    format!("$VAR1 = {{\n{}\n}};\n", entries.join(",\n"))
}

/// Decodes text as Perl's `:encoding(UTF-8)` layer does by default: bytes that
/// are not valid UTF-8 become `\xHH`.
pub fn decode_utf8_perlqq(bytes: &[u8]) -> String {
    let mut text = String::new();
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                text.push_str(valid);
                return text;
            }
            Err(error) => {
                let (valid, after) = rest.split_at(error.valid_up_to());
                text.push_str(std::str::from_utf8(valid).unwrap_or_default());
                let bad = error.error_len().unwrap_or(after.len()).max(1);
                let (invalid, valid_after) = after.split_at(bad.min(after.len()));
                for byte in invalid {
                    let _ = write!(text, "\\x{byte:02X}");
                }
                rest = valid_after;
            }
        }
    }
}

/// The number at the start of a string, as Perl reads a string in numeric
/// context: optional white space and sign, digits with an optional fraction,
/// and an optional exponent. Anything else gives 0.
fn numeric_prefix(text: &str) -> f64 {
    let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let bytes = text.as_bytes();
    let mut end = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    let digits_start = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    let mut mantissa_digits = end - digits_start;
    if bytes.get(end) == Some(&b'.') {
        let fraction_start = end + 1;
        let mut fraction_end = fraction_start;
        while bytes.get(fraction_end).is_some_and(u8::is_ascii_digit) {
            fraction_end += 1;
        }
        mantissa_digits += fraction_end - fraction_start;
        if mantissa_digits > 0 {
            end = fraction_end;
        }
    }
    if mantissa_digits == 0 {
        return 0.0;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut exponent_end = end + 1;
        if matches!(bytes.get(exponent_end), Some(b'+' | b'-')) {
            exponent_end += 1;
        }
        let exponent_digits_start = exponent_end;
        while bytes.get(exponent_end).is_some_and(u8::is_ascii_digit) {
            exponent_end += 1;
        }
        if exponent_end > exponent_digits_start {
            end = exponent_end;
        }
    }
    text[..end].parse().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testkit::legacy::perl;
    use zbm_testkit::{Rng, can_run, have_program, parity};

    #[test]
    fn paths() {
        assert_eq!(basename("/boot/vmlinuz-6.1"), "vmlinuz-6.1");
        assert_eq!(basename("a//"), "a");
        assert_eq!(basename("/"), "/");
        assert_eq!(dirname("/a/b/c"), "/a/b");
        assert_eq!(dirname("/a"), "/");
        assert_eq!(dirname("a"), ".");
        assert_eq!(dirname("a/b/"), "a");
    }

    #[test]
    fn text() {
        assert_eq!(
            split_whitespace(b"  7 .reloc 0000000c\n"),
            [b"".as_slice(), b"7", b".reloc", b"0000000c"]
        );
        assert_eq!(mark_current("%CURRENT-1"), "%{current}-1");
        assert_eq!(mark_current("%currentx"), "%currentx");
        assert_eq!(replace_current("v%{Current}", "3"), "v3");
        assert!(has_current("%{CURRENT}"));
        assert_eq!(strip_version_suffix("vmlinuz-6.1", "6.1"), "vmlinuz");
        assert_eq!(hex_of_numeric_text("305419896\n"), "12345678");
        assert_eq!(hex_of_numeric_text("-1"), "ffffffffffffffff");
        assert_eq!(hex_of_numeric_text(""), "00000000");
        assert_eq!(decode_utf8_perlqq(b"a\xffb"), "a\\xFFb");
    }

    #[test]
    fn dumper_matches_perl() {
        let mut parity = parity!("dumper_matches_perl");
        if !can_run(
            !parity.mode().needs_legacy() || have_program("perl"),
            "perl",
        ) {
            return;
        }
        let pieces = ["a", "/b", "3.1.0", "'", "\\", "_1", " ", "x"];
        let mut rng = Rng::from_env(12);
        for _ in 0..30 {
            let mut groups = BTreeMap::new();
            for _ in 0..rng.below(4) {
                let files: Vec<String> = (0..=rng.below(3)).map(|_| rng.join(&pieces, 4)).collect();
                groups.insert(rng.join(&pieces, 3), files);
            }
            let mut args = Vec::new();
            for (key, files) in &groups {
                args.push(key.clone());
                args.push(files.len().to_string());
                args.extend(files.iter().cloned());
            }
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let program = "use Data::Dumper; $Data::Dumper::Indent = 1; \
                           $Data::Dumper::Sortkeys = 1; $Data::Dumper::Purity = 1; my %g; \
                           while (@ARGV) { my $k = shift; my $n = shift; \
                           $g{$k} = [splice(@ARGV, 0, $n)]; } print Dumper(\\%g);";
            let output: String = parity.legacy(format!("{groups:?}"), || {
                perl(program, &arg_refs, b"").stdout_text()
            });
            assert_eq!(dump_groups(&groups), output, "groups {groups:?}");
        }
    }
}
