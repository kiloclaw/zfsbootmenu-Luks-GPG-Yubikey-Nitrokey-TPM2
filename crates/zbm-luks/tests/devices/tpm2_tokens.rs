//! TPM2 PIN tokens of systemd 262: a token whose PIN is hardened with
//! Argon2id, made by hand from the SB21 plan's known-answer vector V3 so that
//! it runs on every host, and, where the host's `systemd-cryptenroll` hardens
//! PINs with Argon2id, a token enrolled with `direct`, which keeps PBKDF2.
//! The Bash suites had neither.

use crate::harness::{
    Checks, Format, Lab, PinKdf, SEALED_ATTRIBUTES, TpmPin, V3_PIN, cryptenroll_has_argon2id,
    enabled,
};

#[test]
fn tpm2_tokens() {
    if !enabled("tpm2_tokens") {
        return;
    }
    let lab = Lab::new("tpm2_tokens", Some(2521));
    let argon2id = lab.luks("argon2id", b"pw", Format::Luks2, true);
    lab.make_argon2id_token(&argon2id);

    let mut checks = Checks::new("tpm2_tokens");
    let passphrase = |case: &crate::harness::Case<'_>| case.prompts("Passphrase (leave");

    // The plugin is left as the host has it: an Argon2id PIN never goes to
    // it, which before systemd 262 would derive it the old way.
    checks.case("a hand-made Argon2id token opens with its PIN");
    let hardening = Lab::pin_kdf(&argon2id);
    checks.check(
        &format!("the token reads as Argon2id: {hardening}"),
        matches!(hardening, PinKdf::Argon2id(costs)
            if costs.memcost_bytes == 1_049_599 && costs.iterations == 3 && costs.lanes == 1),
    );
    // As systemd seals it: without `userwithauth`, the authorization value
    // alone, in a password session, does not unseal it.
    let attributes = lab.sealed_attributes();
    checks.check(
        &format!("sealed with systemd's attributes: {attributes}"),
        attributes == SEALED_ATTRIBUTES,
    );
    let c = lab.case();
    c.cmdline("zbm.luks_token_wait", "0\n");
    c.crypttab(&[&format!("v {argon2id} none")]);
    c.unlock(&["0000", V3_PIN]);
    checks.check("mapped", c.mapped("v"));
    checks.check("asked for TPM2 PIN twice", c.prompts("TPM2 PIN") == 2);
    checks.check(
        "wrong-PIN message",
        c.out().contains("did not release the key"),
    );
    checks.check("no passphrase prompt", passphrase(&c) == 0);
    checks.check(
        "unlocked by ZFSBootMenu's own unsealing",
        c.log()
            .contains(&format!("unlocked {argon2id} with TPM2 and PIN")),
    );
    checks.check("not given to the plugin", !c.called("--token-only"));
    let mut expected = 8;

    checks.case("direct PIN token");
    if cryptenroll_has_argon2id() {
        let direct = lab.luks("direct", b"pw", Format::Luks2, true);
        lab.enroll_tpm(&direct, "", TpmPin::Direct("1234"));
        checks.check(
            "hardened with PBKDF2",
            Lab::pin_kdf(&direct) == PinKdf::Pbkdf2,
        );
        let c = lab.case();
        c.cmdline("zbm.luks_token_wait", "0\n");
        c.env.set("LUKS_NO_TOKEN_PLUGINS", "1");
        c.crypttab(&[&format!("v {direct} none")]);
        c.unlock(&["1234"]);
        checks.check("mapped", c.mapped("v"));
        checks.check("asked for TPM2 PIN once", c.prompts("TPM2 PIN") == 1);
        checks.check("no passphrase prompt", passphrase(&c) == 0);
        expected += 4;
    } else {
        println!("  skip - systemd-cryptenroll has no Argon2id PINs, so no `direct`");
    }

    checks.finish(expected);
}
