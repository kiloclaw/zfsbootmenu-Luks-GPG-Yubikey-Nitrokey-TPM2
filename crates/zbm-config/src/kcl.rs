//! Splitting, editing and joining kernel command lines.
//!
//! Each function takes and returns the text its Bash counterpart in
//! `zfsbootmenu-kcl.sh` reads and prints: a *tokenized* command line has one
//! argument per line.

use zbm_core::shell::{echo, read_lines};

/// Splits a command line into arguments, one per line.
///
/// Arguments are separated by spaces, tabs and newlines, except within double
/// quotes: words of a quoted argument are joined by a single space. When a
/// quote is never closed, the last argument has no newline.
pub fn tokenize(input: &str) -> String {
    let mut text = String::new();
    let mut quoted = false;
    let mut at_start = true;
    for field in input
        .split([' ', '\t', '\n'])
        .filter(|field| !field.is_empty())
    {
        if field.matches('"').count() % 2 == 1 {
            quoted = !quoted;
        }
        if !at_start {
            text.push(' ');
        }
        text.push_str(field);
        if quoted {
            at_start = false;
        } else {
            at_start = true;
            text.push('\n');
        }
    }
    text
}

/// Removes arguments: an argument is removed when it equals one of
/// `removals`, or when its part before the first `=` does.
///
/// Each argument is read as `while read -r` reads it: a last argument without
/// a newline is dropped, and blanks around each argument are trimmed.
pub fn suppress(text: &str, removals: &[&str]) -> String {
    read_lines(text)
        .iter()
        .filter(|arg| {
            let key = arg.split_once('=').map_or(arg.as_str(), |(key, _)| key);
            !removals
                .iter()
                .any(|&removal| arg.as_str() == removal || key == removal)
        })
        .map(|arg| echo(arg))
        .collect()
}

/// Adds arguments: the text is kept as it is, and each non-empty argument is
/// added as one line, even if it contains spaces.
pub fn append(text: &str, args: &[&str]) -> String {
    let mut result = text.to_owned();
    for arg in args.iter().filter(|arg| !arg.is_empty()) {
        result.push_str(&echo(arg));
    }
    result
}

/// Joins a tokenized command line with single spaces. The result has no
/// newline.
pub fn assemble(text: &str) -> String {
    lines(text).join(" ")
}

/// The lines of a tokenized command line as awk reads them: a last line without
/// a newline counts, an empty text has no lines.
pub fn lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

/// The arguments of a command line, as [`tokenize`] splits them.
pub fn arguments(input: &str) -> Vec<String> {
    lines(&tokenize(input))
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The tokenized form of ZFSBootMenu's own command line: the text of each
/// source is tokenized separately and the results are concatenated.
///
/// The sources are `/etc/cmdline`, the files `/etc/cmdline.d/*.conf` and
/// `/proc/cmdline`, in that order, which `zbm_startup::preinit::zbm_sources`
/// reads.
pub fn build_zbm_kcl<S: AsRef<str>>(sources: &[S]) -> String {
    sources
        .iter()
        .map(|source| tokenize(source.as_ref()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testkit::legacy::{Bash, LOG_STUBS};
    use zbm_testkit::{Rng, parity};

    fn kcl_lib() -> Bash {
        Bash::new()
            .code(LOG_STUBS)
            .source("zfsbootmenu/lib/zfsbootmenu-kcl.sh")
    }

    #[test]
    fn tokenize_keeps_quoted_words_together() {
        assert_eq!(tokenize("a b\tc\n"), "a\nb\nc\n");
        assert_eq!(tokenize("  a   b  "), "a\nb\n");
        assert_eq!(tokenize("x=\"a  b\" y"), "x=\"a b\"\ny\n");
        assert_eq!(tokenize("x=\"a\nb\" y"), "x=\"a b\"\ny\n");
        assert_eq!(tokenize("x=\"\"\"y z"), "x=\"\"\"y z");
        assert_eq!(tokenize("\"open quote"), "\"open quote");
        assert_eq!(tokenize(""), "");
        assert_eq!(tokenize("a\rb"), "a\rb\n");
    }

    #[test]
    fn suppress_matches_whole_arguments_or_keys() {
        let text = "root=zfs\nquiet\nrootwait\nspl_hostid=1\n\"open";
        assert_eq!(suppress(text, &["root", "spl_hostid"]), "quiet\nrootwait\n");
        assert_eq!(
            suppress(text, &["quiet"]),
            "root=zfs\nrootwait\nspl_hostid=1\n"
        );
        assert_eq!(suppress("-n\nx\n", &[]), "x\n");
    }

    #[test]
    fn append_and_assemble() {
        assert_eq!(append("a\n", &["b c", "", "d"]), "a\nb c\nd\n");
        assert_eq!(append("\"open", &["x"]), "\"openx\n");
        assert_eq!(assemble("a\nb c\nd\n"), "a b c d");
        assert_eq!(assemble("a\n\nb"), "a  b");
        assert_eq!(assemble(""), "");
        assert_eq!(arguments("a \"b c\" d"), ["a", "\"b c\"", "d"]);
    }

    #[test]
    fn zbm_kcl_concatenates_sources() {
        assert_eq!(build_zbm_kcl(&["a \"b", "c d"]), "a\n\"bc\nd\n");
    }

    const PIECES: &[&str] = &[
        "a",
        "b",
        "root=x",
        "key=v",
        "=",
        "\"",
        "\"\"",
        "q=\"x y\"",
        " ",
        "  ",
        "\t",
        "\n",
        "-n",
        "-e",
        "spl_hostid=1",
        "x\\y",
        "'",
        "#",
        "%{parent}",
        "&",
    ];

    #[test]
    fn tokenize_matches_kcl_tokenize() {
        let mut parity = parity!("tokenize_matches_kcl_tokenize");
        let mut rng = Rng::from_env(1);
        for _ in 0..200 {
            let input = rng.join(PIECES, 12);
            let legacy: String = parity.legacy(format!("{input:?}"), || {
                kcl_lib()
                    .call("kcl_tokenize", &[], input.as_bytes())
                    .stdout_text()
            });
            assert_eq!(tokenize(&input), legacy, "input {input:?}");
        }
    }

    #[test]
    fn suppress_append_assemble_match_the_library() {
        let mut parity = parity!("suppress_append_assemble_match_the_library");
        let mut rng = Rng::from_env(2);
        let removals = ["root", "a", "key=v", "", "q", "-n"];
        for _ in 0..150 {
            let text = tokenize(&rng.join(PIECES, 10));
            let remove: Vec<&str> = (0..rng.below(3)).map(|_| *rng.choose(&removals)).collect();
            let add: Vec<&str> = (0..rng.below(3)).map(|_| *rng.choose(PIECES)).collect();
            let input = format!("text {text:?}, remove {remove:?}, add {add:?}");
            let (suppressed, appended, assembled): (String, String, String) =
                parity.legacy(input, || {
                    let lib = kcl_lib();
                    let run = |function, args: &[&str]| {
                        lib.call(function, args, text.as_bytes()).stdout_text()
                    };
                    (
                        run("kcl_suppress", &remove),
                        run("kcl_append", &add),
                        run("kcl_assemble", &[]),
                    )
                });

            assert_eq!(suppress(&text, &remove), suppressed, "text {text:?}");
            assert_eq!(append(&text, &add), appended, "text {text:?}");
            assert_eq!(assemble(&text), assembled, "text {text:?}");
        }
    }
}
