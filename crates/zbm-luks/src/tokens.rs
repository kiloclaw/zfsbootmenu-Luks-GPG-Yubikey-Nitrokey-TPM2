//! LUKS2 token metadata: `luks_list_tokens` and the JSON extractors
//! `luks_json_str`, `luks_json_arr` and `luks_json_bool`.
//!
//! The extractors read single values from the compact JSON of
//! `cryptsetup token export`, whose keys are unique, exactly as the legacy
//! `sed` expressions do: for each line, the last occurrence of the key wins.

use std::fmt;

use zbm_core::exec::{Command, Input, Stream};
use zbm_core::shell::command_substitution;
use zbm_system::System;

/// A LUKS2 token: its ID and its type, such as `systemd-tpm2`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    /// The token ID.
    pub id: String,
    /// The token type.
    pub kind: String,
}

impl fmt::Display for Token {
    /// The line `luks_list_tokens` prints: `<id> <type>`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.id, self.kind)
    }
}

/// The lines `sed` reads from a here-string: the text and a final newline.
fn sed_lines(text: &str) -> std::str::Split<'_, char> {
    text.split('\n')
}

/// The value a `sed -n 's/.*"<key>": *<open>\(...\)<close>.*/\1/p'` line
/// yields: the last occurrence of `"<key>":` and spaces followed by `value`'s
/// match.
fn last_match<'a>(
    line: &'a str,
    key: &str,
    value: impl Fn(&'a str) -> Option<&'a str>,
) -> Option<&'a str> {
    let pattern = format!("\"{key}\":");
    let starts: Vec<usize> = line.match_indices(&pattern).map(|(at, _)| at).collect();
    starts.iter().rev().find_map(|&at| {
        let rest = line[at + pattern.len()..].trim_start_matches(' ');
        value(rest)
    })
}

/// The output of `sed -n` with substitution `matcher`, as `$(...)` captures
/// it.
fn sed(json: &str, key: &str, value: impl Fn(&str) -> Option<&str>) -> String {
    let mut output = String::new();
    for line in sed_lines(json) {
        if let Some(found) = last_match(line, key, &value) {
            output.push_str(found);
            output.push('\n');
        }
    }
    command_substitution(&output).to_owned()
}

/// The string value of `key` (`luks_json_str`), unless it is empty.
pub fn json_str(json: &str, key: &str) -> Option<String> {
    let value = sed(json, key, |rest| {
        let rest = rest.strip_prefix('"')?;
        rest.find('"').map(|end| &rest[..end])
    });
    (!value.is_empty()).then_some(value)
}

/// The contents of the array `key` (`luks_json_arr`), without quotes and
/// spaces, such as `1,2`; `None` if the JSON has no such array.
pub fn json_arr(json: &str, key: &str) -> Option<String> {
    let value = sed(json, key, |rest| {
        let rest = rest.strip_prefix('[')?;
        rest.find(']').map(|end| &rest[..end])
    });
    // `grep -q "\"${2}\": *\["`
    let present = sed_lines(json)
        .any(|line| last_match(line, key, |rest| rest.strip_prefix('[').map(|_| "")).is_some());
    present.then(|| value.replace(['"', ' '], ""))
}

/// The boolean value of `key` (`luks_json_bool`), `true` or `false`.
pub fn json_bool(json: &str, key: &str) -> Option<String> {
    let value = sed(json, key, |rest| {
        ["true", "false"]
            .into_iter()
            .find(|word| rest.starts_with(word))
    });
    (!value.is_empty()).then_some(value)
}

/// Whether `c` is in awk's `[[:space:]]`.
fn awk_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

/// Whether `line` matches `^[[:space:]]*"<name>"[[:space:]]*:`, where a
/// `None` name stands for `[0-9]+`; returns the name.
fn awk_member<'a>(line: &'a str, name: Option<&str>) -> Option<&'a str> {
    let rest = line.trim_start_matches(awk_space).strip_prefix('"')?;
    let end = rest.find('"')?;
    let found = &rest[..end];
    let matches = match name {
        Some(name) => found == name,
        None => !found.is_empty() && found.bytes().all(|byte| byte.is_ascii_digit()),
    };
    let after = rest[end + 1..].trim_start_matches(awk_space);
    (matches && after.starts_with(':')).then_some(found)
}

/// The type in a `"type": "..."` line, as the two `sub` calls of the legacy
/// awk program cut it out.
fn awk_type(line: &str) -> &str {
    let mut text = line;
    if let Some(colon) = text.find(':') {
        let rest = text[colon + 1..].trim_start_matches(awk_space);
        if let Some(rest) = rest.strip_prefix('"') {
            text = rest;
        }
    }
    text.find('"').map_or(text, |quote| &text[..quote])
}

/// The tokens in the pretty-printed JSON of
/// `cryptsetup luksDump --dump-json-metadata`, as the legacy awk program finds
/// them by tracking the depth of braces; `None` for empty output, for which
/// the program fails.
pub fn parse_json_metadata(text: &str) -> Option<Vec<Token>> {
    if text.is_empty() {
        return None;
    }
    let records = text.strip_suffix('\n').unwrap_or(text);
    let mut tokens = Vec::new();
    let (mut depth, mut in_tokens, mut want) = (0_i64, false, false);
    let mut id = "";
    for line in records.split('\n') {
        if depth == 1 && awk_member(line, Some("tokens")).is_some() {
            in_tokens = true;
        }
        if in_tokens
            && depth == 2
            && let Some(found) = awk_member(line, None)
        {
            id = found;
            want = true;
        }
        if in_tokens && want && depth == 3 && awk_member(line, Some("type")).is_some() {
            tokens.push(Token {
                id: id.to_owned(),
                kind: awk_type(line).to_owned(),
            });
            want = false;
        }
        let opens = line.matches('{').count();
        let closes = line.matches('}').count();
        depth += i64::try_from(opens).unwrap_or(i64::MAX) - i64::try_from(closes).unwrap_or(0);
        if in_tokens && depth <= 1 {
            in_tokens = false;
        }
    }
    Some(tokens)
}

/// The tokens in the text of `cryptsetup luksDump`, for cryptsetup before 2.4,
/// which cannot print the JSON metadata.
pub fn parse_dump_text(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut in_tokens = false;
    let records = text.strip_suffix('\n').unwrap_or(text);
    for line in records.split('\n') {
        if line.starts_with("Tokens:") {
            in_tokens = true;
            continue;
        }
        if line.chars().next().is_some_and(|c| !awk_space(c)) {
            in_tokens = false;
        }
        let Some(rest) = line.strip_prefix("  ") else {
            continue;
        };
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if !in_tokens || digits == 0 || !rest[digits..].starts_with(": ") {
            continue;
        }
        // `sub(":", "", $1); print $1, $2`
        let mut fields = line.split(awk_space).filter(|field| !field.is_empty());
        let first = fields.next().unwrap_or_default().replacen(':', "", 1);
        tokens.push(Token {
            id: first,
            kind: fields.next().unwrap_or_default().to_owned(),
        });
    }
    tokens
}

/// What `cryptsetup luksDump [options] <device>` prints, without its errors.
fn luks_dump(system: &System<'_>, options: &[&str], device: &str) -> String {
    let command = Command::new("cryptsetup")
        .stdin(Input::Null)
        .arg("luksDump")
        .args(options)
        .arg(device)
        .stderr(Stream::Null);
    system
        .runner
        .run(&command)
        .map(|output| output.stdout_text())
        .unwrap_or_default()
}

/// The LUKS2 tokens of a device or detached header (`luks_list_tokens`).
///
/// The JSON metadata is the reliable source: with systemd's token plugins
/// installed, cryptsetup 2.8 (systemd 261) prints the text of every token
/// after the first to standard error, so the `luksDump` text misses them.
pub fn list_tokens(system: &System<'_>, device: &str) -> Vec<Token> {
    let json = luks_dump(system, &["--dump-json-metadata"], device);
    if let Some(tokens) = parse_json_metadata(&json) {
        return tokens;
    }
    parse_dump_text(&luks_dump(system, &[], device))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::legacy_luks;
    use zbm_core::exec::{Output, Status};
    use zbm_testutil::fakes::FakeSystem;
    use zbm_testutil::legacy::shell_quote;
    use zbm_testutil::{Rng, repo_root};

    fn fixture(name: &str) -> String {
        let path = repo_root()
            .join("crates/zbm-luks/tests/fixtures")
            .join(name);
        std::fs::read_to_string(path).unwrap()
    }

    /// The fixtures as `cryptsetup` prints them: the JSON metadata (with its
    /// exit status) and the text, by volume.
    const VOLUMES: [(&str, u8); 5] = [
        ("tpm2", 0),
        ("three", 0),
        ("gap", 0),
        ("plain", 0),
        ("luks1", 1),
    ];

    /// A machine whose `cryptsetup luksDump` prints the fixtures of `volume`.
    fn dump_machine(volume: &str, status: u8, text: Option<String>) -> FakeSystem {
        let fake = FakeSystem::new();
        let json = fixture(&format!("luksdump-{volume}.json"));
        let text = text.unwrap_or_else(|| fixture(&format!("luksdump-{volume}.txt")));
        fake.runner.on(move |command| {
            let args: Vec<String> = command
                .get_args()
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            let json_wanted = args.iter().any(|arg| arg == "--dump-json-metadata");
            let (code, stdout) = if json_wanted {
                (status, json.clone())
            } else {
                (0, text.clone())
            };
            Some(Ok(Output::new(Status::exited(code), stdout)))
        });
        fake
    }

    /// The legacy function with a `cryptsetup` that prints the same.
    fn legacy_list(volume: &str, status: u8, text_file: &str) -> String {
        let dir = repo_root().join("crates/zbm-luks/tests/fixtures");
        let json = dir.join(format!("luksdump-{volume}.json"));
        let stub = format!(
            r#"cryptsetup() {{
                 if [ "$2" = --dump-json-metadata ]; then cat {}; return {status}; fi
                 cat {}
               }}"#,
            shell_quote(&json.to_string_lossy()),
            shell_quote(text_file),
        );
        legacy_luks()
            .code(&stub)
            .call("luks_list_tokens", &["/dev/x"], b"")
            .stdout_text()
    }

    fn show(tokens: &[Token]) -> String {
        tokens.iter().map(|token| format!("{token}\n")).collect()
    }

    #[test]
    fn recorded_tokens_match_the_legacy_listing() {
        let dir = repo_root().join("crates/zbm-luks/tests/fixtures");
        for (volume, status) in VOLUMES {
            let text_file = dir.join(format!("luksdump-{volume}.txt"));
            let legacy = legacy_list(volume, status, &text_file.to_string_lossy());
            let fake = dump_machine(volume, status, None);
            let rust = list_tokens(&fake.system(), "/dev/x");
            assert_eq!(show(&rust), legacy, "{volume}");
        }
        let tokens = |volume: &str, status: u8| {
            show(&list_tokens(
                &dump_machine(volume, status, None).system(),
                "/dev/x",
            ))
        };
        assert_eq!(tokens("tpm2", 0), "0 systemd-tpm2\n");
        assert_eq!(
            tokens("three", 0),
            "0 systemd-tpm2\n1 systemd-fido2\n2 clevis\n"
        );
        assert_eq!(tokens("gap", 0), "0 systemd-tpm2\n2 clevis\n");
        assert_eq!(tokens("plain", 0), "");
        // LUKS1 has no JSON metadata, and its text has no tokens.
        assert_eq!(tokens("luks1", 1), "");
    }

    #[test]
    fn the_text_is_read_when_there_is_no_json() {
        // cryptsetup before 2.4: no JSON, so the text of a volume with tokens.
        let fake = FakeSystem::new();
        let text = fixture("luksdump-three.txt");
        fake.runner.on(move |command| {
            let json = command
                .get_args()
                .iter()
                .any(|arg| arg == "--dump-json-metadata");
            Some(Ok(if json {
                Output::new(Status::exited(1), "")
            } else {
                Output::new(Status::exited(0), text.clone())
            }))
        });
        assert_eq!(
            show(&list_tokens(&fake.system(), "/dev/x")),
            "0 systemd-tpm2\n1 systemd-fido2\n2 clevis\n"
        );
        assert_eq!(
            fake.runner.lines(),
            [
                "cryptsetup luksDump --dump-json-metadata /dev/x",
                "cryptsetup luksDump /dev/x"
            ]
        );
    }

    #[test]
    fn texts_match_the_legacy_parser() {
        let mut texts: Vec<String> = VOLUMES
            .iter()
            .map(|(volume, _)| fixture(&format!("luksdump-{volume}.txt")))
            .collect();
        texts.extend(
            [
                "Tokens:\n  0: a b\n  1:\n  2: \n\t3: x\n  4x: y\n  05: z\nDigests:\n  6: w\n",
                "Tokens:\n  0: last",
                "Tokens:\n\n  7: after-empty\n \n  8:  spaced   out\n",
                "",
            ]
            .map(str::to_owned),
        );
        for text in texts {
            let file = zbm_testutil::TempDir::new("zbm-luks-text");
            let path = file.write("dump.txt", &text);
            // Empty JSON output: the legacy code falls back to the text.
            let stub = format!(
                r#"cryptsetup() {{ [ "$2" = --dump-json-metadata ] && return 1; cat {}; }}"#,
                shell_quote(&path.to_string_lossy())
            );
            let legacy = legacy_luks()
                .code(&stub)
                .call("luks_list_tokens", &["/dev/x"], b"")
                .stdout_text();
            assert_eq!(show(&parse_dump_text(&text)), legacy, "{text:?}");
        }
    }

    #[test]
    fn json_variants_match_the_legacy_parser() {
        let jsons = [
            "{\n  \"tokens\":{\n    \"0\":{\n      \"type\":\"a\"\n    }\n  }\n}\n",
            // Compact JSON: one line, so nothing is found at the right depth.
            "{\"tokens\":{\"0\":{\"type\":\"a\"}}}",
            "{\n \"tokens\" : {\n  \"12\" : {\n   \"type\" : \"x y\"\n  },\n  \"13\":{\n   \
             \"keyslots\":[],\n   \"type\": 5\n  },\n  \"14\":{\n   \"type\":\"{\"\n  }\n }\n}\n",
            "{\n  \"keyslots\":{\n    \"0\":{\n      \"type\":\"luks2\"\n    }\n  },\n  \
             \"tokens\":{\n  }\n}\n",
            "{\n  \"tokens\":{\n    \"1\":{\n      \"jwe\":{\n        \"type\":\"no\"\n      \
             },\n      \"type\":\"clevis\"\n    }\n  }\n}",
            "\n",
            "x",
        ];
        for json in jsons {
            let dir = zbm_testutil::TempDir::new("zbm-luks-json");
            let path = dir.write("dump.json", json);
            let stub = format!(
                r#"cryptsetup() {{ [ "$2" = --dump-json-metadata ] && cat {}; }}"#,
                shell_quote(&path.to_string_lossy())
            );
            let legacy = legacy_luks()
                .code(&stub)
                .call("luks_list_tokens", &["/dev/x"], b"")
                .stdout_text();
            let rust = parse_json_metadata(json).unwrap_or_default();
            assert_eq!(show(&rust), legacy, "{json:?}");
        }
        assert_eq!(parse_json_metadata(""), None);
    }

    /// The legacy extractor `function` of `key` in `json`, as `Some` output
    /// when it succeeds.
    fn legacy_extract(function: &str, json: &str, key: &str) -> Option<String> {
        let output = legacy_luks().call(function, &[json, key], b"");
        let text = output.stdout_text();
        output
            .status
            .success()
            .then(|| text.strip_suffix('\n').unwrap_or(&text).to_owned())
    }

    fn assert_same_extracts(json: &str, keys: &[&str]) {
        for key in keys {
            for (function, rust) in [
                ("luks_json_str", json_str(json, key)),
                ("luks_json_arr", json_arr(json, key)),
                ("luks_json_bool", json_bool(json, key)),
            ] {
                assert_eq!(
                    rust,
                    legacy_extract(function, json, key),
                    "{function} {key} {json:?}"
                );
            }
        }
    }

    #[test]
    fn recorded_token_values_match_the_legacy_extractors() {
        let keys = [
            "type",
            "keyslots",
            "tpm2-blob",
            "tpm2-pcrs",
            "tpm2-pcr-bank",
            "tpm2-policy-hash",
            "tpm2-pin",
            "tpm2_salt",
            "tpm2_srk",
            "tpm2-primary-alg",
            "fido2-credential",
            "fido2-salt",
            "fido2-rp",
            "fido2-clientPin-required",
            "fido2-up-required",
            "fido2-uv-required",
            "jwe",
            "protected",
            "missing",
        ];
        for name in [
            "token-tpm2.json",
            "token-tpm2-pin.json",
            "token-fido2.json",
            "token-clevis.json",
        ] {
            assert_same_extracts(&fixture(name), &keys);
        }
        let pin = fixture("token-tpm2-pin.json");
        assert_eq!(json_str(&pin, "type").as_deref(), Some("systemd-tpm2"));
        assert_eq!(json_arr(&pin, "tpm2-pcrs").as_deref(), Some("7"));
        assert_eq!(json_bool(&pin, "tpm2-pin").as_deref(), Some("true"));
        assert_eq!(json_str(&pin, "tpm2-pcr-bank").as_deref(), Some("sha256"));
        assert_eq!(json_arr(&pin, "keyslots").as_deref(), Some("1"));
        let fido2 = fixture("token-fido2.json");
        assert_eq!(
            json_str(&fido2, "fido2-rp").as_deref(),
            Some("io.systemd.cryptsetup")
        );
        assert_eq!(
            json_bool(&fido2, "fido2-up-required").as_deref(),
            Some("true")
        );
    }

    #[test]
    fn random_json_matches_the_legacy_extractors() {
        let mut rng = Rng::from_env(0x7015);
        let pieces = [
            "\"k\":",
            "\"k\": ",
            "\"k\":  ",
            "\"v\"",
            "\"\"",
            "[",
            "]",
            "\"a\", \"b\"",
            "1, 2",
            "true",
            "false",
            "truest",
            " ",
            ",",
            "{",
            "}",
            "\n",
            "\"k2\":",
            "x",
        ];
        for _ in 0..80 {
            let json = rng.join(&pieces, 10);
            assert_same_extracts(&json, &["k", "k2"]);
        }
        assert_same_extracts(
            "{\"k\":\"first\",\"k\":\"last\"}\n{\"k\": [ \"1\" , \"2\" ]}\n\"k\":true \"k\":false",
            &["k"],
        );
        assert_eq!(
            json_arr("{\"k\": [ \"1\" , \"2\" ]}", "k").as_deref(),
            Some("1,2")
        );
        assert_eq!(json_arr("{\"k\":[]}", "k").as_deref(), Some(""));
        assert_eq!(json_arr("{\"k\":\"[]\"}", "k"), None);
    }

    #[test]
    fn tokens_print_as_the_legacy_lines() {
        let token = Token {
            id: "3".to_owned(),
            kind: String::new(),
        };
        assert_eq!(token.to_string(), "3 ");
    }
}
