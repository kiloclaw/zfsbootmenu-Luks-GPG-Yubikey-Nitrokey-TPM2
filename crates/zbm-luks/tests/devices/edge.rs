//! Failure paths, races, token listing and teardown matching, the Rust port
//! of `tests/luks/unit/edge.sh`. Its two legacy-only cases, which replace
//! functions inside the Bash code, are unit tests of the Rust runtime:
//! `disabled_or_unavailable_unlocking_fails_only_retries` (zbm-luks) and
//! `late_luks_volumes_are_unlocked_before_retrying` (zbm-boot).

use std::time::Duration;

mod later;

use crate::harness::{
    Case, Checks, Dump, Format, Gnupg, Lab, LinesConsole, enabled, has_line, no_leftovers, run,
    script, try_run,
};

const CRYPTSETUP: &str = "/usr/sbin/cryptsetup";

/// The tokens `luks_list_tokens` prints for `device`, one `id kind` per line.
fn list_tokens(case: &Case<'_>, device: &str) -> String {
    let console = LinesConsole::new(&[], Duration::ZERO, Duration::ZERO);
    case.with_session(&console, |_, system| {
        zbm_luks::tokens::list_tokens(system, device)
            .iter()
            .map(|token| format!("{} {}\n", token.id, token.kind))
            .collect()
    })
}

/// The key slots of a LUKS2 header, from the `luksDump` text.
fn key_slots(header: &str) -> Vec<String> {
    let dump = try_run(CRYPTSETUP, &["luksDump", header], b"", &[]);
    let text = String::from_utf8_lossy(&dump.stdout).into_owned();
    let mut slots = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("Keyslots:") {
            inside = true;
        } else if line.starts_with("Tokens:") {
            inside = false;
        } else if inside
            && let Some(rest) = line.strip_prefix("  ")
            && let Some((slot, kind)) = rest.split_once(": ")
            && kind.starts_with("luks2")
        {
            slots.push(slot.to_owned());
        }
    }
    slots
}

/// The token JSON `token` with its `keyslots` list replaced by `[slot]`.
fn with_key_slot(token: &str, slot: &str) -> String {
    let start = token.find("\"keyslots\"").unwrap();
    let open = start + token[start..].find('[').unwrap();
    let close = open + token[open..].find(']').unwrap();
    format!("{}[\"{slot}\"]{}", &token[..open], &token[close + 1..])
}

/// A case in which the volumes `zfs` hold ZFS, with timed prompts of a
/// second and no wait for security keys.
fn new_case<'l>(lab: &'l Lab, zfs: &[String]) -> Case<'l> {
    let mut case = lab.case();
    for device in zfs {
        case.content(device, "zfs_member");
    }
    case.cmdline("zbm.luks_token_wait", "0\n");
    case.prompt_delay = Duration::from_secs(1);
    case
}

/// Turns off cryptsetup's token plugins in `case`.
fn no_plugins(case: &Case<'_>) {
    case.env.set("LUKS_NO_TOKEN_PLUGINS", "1");
}

/// The volumes and keys the later sections of the suite use.
struct Volumes {
    /// The volumes that hold ZFS.
    zfs: Vec<String>,
    /// TPM2 and FIDO2 tokens.
    multi: String,
    /// A TPM2 token whose key slot was removed.
    stale: String,
    /// A FIDO2 token only.
    fido_only: String,
    /// A passphrase only.
    plain: String,
    /// LUKS1.
    l1: String,
    /// The GnuPG keys.
    gnupg: Gnupg,
}

#[test]
fn edge() {
    if !enabled("edge") {
        return;
    }
    let lab = Lab::new("edge", Some(2721));
    let stale = lab.luks("stale", b"pw", Format::Luks2, true);
    lab.enroll_tpm(&stale, "", None);
    run(
        CRYPTSETUP,
        &["luksKillSlot", "-q", "--key-file=-", &stale, "1"],
        b"pw",
        &[],
    );
    let multi = lab.luks("multi", b"pw", Format::Luks2, true);
    lab.enroll_tpm(&multi, "", None);
    let fp = lab.path("fp");
    std::fs::write(&fp, &lab.fido.hmac).unwrap();
    Lab::add_key(&multi, &fp);
    lab.import_token(&multi, &lab.fido.token("2", false));
    let fido_only = lab.luks("fidoonly", b"pw", Format::Luks2, true);
    Lab::add_key(&fido_only, &fp);
    lab.import_token(&fido_only, &lab.fido.token("1", false));
    let tok = lab.luks("tok", b"pw", Format::Luks2, true);
    lab.enroll_tpm(&tok, "", None);
    let l1 = lab.luks("one", b"pw", Format::Luks1, false);
    let plain = lab.luks("plain", b"pw", Format::Luks2, true);
    let hdrdata_image = lab.image("hdrdata.img", 32);
    let hdr = lab.image("hdr", 16).display().to_string();
    let hdrdata = lab.attach(&hdrdata_image, false);
    Lab::format(&hdrdata, b"pw", Format::Luks2, None, Some(&hdr));
    let good_key = lab.path("good.key");
    std::fs::write(&good_key, crate::harness::random(64)).unwrap();
    let gk = lab.luks("gk", b"pw", Format::Luks2, true);
    Lab::add_key(&gk, &good_key);
    let wrong_key = lab.text("wrong.key");
    std::fs::write(&wrong_key, crate::harness::random(64)).unwrap();
    let gnupg = Gnupg::new(&lab, &good_key);
    let ks = lab.path("ks");
    script(&ks, "#!/bin/sh\nprintf pw\n");
    let ks = ks.display().to_string();
    let ugk = Lab::uuid(&gk);

    let mut checks = Checks::new("edge");
    let mut zfs = vec![
        stale.clone(),
        multi.clone(),
        fido_only.clone(),
        tok.clone(),
        l1.clone(),
        plain.clone(),
        hdrdata.clone(),
        gk.clone(),
    ];
    let case = |zfs: &[String]| new_case(&lab, zfs);

    println!("### key management");
    checks.case("TPM2 token whose key slot was removed -> passphrase fallback");
    let c = case(&zfs);
    no_plugins(&c);
    c.crypttab(&[&format!("v {stale} none")]);
    c.unlock(&["pw"]);
    checks.check("mapped via passphrase", c.mapped("v"));
    checks.check(
        "TPM unseal attempted first",
        c.calls()
            .iter()
            .filter(|call| call.starts_with("cryptsetup open "))
            .count()
            >= 2,
    );
    checks.check("no leftovers", no_leftovers());

    checks.case("TPM2 + FIDO2 tokens on one volume, no TPM present -> FIDO2 unlocks");
    let c = case(&zfs);
    no_plugins(&c);
    c.env.set("LUKS_TPM2_TCTI", "device:/dev/no-such-tpm");
    c.crypttab(&[&format!("v {multi} none")]);
    c.unlock(&[]);
    checks.check("mapped", c.mapped("v"));
    checks.check("TPM skipped", c.log().contains("no TPM2 available"));
    checks.check("FIDO2 used", c.log().contains("with FIDO2"));

    checks.case("token list: TPM2 + FIDO2 tokens on one volume");
    let c = case(&zfs);
    let out = list_tokens(&c, &multi);
    checks.check("token 0 systemd-tpm2", has_line(&out, "0 systemd-tpm2"));
    checks.check("token 1 systemd-fido2", has_line(&out, "1 systemd-fido2"));

    // With systemd 261's token plugins, cryptsetup 2.8 prints the luksDump
    // text of every token after the first to stderr. Emulate that and list
    // the tokens.
    checks.case("token list: luksDump prints later tokens to stderr (systemd 261 plugins)");
    let c = case(&zfs);
    *c.runner.dump.lock().unwrap() = Dump::LaterTokensOnStderr;
    let out = list_tokens(&c, &multi);
    checks.check("token 0 found", has_line(&out, "0 systemd-tpm2"));
    checks.check(
        "token 1 found despite stderr",
        has_line(&out, "1 systemd-fido2"),
    );

    checks.case("token list: LUKS1 volume has no tokens");
    let c = case(&zfs);
    checks.check("nothing listed", list_tokens(&c, &l1).is_empty());

    // three token types, then a gap in the token ids, and a detached header
    let three = lab.luks("three", b"pw", Format::Luks2, true);
    lab.enroll_tpm(&three, "", None);
    Lab::add_key(&three, &fp);
    lab.import_token(&three, &lab.fido.token("2", false));
    lab.bind_clevis(&three);
    checks.case("token list: TPM2, FIDO2 and clevis (nested JSON) tokens");
    let c = case(&zfs);
    let out = list_tokens(&c, &three);
    checks.check("three tokens", out.lines().count() == 3);
    checks.check("clevis listed", has_line(&out, "2 clevis"));
    run(
        CRYPTSETUP,
        &["token", "remove", "--token-id", "1", &three],
        b"",
        &[],
    );
    checks.case("token list: gap in the token ids after removing token 1");
    let c = case(&zfs);
    let out = list_tokens(&c, &three);
    checks.check("token 0 kept", has_line(&out, "0 systemd-tpm2"));
    checks.check("token 2 kept", has_line(&out, "2 clevis"));
    checks.check("two tokens", out.lines().count() == 2);
    lab.import_token(&hdr, &lab.fido.token("0", false));
    checks.case("token list: detached LUKS2 header");
    let c = case(&zfs);
    checks.check(
        "token read from the header file",
        has_line(&list_tokens(&c, &hdr), "0 systemd-fido2"),
    );

    checks.case("key script wins over tokens");
    let c = case(&zfs);
    c.crypttab(&[&format!("v {tok} none keyscript={ks}")]);
    c.unlock(&[]);
    checks.check("mapped", c.mapped("v"));
    checks.check("no token export", !c.called("token export"));

    checks.case("detached LUKS header");
    let c = case(&zfs);
    c.crypttab(&[&format!("v {hdrdata} none header={hdr}")]);
    c.unlock(&["pw"]);
    checks.check("mapped", c.mapped("v"));
    checks.check(
        "header passed to cryptsetup",
        c.called(&format!("--header {hdr}")),
    );

    // clevis as the second token, behind a TPM2 token with a PIN, with the
    // luksDump text lacking later tokens (systemd 261's token plugins)
    let clv2 = lab.luks("clev2", b"pw", Format::Luks2, true);
    lab.enroll_tpm(&clv2, "", Some("1234"));
    lab.bind_clevis(&clv2);
    zfs.push(clv2.clone());
    checks.case("clevis as second token, luksDump prints it to stderr (systemd 261 plugins)");
    let c = case(&zfs);
    *c.runner.dump.lock().unwrap() = Dump::LaterTokensOnStderr;
    c.crypttab(&[&format!("v {clv2} none")]);
    c.unlock(&[]);
    checks.check("mapped with no prompt", c.mapped("v"));
    checks.check(
        "unlocked via clevis",
        c.log().contains(&format!("unlocked {clv2} with clevis")),
    );
    checks.check("no PIN prompt", c.prompts("TPM2 PIN") == 0);

    // clevis token in a detached header: the JWE of CLV2 opens a new key slot
    // of the header
    let tpm = lab.tpm_env();
    let tpm: Vec<(&str, &str)> = tpm
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let token = run(
        CRYPTSETUP,
        &["token", "export", "--token-id", "1", &clv2],
        b"",
        &[],
    );
    let jwe = run(
        "jose",
        &["fmt", "-j-", "-Og", "jwe", "-o-"],
        token.as_bytes(),
        &[],
    );
    let compact = run("jose", &["jwe", "fmt", "-i-", "-c"], jwe.as_bytes(), &[]);
    let clevis_key = try_run("clevis", &["decrypt"], compact.as_bytes(), &tpm).stdout;
    let clevis_key_file = lab.path("clevis.key");
    std::fs::write(&clevis_key_file, &clevis_key).unwrap();
    let before = key_slots(&hdr);
    let key_file = clevis_key_file.display().to_string();
    run(
        CRYPTSETUP,
        &[
            "luksAddKey",
            "-q",
            "--header",
            &hdr,
            "--pbkdf",
            "pbkdf2",
            "--pbkdf-force-iterations",
            "1000",
            "--key-file=-",
            &hdrdata,
            &key_file,
        ],
        b"pw",
        &[],
    );
    let new_slot = key_slots(&hdr)
        .into_iter()
        .find(|slot| !before.contains(slot))
        .unwrap_or_default();
    lab.import_token(&hdr, &with_key_slot(&token, &new_slot));
    checks.case("clevis token in a detached LUKS2 header");
    let c = case(&zfs);
    c.crypttab(&[&format!("v {hdrdata} none header={hdr}")]);
    c.unlock(&[]);
    checks.check(
        &format!("clevis slot added ({new_slot})"),
        !new_slot.is_empty(),
    );
    checks.check("mapped with no prompt", c.mapped("v"));
    checks.check(
        "unlocked via clevis",
        c.log().contains(&format!("unlocked {hdrdata} with clevis")),
    );

    checks.case("LUKS1 volume (no tokens)");
    let c = case(&zfs);
    c.crypttab(&[&format!("v {l1} none")]);
    c.unlock(&["pw"]);
    checks.check("mapped", c.mapped("v"));
    checks.check("no errors", !c.log().contains("ERROR"));

    checks.case("wrong key file content -> passphrase fallback");
    let c = case(&zfs);
    c.crypttab(&[&format!("v {gk} {wrong_key}")]);
    c.unlock(&["pw"]);
    checks.check("mapped", c.mapped("v"));
    checks.check(
        "warned",
        c.log().contains(&format!("key file {wrong_key} failed")),
    );

    checks.case("GnuPG file encrypted to two recipients, only second key present");
    let c = case(&zfs);
    c.env.set("LUKS_GNUPG_DIR", gnupg.only_b.as_str());
    c.crypttab(&[&format!("v {gk} {}", gnupg.encrypted)]);
    c.unlock(&["passb"]);
    checks.check("mapped", c.mapped("v"));
    checks.check("secret key prompt", c.prompts("Secret key passphrase") == 1);

    checks.case("zbm.luks_key pointing at a GnuPG file");
    let c = case(&zfs);
    c.cmdline("zbm.luks_key", &format!("{}\n", gnupg.symmetric));
    c.crypttab(&[&format!("v {gk} none")]);
    c.unlock(&["symp"]);
    checks.check("mapped", c.mapped("v"));

    // Gentoo V3 guide: no crypttab at all, only zbm.luks=UUID= and
    // zbm.luks_key=
    let luks_gk = format!("luks-{ugk}");
    checks.case("no crypttab: zbm.luks=UUID= + symmetric GnuPG zbm.luks_key");
    let c = case(&zfs);
    c.no_crypttab();
    c.cmdline("zbm.luks", &format!("UUID={ugk}\n"));
    c.cmdline("zbm.luks_key", &format!("{}\n", gnupg.symmetric));
    c.unlock(&["symp"]);
    checks.check("mapped as luks-<UUID>", c.mapped(&luks_gk));
    checks.check("cmdline mode", !c.log().contains("mode: auto"));
    checks.check(
        "one prompt",
        c.timed_prompts().matches("timed_prompt").count() <= 1,
    );
    checks.check("no leftovers", no_leftovers());
    checks.case("no crypttab: zbm.luks=UUID= + public-key GnuPG zbm.luks_key");
    let c = case(&zfs);
    c.no_crypttab();
    c.env.set("LUKS_GNUPG_DIR", gnupg.only_b.as_str());
    c.cmdline("zbm.luks", &format!("UUID={ugk}\n"));
    c.cmdline("zbm.luks_key", &format!("{}\n", gnupg.encrypted));
    c.unlock(&["passb"]);
    checks.check("mapped as luks-<UUID>", c.mapped(&luks_gk));
    checks.check(
        "unlocked with GnuPG key file",
        c.log().contains("with GnuPG key file"),
    );
    checks.check("no leftovers", no_leftovers());
    checks.case("no crypttab: GnuPG passphrase wrong, then empty -> LUKS passphrase fallback");
    let c = case(&zfs);
    c.no_crypttab();
    c.cmdline("zbm.luks", &format!("UUID={ugk}\n"));
    c.cmdline("zbm.luks_key", &format!("{}\n", gnupg.symmetric));
    c.unlock(&["wrong", "", "pw"]);
    checks.check("mapped via fallback", c.mapped(&luks_gk));
    // Gentoo V4 guide: no crypttab, only zbm.luks=UUID=, passphrase prompt
    checks.case("no crypttab: zbm.luks=UUID= with passphrase only");
    let c = case(&zfs);
    c.no_crypttab();
    c.cmdline("zbm.luks", &format!("UUID={ugk}\n"));
    c.unlock(&["pw"]);
    checks.check("mapped as luks-<UUID>", c.mapped(&luks_gk));
    checks.check(
        "only the listed volume",
        c.mappings()
            .iter()
            .filter(|name| name.contains("luks-"))
            .count()
            == 1,
    );

    checks.case("auto discovery unlocks TPM2 volume silently, closes swap");
    let c = case(&zfs);
    c.unlock(&[]);
    c.content(&plain, "swap");
    let utok = Lab::uuid(&tok);
    checks.check(
        "TOK mapped",
        c.mappings()
            .iter()
            .any(|name| name.contains(&format!("luks-{utok}"))),
    );
    checks.check(
        "no prompt for TOK",
        !c.out().contains(&format!("Unlock LUKS volume {tok}")),
    );

    checks.case("empty crypttab -> auto mode");
    let c = case(&zfs);
    c.crypttab_raw("");
    c.unlock(&[]);
    checks.check("auto mode", c.log().contains("mode: auto"));

    let volumes = Volumes {
        zfs,
        multi,
        stale,
        fido_only,
        plain,
        l1,
        gnupg,
    };
    later::races_and_failures(&lab, &mut checks, &volumes);
    checks.finish(81);
}
