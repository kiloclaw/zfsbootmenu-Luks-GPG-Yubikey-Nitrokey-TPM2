//! Key management, the Rust port of `tests/luks/unit/tokens.sh`: TPM2 tokens
//! with and without a PIN, clevis, GnuPG key files, YubiKey
//! challenge-response and FIDO2 tokens.

use crate::harness::{Case, Checks, Format, Lab, enabled, run};

/// The HMAC-SHA1 of `data` with the YubiKey stand-in's secret, in hex.
fn yubikey_response(data: &[u8]) -> String {
    let digest = run(
        "openssl",
        &["dgst", "-sha1", "-hmac", "yk-secret"],
        data,
        &[],
    );
    digest.rsplit(' ').next().unwrap_or_default().to_owned()
}

/// Whether a decrypted key or GnuPG home is left in `/run` or `/tmp`.
fn key_left(dirs: &[&str], prefixes: &[&str]) -> bool {
    dirs.iter().any(|dir| {
        std::fs::read_dir(dir).is_ok_and(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                prefixes.iter().any(|prefix| name.starts_with(prefix))
            })
        })
    })
}

#[test]
fn tokens() {
    if !enabled("tokens") {
        return;
    }
    let lab = Lab::new("tokens", Some(2421));
    let t0 = lab.luks("tpm", b"pw", Format::Luks2, true);
    lab.enroll_tpm(&t0, "", None);
    let t1 = lab.luks("tpmpin", b"pw", Format::Luks2, true);
    lab.enroll_tpm(&t1, "", Some("1234"));
    let t2 = lab.luks("tpmpcr", b"pw", Format::Luks2, true);
    lab.enroll_tpm(&t2, "16", None);
    let cl = lab.luks("clevis", b"pw", Format::Luks2, true);
    lab.bind_clevis(&cl);

    // GnuPG: a symmetric key file, and a key file encrypted to a secret key
    let generator = lab.text("gen");
    std::fs::create_dir(&generator).unwrap();
    run("chmod", &["700", &generator], b"", &[]);
    let home = [("GNUPGHOME", generator.as_str())];
    let k1 = lab.path("k1");
    let k2 = lab.path("k2");
    std::fs::write(&k1, crate::harness::random(64)).unwrap();
    std::fs::write(&k2, crate::harness::random(64)).unwrap();
    let gs = lab.luks("gpgsym", b"pw", Format::Luks2, true);
    Lab::add_key(&gs, &k1);
    let gp = lab.luks("gpgpub", b"pw", Format::Luks2, true);
    Lab::add_key(&gp, &k2);
    let gp2 = lab.luks("gpgpub2", b"pw", Format::Luks2, true);
    Lab::add_key(&gp2, &k2);
    let k1_gpg = lab.text("k1.gpg");
    let k2_gpg = lab.text("k2.gpg");
    let k1_text = k1.display().to_string();
    let k2_text = k2.display().to_string();
    let batch = ["-q", "--batch", "--pinentry-mode", "loopback"];
    run(
        "gpg",
        &[
            &batch[..],
            &[
                "--passphrase",
                "gpgpass",
                "--symmetric",
                "-o",
                &k1_gpg,
                &k1_text,
            ],
        ]
        .concat(),
        b"",
        &home,
    );
    run(
        "gpg",
        &[
            &batch[..],
            &[
                "--passphrase",
                "keypass",
                "--quick-gen-key",
                "ZBM <zbm@test>",
                "ed25519",
                "sign",
                "never",
            ],
        ]
        .concat(),
        b"",
        &home,
    );
    let keys = run("gpg", &["--list-keys", "--with-colons"], b"", &home);
    let fpr = keys
        .lines()
        .find_map(|line| {
            line.strip_prefix("fpr:")
                .map(|rest| rest.split(':').nth(8).unwrap_or_default().to_owned())
        })
        .unwrap();
    run(
        "gpg",
        &[
            &batch[..],
            &[
                "--passphrase",
                "keypass",
                "--quick-add-key",
                &fpr,
                "cv25519",
                "encr",
                "never",
            ],
        ]
        .concat(),
        b"",
        &home,
    );
    run(
        "gpg",
        &[
            "-q",
            "--batch",
            "--trust-model",
            "always",
            "-r",
            &fpr,
            "--encrypt",
            "-o",
            &k2_gpg,
            &k2_text,
        ],
        b"",
        &home,
    );
    let imgpub = lab.text("imgpub");
    let imgsec = lab.text("imgsec");
    for dir in [&imgpub, &imgsec] {
        std::fs::create_dir(dir).unwrap();
        run("chmod", &["700", dir], b"", &[]);
    }
    let public = std::process::Command::new("gpg")
        .args(["--batch", "--export", &fpr])
        .env("GNUPGHOME", &generator)
        .output()
        .unwrap()
        .stdout;
    run(
        "gpg",
        &["-q", "--batch", "--import"],
        &public,
        &[("GNUPGHOME", imgpub.as_str())],
    );
    run(
        "cp",
        &["-a", &format!("{generator}/."), &format!("{imgsec}/")],
        b"",
        &[],
    );
    let _ = crate::harness::try_run("gpgconf", &["--kill", "all"], b"", &home);

    // YubiKey challenge-response: the LUKS passphrase is HMAC-SHA1(challenge)
    let ykresp = lab.path("ykresp");
    std::fs::write(&ykresp, yubikey_response(b"ykpass")).unwrap();
    let yk = lab.luks("yk", b"pw", Format::Luks2, true);
    Lab::add_key(&yk, &ykresp);
    let yk2 = lab.luks("yk2", b"pw", Format::Luks2, true);
    Lab::add_key(&yk2, &ykresp);
    let challenge = run("sha256sum", &[], b"ykpass", &[]);
    let challenge = challenge.get(..64).unwrap_or_default().to_owned();
    let ykresp2 = lab.path("ykresp2");
    std::fs::write(
        &ykresp2,
        format!("{challenge}{}", yubikey_response(challenge.as_bytes())),
    )
    .unwrap();
    let yh = lab.luks("ykhash", b"pw", Format::Luks2, true);
    Lab::add_key(&yh, &ykresp2);

    // FIDO2: a systemd-fido2 token whose key slot's passphrase is the
    // hmac-secret (base64)
    let f2 = lab.luks("fido", b"pw", Format::Luks2, true);
    let fidopass = lab.path("fidopass");
    std::fs::write(&fidopass, &lab.fido.hmac).unwrap();
    Lab::add_key(&f2, &fidopass);
    lab.import_token(&f2, &lab.fido.token("1", false));

    let mut checks = Checks::new("tokens");
    let case = || {
        let case = lab.case();
        case.cmdline("zbm.luks_token_wait", "0\n");
        case
    };
    let no_plugins = |case: &Case<'_>| case.env.set("LUKS_NO_TOKEN_PLUGINS", "1");
    let passphrase = |case: &Case<'_>| case.prompts("Passphrase (leave");

    checks.case("TPM2 token without PIN (plugin present but no TPM device -> native)");
    let c = case();
    c.crypttab(&[&format!("v {t0} none")]);
    c.unlock(&[]);
    checks.check("mapped with no prompt", c.mapped("v"));
    checks.check("no passphrase prompt", passphrase(&c) == 0);
    checks.check(
        "unlocked via TPM2",
        c.log().contains(&format!("unlocked {t0} with TPM2")),
    );

    checks.case("TPM2 token without PIN, plugins disabled");
    let c = case();
    no_plugins(&c);
    c.crypttab(&[&format!("v {t0} none")]);
    c.unlock(&[]);
    checks.check("mapped", c.mapped("v"));

    checks.case("TPM2 PIN token: wrong PIN, then right PIN");
    let c = case();
    no_plugins(&c);
    c.crypttab(&[&format!("v {t1} none")]);
    c.unlock(&["0000", "1234"]);
    checks.check("mapped", c.mapped("v"));
    checks.check("asked for TPM2 PIN twice", c.prompts("TPM2 PIN") == 2);
    checks.check(
        "wrong-PIN message",
        c.out().contains("did not release the key"),
    );
    checks.check("no passphrase prompt", passphrase(&c) == 0);

    checks.case("TPM2 PIN token: empty PIN falls back to passphrase");
    let c = case();
    no_plugins(&c);
    c.crypttab(&[&format!("v {t1} none")]);
    c.unlock(&["", "pw"]);
    checks.check("mapped via passphrase", c.mapped("v"));
    checks.check("passphrase prompt shown", passphrase(&c) == 1);

    checks.case("TPM2 PIN and no-PIN volumes together");
    let c = case();
    no_plugins(&c);
    c.crypttab(&[&format!("a {t1} none"), &format!("b {t0} none")]);
    c.unlock(&["1234"]);
    checks.check("a mapped", c.mapped("a"));
    checks.check("b mapped", c.mapped("b"));

    // The authorization value derived from the PIN reaches tpm2_unseal on
    // standard input, never in its arguments, which any process can read in
    // /proc
    checks.case("TPM2 PIN token: the PIN's authorization value stays out of arguments");
    let c = case();
    no_plugins(&c);
    c.crypttab(&[&format!("v {t1} none")]);
    c.unlock(&["1234"]);
    let unseals: Vec<String> = c
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("tpm2_unseal"))
        .collect();
    checks.check("mapped", c.mapped("v"));
    checks.check("tpm2_unseal ran", !unseals.is_empty());
    checks.check(
        "authorization read from standard input",
        unseals.iter().any(|call| call.contains("+file:-")),
    );
    checks.check(
        "no authorization value in the arguments",
        !unseals.iter().any(|call| call.contains("hex:")),
    );

    checks.case("TPM2 token bound to PCR 16 after PCR 16 changed -> passphrase");
    let c = case();
    no_plugins(&c);
    let extended = lab.with_tpm(
        "tpm2_pcrextend",
        &["16:sha256=0000000000000000000000000000000000000000000000000000000000000001"],
        &[],
    );
    assert!(extended.status.success(), "tpm2_pcrextend failed");
    c.crypttab(&[&format!("v {t2} none")]);
    c.unlock(&["pw"]);
    checks.check("mapped via passphrase", c.mapped("v"));
    checks.check("passphrase prompt shown", passphrase(&c) == 1);

    checks.case("TPM2 PIN token: repeated wrong PINs hit the TPM lockout");
    let c = case();
    no_plugins(&c);
    c.crypttab(&[&format!("v {t1} none")]);
    c.unlock(&["000", "111", "222", "pw"]);
    checks.check(
        "lockout explained",
        c.out().contains("locked against PIN guessing"),
    );
    checks.check("fell back to passphrase", c.mapped("v"));
    checks.check("no PIN prompt after lockout", c.prompts("TPM2 PIN") <= 3);

    checks.case("TPM2 PIN token while TPM is in lockout: no PIN prompt at all");
    let c = case();
    no_plugins(&c);
    c.crypttab(&[&format!("v {t1} none")]);
    c.unlock(&["pw"]);
    checks.check("no PIN prompt", c.prompts("TPM2 PIN") == 0);
    checks.check(
        "lockout explained",
        c.out().contains("locked against PIN guessing"),
    );
    let _ = lab.with_tpm("tpm2_dictionarylockout", &["-c"], &[]);

    checks.case("clevis TPM2 token");
    let c = case();
    c.crypttab(&[&format!("v {cl} none")]);
    c.unlock(&[]);
    checks.check("mapped with no prompt", c.mapped("v"));
    checks.check(
        "unlocked via clevis",
        c.log().contains(&format!("unlocked {cl} with clevis")),
    );

    checks.case("no-tokens option skips tokens");
    let c = case();
    c.crypttab(&[&format!("v {cl} none no-tokens")]);
    c.unlock(&["pw"]);
    checks.check("mapped via passphrase", c.mapped("v"));
    checks.check("clevis not used", !c.log().contains("with clevis"));

    checks.case("zbm.luks_tokens=0 disables tokens");
    let c = case();
    c.cmdline("zbm.luks_tokens", "0\n");
    c.crypttab(&[&format!("v {t0} none")]);
    c.unlock(&["pw"]);
    checks.check("passphrase prompt shown", passphrase(&c) == 1);

    checks.case("GnuPG symmetric key file: wrong, then right passphrase");
    let c = case();
    c.env.set("LUKS_GNUPG_DIR", imgpub.as_str());
    c.crypttab(&[&format!("v {gs} {k1_gpg}")]);
    c.unlock(&["nope", "gpgpass"]);
    checks.check("mapped", c.mapped("v"));
    checks.check(
        "key file passphrase asked twice",
        c.prompts("Key file passphrase") == 2,
    );
    checks.check(
        "decrypted key not left behind",
        !key_left(&["/run"], &["zbm-key.", "zbm-gpg."]),
    );

    checks.case("GnuPG key file encrypted to a secret key");
    let c = case();
    c.env.set("LUKS_GNUPG_DIR", imgsec.as_str());
    c.crypttab(&[&format!("v {gp} {k2_gpg}")]);
    c.unlock(&["keypass"]);
    checks.check("mapped", c.mapped("v"));
    checks.check(
        "secret key passphrase prompt",
        c.prompts("Secret key passphrase") == 1,
    );

    checks.case("GnuPG key file, secret key/card absent -> passphrase, no PIN prompt");
    let c = case();
    c.env.set("LUKS_GNUPG_DIR", imgpub.as_str());
    c.crypttab(&[&format!("v {gp} {k2_gpg}")]);
    c.unlock(&["pw"]);
    checks.check("mapped via passphrase", c.mapped("v"));
    checks.check(
        "never asked for a PIN",
        !c.out().contains("Card PIN") && !c.out().contains("Secret key passphrase"),
    );
    checks.check("logged missing key", c.log().contains("no secret key for"));

    checks.case("gpg option for key file without .gpg suffix");
    let c = case();
    c.env.set("LUKS_GNUPG_DIR", imgpub.as_str());
    let k1_bin = lab.text("k1.bin");
    std::fs::copy(&k1_gpg, &k1_bin).unwrap();
    c.crypttab(&[&format!("v {gs} {k1_bin} gpg")]);
    c.unlock(&["gpgpass"]);
    checks.check("mapped", c.mapped("v"));

    checks.case("YubiKey challenge-response (slot 2)");
    let c = case();
    c.crypttab(&[&format!("v {yk} none ykchalresp")]);
    c.unlock(&["ykpass"]);
    checks.check("mapped", c.mapped("v"));
    checks.check(
        "challenge sent to slot 2 via stdin",
        c.called("ykchalresp -2 -i-"),
    );

    checks.case("YubiKey challenge-response, hash + concat");
    let c = case();
    c.crypttab(&[&format!(
        "v {yh} none ykchalresp=2,ykchalresp-hash,ykchalresp-concat"
    )]);
    c.unlock(&["ykpass"]);
    checks.check("mapped", c.mapped("v"));

    checks.case("YubiKey wrong passphrase then right");
    let c = case();
    c.crypttab(&[&format!("v {yk} none ykchalresp")]);
    c.unlock(&["bad", "ykpass"]);
    checks.check("mapped", c.mapped("v"));

    checks.case("FIDO2 token via native fido2-assert");
    let c = case();
    no_plugins(&c);
    c.crypttab(&[&format!("v {f2} none")]);
    c.unlock(&[]);
    checks.check("mapped", c.mapped("v"));
    checks.check(
        "touch instruction shown",
        c.out().contains("Touch the security key"),
    );
    checks.check(
        "fido2-assert used hmac-secret, up=true, no PIN",
        c.called("fido2-assert -G -h -t up=true -t uv=false /dev/hidraw99"),
    );

    checks.case("FIDO2 token requiring PIN passes pin=true");
    let c = case();
    no_plugins(&c);
    *c.runner.fido_needs_pin.lock().unwrap() = true;
    run(
        "/usr/sbin/cryptsetup",
        &["token", "remove", "--token-id", "0", &f2],
        b"",
        &[],
    );
    lab.import_token(&f2, &lab.fido.token("1", true));
    c.crypttab(&[&format!("v {f2} none")]);
    c.unlock(&[]);
    run(
        "/usr/sbin/cryptsetup",
        &["token", "remove", "--token-id", "0", &f2],
        b"",
        &[],
    );
    lab.import_token(&f2, &lab.fido.token("1", false));
    checks.check("mapped", c.mapped("v"));
    checks.check("pin=true requested", c.called("pin=true"));

    checks.case("FIDO2 key absent -> passphrase");
    let c = case();
    no_plugins(&c);
    std::fs::rename("/dev/hidraw99", "/dev/xhidraw99").unwrap();
    c.crypttab(&[&format!("v {f2} none")]);
    c.unlock(&["pw"]);
    std::fs::rename("/dev/xhidraw99", "/dev/hidraw99").unwrap();
    checks.check("mapped via passphrase", c.mapped("v"));
    checks.check(
        "logged missing key",
        c.log().contains("no FIDO2 security key"),
    );

    checks.case("two volumes sharing one GnuPG key file: one prompt");
    let c = case();
    c.env.set("LUKS_GNUPG_DIR", imgsec.as_str());
    c.crypttab(&[&format!("a {gp} {k2_gpg}"), &format!("b {gp2} {k2_gpg}")]);
    c.unlock(&["keypass"]);
    checks.check("a mapped", c.mapped("a"));
    checks.check("b mapped", c.mapped("b"));
    checks.check(
        "secret key passphrase asked once",
        c.prompts("Secret key passphrase") == 1,
    );
    checks.check(
        "decrypted key wiped afterwards",
        !key_left(&["/run", "/tmp"], &["zbm-key."]),
    );

    checks.case("two volumes on one YubiKey slot: one challenge");
    let c = case();
    c.crypttab(&[
        &format!("a {yk} none ykchalresp"),
        &format!("b {yk2} none ykchalresp"),
    ]);
    c.unlock(&["ykpass"]);
    checks.check("a mapped", c.mapped("a"));
    checks.check("b mapped", c.mapped("b"));
    checks.check(
        "challenge asked once",
        c.prompts("YubiKey challenge-response") == 1,
    );

    checks.finish(55);
}
