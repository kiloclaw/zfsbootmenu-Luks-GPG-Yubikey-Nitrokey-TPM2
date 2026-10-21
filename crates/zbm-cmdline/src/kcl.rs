//! Splitting, editing and joining kernel command lines.
//!
//! Each function takes and returns the text its Bash counterpart in
//! `zfsbootmenu-kcl.sh` reads and prints: a *tokenized* command line has one
//! argument per line.

use zbm_core::shell::{echo, read_lines};

/// Splits a command line into arguments, one per line, as `kcl_tokenize` does.
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

/// Removes arguments, as `kcl_suppress` does: an argument is removed when it
/// equals one of `removals`, or when its part before the first `=` does.
///
/// Like the `while read -r` loop of the legacy function, this drops a last
/// argument without a newline and trims blanks around each argument.
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

/// Adds arguments, as `kcl_append` does: the text is kept as it is, and each
/// non-empty argument is added as one line, even if it contains spaces.
pub fn append(text: &str, args: &[&str]) -> String {
    let mut result = text.to_owned();
    for arg in args.iter().filter(|arg| !arg.is_empty()) {
        result.push_str(&echo(arg));
    }
    result
}

/// Joins a tokenized command line with single spaces, as `kcl_assemble` does.
/// The result has no newline.
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

/// The tokenized form of ZFSBootMenu's own command line, as `_build_zbm_kcl`
/// builds it: the text of each source is tokenized separately and the results
/// are concatenated.
///
/// The sources are `/etc/cmdline`, the files `/etc/cmdline.d/*.conf` and
/// `/proc/cmdline`, in that order; see [`zbm_sources`].
pub fn build_zbm_kcl<S: AsRef<str>>(sources: &[S]) -> String {
    sources
        .iter()
        .map(|source| tokenize(source.as_ref()))
        .collect()
}

/// The files `_build_zbm_kcl` reads, in order, under `root` (normally `/`):
/// `etc/cmdline`, the `etc/cmdline.d/*.conf` files sorted by name, and
/// `proc/cmdline`. Files that cannot be read are skipped.
pub fn zbm_sources(root: &std::path::Path) -> Vec<String> {
    let mut paths = vec![root.join("etc/cmdline")];
    if let Ok(entries) = std::fs::read_dir(root.join("etc/cmdline.d")) {
        let mut confs: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().is_some_and(|ext| ext == "conf")
                    && path
                        .file_name()
                        .is_some_and(|name| !name.to_string_lossy().starts_with('.'))
            })
            .collect();
        // The initramfs runs in the C locale, where Bash sorts glob results by byte
        // value.
        confs.sort();
        paths.extend(confs);
    }
    paths.push(root.join("proc/cmdline"));
    paths
        .iter()
        .filter_map(|path| std::fs::read(path).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testutil::Rng;
    use zbm_testutil::legacy::{Bash, LOG_STUBS};

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
        let root = zbm_testutil::TempDir::new("zbm-cmdline-sources");
        root.write("etc/cmdline", "first");
        root.write("etc/cmdline.d/b.conf", "third");
        root.write("etc/cmdline.d/a.conf", "second");
        root.write("etc/cmdline.d/.hidden.conf", "hidden");
        root.write("etc/cmdline.d/other.txt", "other");
        root.write("proc/cmdline", "last");
        assert_eq!(
            zbm_sources(root.path()),
            ["first", "second", "third", "last"]
        );
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
        let mut rng = Rng::from_env(1);
        let lib = kcl_lib();
        for _ in 0..200 {
            let input = rng.join(PIECES, 12);
            let legacy = lib.call("kcl_tokenize", &[], input.as_bytes());
            assert_eq!(tokenize(&input), legacy.stdout_text(), "input {input:?}");
        }
    }

    #[test]
    fn suppress_append_assemble_match_the_library() {
        let mut rng = Rng::from_env(2);
        let lib = kcl_lib();
        let removals = ["root", "a", "key=v", "", "q", "-n"];
        for _ in 0..150 {
            let text = tokenize(&rng.join(PIECES, 10));
            let remove: Vec<&str> = (0..rng.below(3)).map(|_| *rng.choose(&removals)).collect();
            let legacy = lib.call("kcl_suppress", &remove, text.as_bytes());
            assert_eq!(
                suppress(&text, &remove),
                legacy.stdout_text(),
                "text {text:?}"
            );

            let add: Vec<&str> = (0..rng.below(3)).map(|_| *rng.choose(PIECES)).collect();
            let legacy = lib.call("kcl_append", &add, text.as_bytes());
            assert_eq!(append(&text, &add), legacy.stdout_text(), "text {text:?}");

            let legacy = lib.call("kcl_assemble", &[], text.as_bytes());
            assert_eq!(assemble(&text), legacy.stdout_text(), "text {text:?}");
        }
    }
}
