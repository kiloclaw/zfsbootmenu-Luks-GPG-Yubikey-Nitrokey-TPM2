//! LUKS2 token listing: the ID and type of each token on a volume.
//!
//! The tokens and their fields come from `zbm_crypt::Cryptsetup`, which reads
//! the JSON metadata of `cryptsetup luksDump --dump-json-metadata` with
//! `serde`, and the `luksDump` text for cryptsetup before 2.4. The `sed` and
//! `awk` emulation of the legacy code is gone; the tests below check that the
//! listing still matches the legacy listing on recorded volumes.

use zbm_crypt::Cryptsetup;
pub use zbm_crypt::Token;
use zbm_platform::System;

/// The LUKS2 tokens of a device or detached header: the ID and type of each,
/// from the JSON metadata when cryptsetup can print it.
///
/// The JSON metadata is the reliable source: with systemd's token plugins
/// installed, cryptsetup 2.8 (systemd 261) prints the text of every token
/// after the first to standard error, so the `luksDump` text misses them.
pub fn list_tokens(system: &System<'_>, device: &str) -> Vec<Token> {
    Cryptsetup::new(system.runner).list_tokens(device)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::legacy_luks;
    use zbm_platform::exec::{Output, Status};
    use zbm_testkit::fakes::FakeSystem;
    use zbm_testkit::legacy::shell_quote;
    use zbm_testkit::{parity, repo_root};

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
        tokens
            .iter()
            .map(|token| token.to_string() + "\n")
            .collect()
    }

    #[test]
    fn recorded_tokens_match_the_legacy_listing() {
        let mut parity = parity!("recorded_tokens_match_the_legacy_listing");
        let dir = repo_root().join("crates/zbm-luks/tests/fixtures");
        for (volume, status) in VOLUMES {
            let text_file = dir.join(format!("luksdump-{volume}.txt"));
            let legacy: String = parity.legacy(format!("{volume} {status}"), || {
                legacy_list(volume, status, &text_file.to_string_lossy())
            });
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
        let mut parity = parity!("texts_match_the_legacy_parser");
        for text in texts {
            let legacy: String = parity.legacy(format!("{text:?}"), || {
                let file = zbm_testkit::TempDir::new("zbm-luks-text");
                let path = file.write("dump.txt", &text);
                // Empty JSON output: the legacy code falls back to the text.
                let stub = format!(
                    r#"cryptsetup() {{ [ "$2" = --dump-json-metadata ] && return 1; cat {}; }}"#,
                    shell_quote(&path.to_string_lossy())
                );
                legacy_luks()
                    .code(&stub)
                    .call("luks_list_tokens", &["/dev/x"], b"")
                    .stdout_text()
            });
            assert_eq!(
                show(&zbm_crypt::token::from_dump_text(&text)),
                legacy,
                "{text:?}"
            );
        }
    }

    #[test]
    fn tokens_print_as_the_legacy_lines() {
        assert_eq!(Token::without_fields("3", "").to_string(), "3 ");
    }
}
