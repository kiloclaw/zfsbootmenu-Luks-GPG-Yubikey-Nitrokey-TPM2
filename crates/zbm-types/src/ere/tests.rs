use std::process::Command;

use pretty_assertions::assert_eq;
use zbm_testkit::{Rng, have_program};

use super::translate;
use crate::{KernelSelector, Reason};

/// The texts each case is matched against.
const TEXTS: [&str; 12] = [
    "a", "ab", "aa", "d", "]", "-", "\n", "\\", "{", " ", "é", "aaa",
];

/// What Bash's `[[ text =~ pattern ]]` gives for `pattern` and each of
/// [`TEXTS`], as recorded with Bash 5.2 and glibc 2.39: `0` a match, `1`
/// none, `2` a pattern that is not valid.
const CASES: &[(&str, &str)] = &[
    ("a", "000111111110"),
    ("*a", "222222222222"),
    ("a**", "000000000000"),
    ("a{", "222222222222"),
    ("a{2", "222222222222"),
    ("a{,2}", "000000000000"),
    ("{1}", "222222222222"),
    ("a{1}{2}", "110111111110"),
    ("^*", "222222222222"),
    ("(*a)", "222222222222"),
    ("a|*b", "222222222222"),
    ("a)", "111111111111"),
    ("(a", "222222222222"),
    ("()", "000000000000"),
    ("a|", "000000000000"),
    ("|a", "000000000000"),
    ("[z-a]", "222222222222"),
    ("[]a]", "000101111110"),
    ("[^]a]", "101010000001"),
    ("[a-]", "000110111110"),
    ("[\\]", "111111101111"),
    ("\\d", "111011111111"),
    ("\\w", "000011111110"),
    ("\\<a", "000111111110"),
    ("a\\>", "010111111110"),
    ("\\`a", "000111111110"),
    ("a\\'", "010111111110"),
    ("\\1", "222222222222"),
    ("a\\", "222222222222"),
    ("[[:foo:]]", "222222222222"),
    ("[[.a.]]", "000111111110"),
    ("[[.space.]]", "222222222222"),
    ("[[=a=]]", "000111111110"),
    ("a{1,2,3}", "222222222222"),
    ("a{3,1}", "222222222222"),
    ("a{99999}", "222222222222"),
    ("a{32768}", "222222222222"),
    ("x$y", "111111111111"),
    ("a^", "111111111111"),
    ("$*", "222222222222"),
    ("a$*", "222222222222"),
    ("\\<*", "222222222222"),
    ("a+?", "000000000000"),
    ("[a", "222222222222"),
    ("a]", "111111111111"),
    ("{", "222222222222"),
    ("}", "111111111111"),
    ("a{}", "222222222222"),
    ("a{x}", "222222222222"),
    ("a{ 1}", "222222222222"),
    ("a{01}", "000111111110"),
    ("a{0}", "000000000000"),
    ("a{,}", "000000000000"),
    ("(a){0}d", "111011111111"),
    ("(^a)", "000111111110"),
    ("d(^a)", "111111111111"),
    ("\\{", "111111110111"),
    ("[[:alpha:]-z]", "222222222222"),
    ("[a-[:digit:]]", "222222222222"),
    ("[[:digit:]-]", "111110111111"),
    ("[--z]", "000000101110"),
    ("[%--]", "111110111111"),
    ("[--]", "111110111111"),
    ("[a-c-e]", "222222222222"),
    ("[[.-.]-z]", "000000101110"),
    ("[a-[.z.]]", "000011111110"),
    ("[[=a=]-c]", "222222222222"),
    ("[a-[=c=]]", "222222222222"),
    ("[[.].]]", "111101111111"),
    ("[[..]]", "222222222222"),
    ("[[:]", "222222222222"),
    ("[[:a]x:]]", "222222222222"),
    ("[[]", "111111111111"),
    ("[]", "222222222222"),
    ("[^]", "222222222222"),
    ("[]-a]", "000101111110"),
    ("\\n", "111111111111"),
    ("\\s", "111111011011"),
    ("\\S", "000000100100"),
    ("\\bd", "111011111111"),
    ("\\B", "100100000000"),
    ("\\W", "111100000001"),
    ("[[:space:]]", "111111011011"),
    ("[[:blank:]]", "111111111011"),
    ("[[:punct:]]", "111100100111"),
    ("[[:print:]]", "000000100010"),
    ("[[:cntrl:]]", "111111011111"),
    ("[[:xdigit:]]", "000011111110"),
    ("[[:upper:][:digit:]]", "111111111111"),
    ("[\\w]", "111111101111"),
    ("\\-", "111110111111"),
    ("é", "111111111101"),
    ("[a-é]", "000011110100"),
    ("é$", "111111111101"),
    ("[é]", "111111111101"),
    (".", "000000000000"),
    ("[^a]", "101000000001"),
    ("a(){1}", "000111111110"),
    ("x*{2}", "000000000000"),
    ("(|a)", "000000000000"),
    ("a||b", "000000000000"),
    ("^^a", "000111111110"),
    ("a$$", "010111111110"),
    ("(a|^d)", "000011111110"),
];

/// What the selector gives for `pattern` and each of `texts`, as Bash
/// writes it: `0` a match, `1` none, `2` a pattern that is not valid.
fn statuses(pattern: &str, texts: &[&str]) -> String {
    let Ok(selector) = pattern.parse::<KernelSelector>() else {
        return "2".repeat(texts.len());
    };
    texts
        .iter()
        .map(|text| if selector.matches(text) { '0' } else { '1' })
        .collect()
}

#[test]
fn patterns_mean_what_they_mean_to_bash() {
    for (pattern, expected) in CASES {
        assert_eq!(statuses(pattern, &TEXTS), *expected, "{pattern:?}");
    }
}

#[test]
fn the_cases_are_what_this_host_s_bash_gives() {
    if !have_program("bash") {
        return;
    }
    let patterns: Vec<&str> = CASES.iter().map(|(pattern, _)| *pattern).collect();
    let bash = bash_statuses(&patterns, &TEXTS);
    for ((pattern, expected), got) in CASES.iter().zip(bash) {
        assert_eq!(got, *expected, "{pattern:?}");
    }
}

#[test]
fn back_references_and_deep_nesting_are_refused() {
    for (pattern, why) in [
        ("(a)\\1", "it has a back-reference, which is not supported"),
        ("a\\", "it ends with '\\'"),
        ("(a", "it has a '(' that is not closed"),
    ] {
        let error = pattern.parse::<KernelSelector>().unwrap_err();
        assert_eq!(error.reason(), &Reason::Pattern(why), "{pattern:?}");
    }
    let deep = "(".repeat(300) + &")".repeat(300);
    assert!(deep.parse::<KernelSelector>().is_err());
    assert!(translate("(".repeat(200).as_bytes()).is_err());
    let starred = format!("a{}", "*".repeat(300));
    assert!(starred.parse::<KernelSelector>().is_err());
    // Long patterns that nest little are taken.
    assert!("a".repeat(5000).parse::<KernelSelector>().is_ok());
}

#[test]
fn the_selector_prints_as_it_was_set() {
    let selector: KernelSelector = "^/boot/vmlinuz-6\\.6".parse().unwrap();

    assert_eq!(selector.to_string(), "^/boot/vmlinuz-6\\.6");
    assert_eq!(selector.as_str(), "^/boot/vmlinuz-6\\.6");
    assert_eq!(selector, "^/boot/vmlinuz-6\\.6".parse().unwrap());
    assert!(selector.matches("/boot/vmlinuz-6.6.30"));
    assert!(!selector.matches("/boot/vmlinuz-6.12.9"));
    // An empty pattern matches every kernel, as it does in Bash.
    assert!(
        "".parse::<KernelSelector>()
            .unwrap()
            .matches("/boot/vmlinuz")
    );
}

/// The pieces random patterns are made of.
const PIECES: &[&str] = &[
    "a",
    "b",
    "d",
    "-",
    "]",
    "[",
    "^",
    "$",
    "(",
    ")",
    "|",
    "*",
    "+",
    "?",
    "{",
    "}",
    ",",
    "0",
    "1",
    "2",
    "\\",
    ".",
    ":",
    "=",
    "[:alpha:]",
    "[:digit:]",
    "[.a.]",
    "[=b=]",
    "w",
    "s",
    "<",
    ">",
    "é",
    "\\w",
    "\\b",
    "\\<",
    "\\>",
    "\\s",
    "[^",
    "{1}",
    "{,2}",
    "{1,}",
];

/// The texts random patterns are matched against.
const RANDOM_TEXTS: &[&str] = &[
    "",
    "a",
    "b",
    "ab",
    "ba",
    "aab",
    "a-b",
    "]",
    "[",
    "{",
    "}",
    "1",
    "a1",
    "a b",
    "-",
    "\\",
    "é",
    "d.d",
    "_w",
    "^",
    "$",
    "/boot/vmlinuz-6.12.9_1",
    ":",
    "=",
    "a,b",
];

/// Random patterns give what Bash gives, valid or not, but for the
/// back-references Bash takes and the selector refuses. The texts hold no
/// newline, where glibc's anchors have a quirk (see `ere.rs`).
/// `ZBM_TEST_SEED` picks other patterns.
#[test]
fn random_patterns_match_as_bash_matches_them() {
    if !have_program("bash") {
        return;
    }
    let mut rng = Rng::from_env(0x5e1e);
    let patterns: Vec<String> = (0..3000).map(|_| rng.join(PIECES, 8)).collect();
    let refs: Vec<&str> = patterns.iter().map(String::as_str).collect();
    let bash = bash_statuses(&refs, RANDOM_TEXTS);
    let mut compared = 0;
    let mut differ = Vec::new();
    for (pattern, expected) in patterns.iter().zip(bash) {
        let refused =
            translate(pattern.as_bytes()).is_err_and(|why| why.contains("back-reference"));
        if refused {
            continue;
        }
        compared += 1;
        let got = statuses(pattern, RANDOM_TEXTS);
        if got != expected {
            differ.push(format!("{pattern:?}: Bash {expected}, here {got}"));
        }
    }
    assert!(
        differ.is_empty(),
        "{} differ:\n{}",
        differ.len(),
        differ.join("\n")
    );
    assert!(compared > 2500, "only {compared} patterns compared");
}

/// What Bash gives for each of `patterns` and each of `texts`, a line of
/// statuses per pattern.
fn bash_statuses(patterns: &[&str], texts: &[&str]) -> Vec<String> {
    let script = r#"n=$1; shift; t=("${@:1:n}"); shift "$n"
for p; do for x in "${t[@]}"; do [[ $x =~ $p ]]; printf %s $?; done; echo; done"#;
    #[allow(
        clippy::disallowed_methods,
        reason = "the reference is the host's Bash, run as the legacy code ran it"
    )]
    let output = Command::new("bash")
        .env("LC_ALL", "C")
        .args(["-c", script, "bash", &texts.len().to_string()])
        .args(texts)
        .args(patterns)
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), patterns.len());
    lines
}

proptest::proptest! {
    /// Any pattern is taken or refused without a panic, and one taken
    /// matches any text without one (the fuzz target `kernel_selector`
    /// checks the same on arbitrary bytes).
    #[test]
    fn any_pattern_is_taken_or_refused(
        pattern in r"[\[\]()|*+?{}^$.\\a-d0-9,:=<>`'w-]{0,40}|\PC{0,40}",
        text in r"\PC{0,20}",
    ) {
        if let Ok(selector) = pattern.parse::<KernelSelector>() {
            proptest::prop_assert_eq!(selector.to_string(), pattern);
            let _ = selector.matches(&text);
        }
    }
}
