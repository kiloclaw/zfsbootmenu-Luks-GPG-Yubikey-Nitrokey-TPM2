//! Tests of file name globbing.

use super::*;
use zbm_testkit::legacy::perl;
use zbm_testkit::{Rng, TempDir, have_program, parity};

#[test]
fn words() {
    let words = |pattern: &str| -> Vec<String> {
        split_words(pattern.as_bytes())
            .into_iter()
            .map(|word| String::from_utf8(word).unwrap())
            .collect()
    };
    assert_eq!(words("a b"), ["a", "b"]);
    assert_eq!(words(" \"d e\" x\\ y"), ["d e", "x\\ y"]);
    assert_eq!(words("a \"b"), ["a", "a \"b"]);
    assert_eq!(words("\"\""), [""]);
    assert_eq!(words("\\\"b"), ["\"b"]);
}

#[test]
fn files() {
    let dir = TempDir::new("generate-zbm-glob");
    for name in ["Ab", "ab", "aB", ".h", "b", "x[1]"] {
        dir.write(name, "");
    }
    let base = dir.path().display().to_string();
    let names = |pattern: &str| -> Vec<String> {
        glob(&format!("{base}/{pattern}"))
            .into_iter()
            .map(|path| path[base.len() + 1..].to_owned())
            .collect()
    };
    assert_eq!(names("*"), ["Ab", "aB", "ab", "b", "x[1]"]);
    assert_eq!(names("A?"), ["Ab", "aB", "ab"]);
    assert_eq!(names(".*"), [".", "..", ".h"]);
    assert_eq!(names("[!a]*"), ["b", "x[1]"]);
    assert_eq!(names("x\\[1]"), ["x[1]"]);
    assert_eq!(names("{b,zz}"), ["b", "zz"]);
    assert_eq!(names("none*"), Vec::<String>::new());
    assert_eq!(glob("a{b"), ["a"]);
    assert_eq!(glob("\\*"), ["*"]);
}

fn perl_glob(pattern: &str, cwd: &std::path::Path) -> String {
    let program = "use File::Glob qw(:globally :nocase); chdir $ARGV[1] or die;\n\
                   print map { \"<$_>\" } glob($ARGV[0]);";
    perl(program, &[pattern, &cwd.to_string_lossy()], b"").stdout_text()
}

#[test]
fn matches_file_glob() {
    let mut parity = parity!("matches_file_glob");
    if parity.mode().needs_legacy() && !have_program("perl") {
        eprintln!("skipped: perl is missing");
        return;
    }
    let dir = TempDir::new("generate-zbm-glob-parity");
    for name in [
        "vmlinuz-6.1.0",
        "VMLINUZ-6.2",
        "vmlinuz-6.10_1.EFI",
        "Ab",
        "ab",
        ".hidden",
        "d e",
        "x[1]",
        "a{b",
        "q'x",
        "-",
        "]",
        "sub/f",
        "sub/G",
        "sub/.dot",
        "s2/f",
    ] {
        dir.write(name, "");
    }
    let pieces = [
        "*", "?", "[ab]", "[!a]", "[]a]", "[a-z]", "[A-Z]", "[a-", "[", "]", "{", "}", ",",
        "{a,b}", "{,}", "\\", "\\*", "'", "\"", " ", "  ", "/", "sub", "s*", "vmlinuz", "-", "6",
        ".", ".*", "EFI", "_", "x", "d", "e", "~", "q", "{}", "1",
    ];
    let mut rng = Rng::from_env(11);
    for _ in 0..400 {
        let pattern = rng.join(&pieces, 5);
        // `~` is the home directory, which differs between machines.
        let home = std::env::var("HOME").unwrap_or_default();
        let without_home = |text: String| {
            if home.is_empty() {
                text
            } else {
                text.replace(&home, "$HOME")
            }
        };
        let rust: String = glob_at(&pattern, dir.path())
            .iter()
            .map(|path| format!("<{path}>"))
            .collect();
        let legacy: String = parity.legacy(format!("{pattern:?}"), || {
            without_home(perl_glob(&pattern, dir.path()))
        });
        assert_eq!(without_home(rust), legacy, "pattern {pattern:?}");
    }
}
