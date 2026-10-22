//! The GnuPG keys of the edge suite.

use std::path::Path;

use super::{Lab, run, try_run};

/// The GnuPG keys of the edge suite: two key pairs, a
/// key file encrypted to both, a home with only the second secret key, and
/// a symmetric key file.
pub struct Gnupg {
    /// The key file encrypted to both keys (`good.key.gpg`).
    pub encrypted: String,
    /// The home with only the second secret key (`onlyb`).
    pub only_b: String,
    /// The symmetric key file, passphrase `symp` (`good.key.sym.gpg`).
    pub symmetric: String,
}

impl Gnupg {
    /// Creates the keys and files in `lab` for `good_key`.
    pub fn new(lab: &Lab, good_key: &Path) -> Gnupg {
        let generator = lab.text("gen");
        std::fs::create_dir(&generator).unwrap();
        run("chmod", &["700", &generator], b"", &[]);
        let home = [("GNUPGHOME", generator.as_str())];
        let loopback = [
            "-q",
            "--batch",
            "--pinentry-mode",
            "loopback",
            "--passphrase",
        ];
        for name in ["a", "b"] {
            let passphrase = format!("pass{name}");
            let user = format!("K{name} <{name}@t>");
            run(
                "gpg",
                &[
                    &loopback[..],
                    &[
                        &passphrase,
                        "--quick-gen-key",
                        &user,
                        "ed25519",
                        "sign",
                        "never",
                    ],
                ]
                .concat(),
                b"",
                &home,
            );
        }
        let fingerprint = |user: &str| {
            run("gpg", &["--list-keys", "--with-colons", user], b"", &home)
                .lines()
                .find_map(|line| {
                    line.strip_prefix("fpr:")
                        .map(|rest| rest.split(':').nth(8).unwrap_or_default().to_owned())
                })
                .unwrap()
        };
        let fa = fingerprint("a@t");
        let fb = fingerprint("b@t");
        run(
            "gpg",
            &[
                &loopback[..],
                &["passa", "--quick-add-key", &fa, "cv25519", "encr", "never"],
            ]
            .concat(),
            b"",
            &home,
        );
        run(
            "gpg",
            &[
                &loopback[..],
                &["passb", "--quick-add-key", &fb, "cv25519", "encr", "never"],
            ]
            .concat(),
            b"",
            &home,
        );
        let encrypted = lab.text("good.key.gpg");
        let good = good_key.display().to_string();
        run(
            "gpg",
            &[
                "-q",
                "--batch",
                "--trust-model",
                "always",
                "-r",
                &fa,
                "-r",
                &fb,
                "--encrypt",
                "-o",
                &encrypted,
                &good,
            ],
            b"",
            &home,
        );
        let only_b = lab.text("onlyb");
        std::fs::create_dir(&only_b).unwrap();
        run("chmod", &["700", &only_b], b"", &[]);
        let secret = std::process::Command::new("gpg")
            .args([
                "--batch",
                "--pinentry-mode",
                "loopback",
                "--passphrase",
                "passb",
                "--export-secret-keys",
                &fb,
            ])
            .env("GNUPGHOME", &generator)
            .output()
            .unwrap()
            .stdout;
        let only = [("GNUPGHOME", only_b.as_str())];
        run(
            "gpg",
            &[
                "-q",
                "--batch",
                "--pinentry-mode",
                "loopback",
                "--passphrase",
                "passb",
                "--import",
            ],
            &secret,
            &only,
        );
        let public = std::process::Command::new("gpg")
            .args(["--batch", "--export", &fa])
            .env("GNUPGHOME", &generator)
            .output()
            .unwrap()
            .stdout;
        run("gpg", &["-q", "--batch", "--import"], &public, &only);
        let symmetric = lab.text("good.key.sym.gpg");
        run(
            "gpg",
            &[
                &loopback[..],
                &["symp", "--symmetric", "-o", &symmetric, &good],
            ]
            .concat(),
            b"",
            &home,
        );
        let _ = try_run("gpgconf", &["--kill", "all"], b"", &home);
        let _ = try_run("gpgconf", &["--kill", "all"], b"", &only);
        Gnupg {
            encrypted,
            only_b,
            symmetric,
        }
    }
}
