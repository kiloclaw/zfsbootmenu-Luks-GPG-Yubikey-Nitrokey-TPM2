//! Version comparison, the Rust counterpart of Perl's
//! `Sort::Versions::versioncmp`.
//!
//! `generate-zbm` orders kernels and image revisions with `versioncmp`, so the
//! Rust port compares versions the same way. The algorithm splits each string
//! into `-`, `.`, runs of digits and runs of other characters, and compares the
//! pieces in turn.

use std::cmp::Ordering;

/// One piece of a version string.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Piece<'a> {
    /// A single `-`.
    Dash,
    /// A single `.`.
    Dot,
    /// A run of ASCII digits.
    Digits(&'a str),
    /// A run of other characters.
    Other(&'a str),
}

/// Splits a version string as the regex `([-.]|\d+|[^-.\d]+)` does.
fn pieces(text: &str) -> Vec<Piece<'_>> {
    let bytes = text.as_bytes();
    let mut pieces = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'-' {
            pieces.push(Piece::Dash);
            index += 1;
        } else if byte == b'.' {
            pieces.push(Piece::Dot);
            index += 1;
        } else if byte.is_ascii_digit() {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            pieces.push(Piece::Digits(&text[start..index]));
        } else {
            let start = index;
            while index < bytes.len()
                && bytes[index] != b'-'
                && bytes[index] != b'.'
                && !bytes[index].is_ascii_digit()
            {
                index += 1;
            }
            pieces.push(Piece::Other(&text[start..index]));
        }
    }
    pieces
}

/// Compares two runs of digits numerically; both are known to have no leading
/// zero, so the longer run is the larger number and equal lengths compare byte
/// by byte.
fn compare_numeric(a: &str, b: &str) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// Compares two version strings as `Sort::Versions::versioncmp` does.
pub fn versioncmp(left: &str, right: &str) -> Ordering {
    let (a_pieces, b_pieces) = (pieces(left), pieces(right));
    let mut a = a_pieces.iter();
    let mut b = b_pieces.iter();
    loop {
        let (Some(a_piece), Some(b_piece)) = (a.next(), b.next()) else {
            // One side ran out: the longer list is greater, as `@A <=> @B`.
            return a_pieces.len().cmp(&b_pieces.len());
        };
        let order = match (a_piece, b_piece) {
            (Piece::Dash, Piece::Dash) | (Piece::Dot, Piece::Dot) => Ordering::Equal,
            (Piece::Dash, _) => return Ordering::Less,
            (_, Piece::Dash) => return Ordering::Greater,
            (Piece::Dot, _) => return Ordering::Less,
            (_, Piece::Dot) => return Ordering::Greater,
            (Piece::Digits(a), Piece::Digits(b)) => {
                if a.starts_with('0') || b.starts_with('0') {
                    a.cmp(b)
                } else {
                    compare_numeric(a, b)
                }
            }
            // Any pair involving an "other" run is compared as upper-case strings; digit
            // runs are unchanged by upper-casing, so this also covers a digit run against
            // an "other" run.
            (a, b) => upper(a).cmp(&upper(b)),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
}

/// The ASCII upper-case of a piece's text, as Perl's `uc` leaves version
/// strings.
fn upper(piece: &Piece) -> String {
    match piece {
        Piece::Digits(text) | Piece::Other(text) => text.to_ascii_uppercase(),
        Piece::Dash => "-".to_owned(),
        Piece::Dot => ".".to_owned(),
    }
}

/// Sorts version strings with [`versioncmp`], keeping equal ones in their
/// original order, like Perl's `sort`.
pub fn sort_versions(versions: &mut [String]) {
    versions.sort_by(|a, b| versioncmp(a, b));
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testkit::legacy::perl;
    use zbm_testkit::{Rng, have_program, parity};

    /// Perl's `versioncmp`: -1, 0 or 1.
    fn perl_cmp(a: &str, b: &str) -> i8 {
        let output = perl(
            "use Sort::Versions; print versioncmp($ARGV[0], $ARGV[1])",
            &[a, b],
            b"",
        );
        assert!(output.status.is_success(), "{}", output.stderr_text());
        match output.stdout_text().trim() {
            "-1" => -1,
            "0" => 0,
            "1" => 1,
            other => panic!("unexpected versioncmp output {other:?}"),
        }
    }

    #[test]
    fn known_cases() {
        assert_eq!(versioncmp("1.0", "1.0.0"), Ordering::Less);
        assert_eq!(versioncmp("1.2", "1.10"), Ordering::Less);
        assert_eq!(versioncmp("1.2a", "1.2"), Ordering::Greater);
        assert_eq!(versioncmp("6.1.0", "6.1.0_1"), Ordering::Less);
        assert_eq!(versioncmp("1.01", "1.1"), Ordering::Less);
        assert_eq!(versioncmp("2.4.4", "2.4.4-1-generic"), Ordering::Less);
        assert_eq!(versioncmp("R1", "r1"), Ordering::Equal);
        assert_eq!(versioncmp("1.2.3_10", "1.2.3_9"), Ordering::Greater);
        assert_eq!(versioncmp("", "0"), Ordering::Less);
        assert_eq!(versioncmp("1a1", "1.a1"), Ordering::Greater);
        assert_eq!(versioncmp("1.0", "1.0"), Ordering::Equal);
    }

    #[test]
    fn sort_is_stable() {
        let mut versions: Vec<String> = ["1.10", "1.2", "1.2", "1.1"]
            .iter()
            .map(|v| (*v).to_owned())
            .collect();
        sort_versions(&mut versions);
        assert_eq!(versions, ["1.1", "1.2", "1.2", "1.10"]);
    }

    #[test]
    fn matches_perl_on_generated_versions() {
        let mut parity = parity!("matches_perl_on_generated_versions");
        if parity.mode().needs_legacy() && !have_program("perl") {
            eprintln!("skipped: perl is missing");
            return;
        }
        let mut rng = Rng::from_env(6);
        let pieces = [
            "0", "1", "2", "9", "10", "01", "007", "100", "a", "b", "A", "rc", "generic", "-", ".",
            "_", "~", "", "x86", "1a", "0x",
        ];
        for _ in 0..500 {
            let a = rng.join(&pieces, 6);
            let b = if rng.below(2) == 0 {
                rng.join(&pieces, 6)
            } else {
                // Often compare near-equal strings, to exercise the tie-breaking.
                let mut edited = a.clone();
                if !edited.is_empty() {
                    let at = rng.below(edited.len());
                    edited.insert(at, *rng.choose(&['1', '0', '.', '-', 'a']));
                }
                edited
            };
            let legacy: i8 = parity.legacy(format!("{a:?} {b:?}"), || perl_cmp(&a, &b));
            assert_eq!(
                versioncmp(&a, &b),
                legacy.cmp(&0),
                "versioncmp({a:?}, {b:?})"
            );
        }
    }
}
