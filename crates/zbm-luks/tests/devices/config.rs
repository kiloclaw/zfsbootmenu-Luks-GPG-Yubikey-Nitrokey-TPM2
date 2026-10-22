//! Crypttab and command line parsing, device specifications, passphrases
//! and USB key sticks, the Rust port of `tests/luks/unit/config.sh`. The
//! TPM2, FIDO2 and GnuPG volumes that script sets up are not used by its
//! checks and are left out.

use std::thread;
use std::time::Duration;

use crate::harness::{
    Case, Checks, Format, Lab, LinesConsole, enabled, mounted, random, run, try_run,
};

/// A passphrase with spaces, quotes, a backslash, `$`, Unicode and a
/// trailing space.
const SPECIAL: &str = "with spaces  and \"quotes\" \\back$HOME -x ünï 密码 ";

/// The loop devices of `image`, from `losetup -j`.
fn loops_of(image: &str) -> String {
    String::from_utf8_lossy(&try_run("losetup", &["-j", image], b"", &[]).stdout).into_owned()
}

#[test]
fn config() {
    if !enabled("config") {
        return;
    }
    let lab = Lab::new("config", None);
    let l1 = lab.luks("one", b"pw", Format::Luks1, false);
    let good_key = random(64);
    std::fs::write(lab.path("good.key"), &good_key).unwrap();
    let gk = lab.luks("gk", b"pw", Format::Luks2, true);
    lab.add_key(&gk, &lab.path("good.key"));
    let spec = lab.luks("spec", SPECIAL.as_bytes(), Format::Luks2, false);
    let lead = lab.luks("lead", b"-starts-with-dash", Format::Luks2, false);
    let long_passphrase: String = crate::harness::base64(&random(300))
        .chars()
        .take(400)
        .collect();
    let long = lab.luks("long", long_passphrase.as_bytes(), Format::Luks2, false);
    // a partitioned image with LUKS in partition 1 (PARTUUID)
    let part_image = lab.image("part.img", 64);
    let part_image_text = part_image.display().to_string();
    run(
        "sfdisk",
        &["-q", &part_image_text],
        b"label: gpt\nsize=40MiB\n",
        &[],
    );
    let part_loop = lab.attach(&part_image, true);
    let _ = try_run("udevadm", &["settle"], b"", &[]);
    thread::sleep(Duration::from_secs(1));
    let part = format!("{part_loop}p1");
    let partuuid = if std::path::Path::new(&part).exists() {
        lab.format(&part, b"pw", Format::Luks2, None, None);
        run(
            "blkid",
            &["-c", "/dev/null", "-s", "PARTUUID", "-o", "value", &part],
            b"",
            &[],
        )
    } else {
        String::new()
    };
    let not_luks_image = lab.image("nl.img", 8);
    let not_luks = lab.attach(&not_luks_image, false);
    // key file with junk around it, an empty key file and a directory
    let mut padded = random(100);
    padded.extend_from_slice(&good_key);
    padded.extend(random(50));
    std::fs::write(lab.path("padded.key"), padded).unwrap();
    std::fs::write(lab.path("empty.key"), "").unwrap();
    std::fs::create_dir_all(lab.path("dir.key")).unwrap();
    // key stick: ext4 with the key, attached late
    let stick = lab.image("stick.img", 16);
    let stick_text = stick.display().to_string();
    run(
        "mkfs.ext4",
        &["-q", "-L", "FINALKEY", &stick_text],
        b"",
        &[],
    );
    let mount = lab.text("m");
    std::fs::create_dir_all(&mount).unwrap();
    run("mount", &["-o", "loop", &stick_text, &mount], b"", &[]);
    std::fs::create_dir_all(lab.path("m/keys")).unwrap();
    std::fs::write(lab.path("m/keys/gk.key"), &good_key).unwrap();
    run("umount", &[&mount], b"", &[]);
    let ugk = Lab::uuid(&gk);
    let upper = ugk.to_uppercase();
    let k = |name: &str| lab.text(name);

    let mut checks = Checks::new("config");
    let devices = [&spec, &lead, &long, &part, &not_luks, &l1, &gk];
    let case = || {
        let mut case = lab.case();
        for device in devices {
            if std::path::Path::new(device).exists() {
                case.content(device, "zfs_member");
            }
        }
        case.cmdline("zbm.luks_token_wait", "0\n");
        case.prompt_delay = Duration::from_secs(1);
        case
    };
    let volume_prompts = |case: &Case<'_>| case.prompts("Unlock LUKS volume");
    let waited = |case: &Case<'_>| case.out().contains("Insert the key device");

    println!("### configuration parsing");
    checks.case("crypttab whose last line has no trailing newline");
    let c = case();
    c.crypttab_raw(&format!("v {gk} none"));
    c.unlock(&["pw"]);
    checks.check("mapped", c.mapped("v"));

    checks.case("crypttab with CRLF line endings");
    let c = case();
    c.crypttab_raw(&format!("v {gk} none discard\r\nw {l1} none\r\n"));
    c.unlock(&["pw", "pw"]);
    checks.check("v mapped", c.mapped("v"));
    checks.check("w mapped", c.mapped("w"));
    checks.check("discard still recognised", c.called("--allow-discards"));

    checks.case("crypttab with tabs, blank lines, indentation and comments");
    let c = case();
    c.crypttab_raw(&format!(
        "\n# comment\n   \n\tv\t\t{gk}   none\t discard  \n# trailing comment\n"
    ));
    c.unlock(&["pw"]);
    checks.check("mapped", c.mapped("v"));

    checks.case("uppercase UUID= in crypttab");
    let c = case();
    c.crypttab(&[&format!("v UUID={upper} none")]);
    c.unlock(&["pw"]);
    checks.check("mapped", c.mapped("v"));

    checks.case("uppercase bare UUID in zbm.luks");
    let c = case();
    c.cmdline("zbm.luks", &format!("{upper}\n"));
    c.no_crypttab();
    c.unlock(&["pw"]);
    checks.check(
        "mapped as luks-<lowercase UUID>",
        c.mapped(&format!("luks-{ugk}")),
    );

    let partitions = !partuuid.is_empty();
    if partitions {
        checks.case("uppercase PARTUUID= (as printed by sgdisk)");
        let c = case();
        c.crypttab(&[&format!("v PARTUUID={} none", partuuid.to_uppercase())]);
        c.unlock(&["pw"]);
        checks.check("mapped", c.mapped("v"));

        checks.case("PARTUUID= of a LUKS partition");
        let c = case();
        c.crypttab(&[&format!("v PARTUUID={partuuid} none")]);
        c.unlock(&["pw"]);
        checks.check("mapped", c.mapped("v"));
    } else {
        println!("  skip - PARTUUID cases: loop partitions are not available here");
    }

    checks.case("zbm.luks=luks-<UUID>");
    let c = case();
    c.cmdline("zbm.luks", &format!("luks-{ugk}\n"));
    c.no_crypttab();
    c.unlock(&["pw"]);
    checks.check("mapped", c.mapped(&format!("luks-{ugk}")));

    checks.case("same device listed three times in zbm.luks");
    let c = case();
    c.cmdline("zbm.luks", &format!("UUID={ugk},{gk},luks-{ugk}\n"));
    c.no_crypttab();
    c.unlock(&["pw"]);
    checks.check(
        "one mapping",
        c.mappings()
            .iter()
            .filter(|name| name.contains("luks-"))
            .count()
            == 1,
    );
    checks.check("one prompt", volume_prompts(&c) == 1);

    checks.case("same device under two names in crypttab");
    let c = case();
    c.crypttab(&[&format!("a {gk} none"), &format!("b UUID={ugk} none")]);
    c.unlock(&["pw"]);
    checks.check("a mapped", c.mapped("a"));
    checks.check("b not mapped", !c.mapped("b"));
    checks.check("one prompt", volume_prompts(&c) == 1);

    checks.case("mapping name already used by another device; next volume still unlocked");
    let c = case();
    let taken = c.mapper.join("taken").display().to_string();
    run("mknod", &[&taken, "b", "7", "250"], b"", &[]);
    c.crypttab(&[&format!("taken {gk} none"), &format!("w {l1} none")]);
    c.unlock(&["pw", "pw"]);
    checks.check(
        "error reported",
        c.log().contains("mapping taken already exists"),
    );
    checks.check("w mapped", c.mapped("w"));

    checks.case("unknown device specification; next volume still unlocked");
    let c = case();
    c.crypttab(&["x FOO=bar none", &format!("w {l1} none")]);
    c.unlock(&["pw"]);
    checks.check(
        "error reported",
        c.log().contains("unrecognized LUKS device specification"),
    );
    checks.check("w mapped", c.mapped("w"));

    checks.case("non-LUKS device in crypttab is skipped");
    let c = case();
    c.crypttab(&[
        &format!("x {not_luks} none nofail"),
        &format!("w {l1} none"),
    ]);
    c.unlock(&["pw"]);
    checks.check("warned", c.log().contains("is not a LUKS volume"));
    checks.check("w mapped", c.mapped("w"));
    checks.check(
        "no prompt for the non-LUKS device",
        !c.out().contains(&format!("Unlock LUKS volume {not_luks}")),
    );

    println!("### invalid and edge-value options");
    checks.case("non-numeric key-slot= is ignored, passphrase still works");
    let c = case();
    c.crypttab(&[&format!("v {gk} none key-slot=abc")]);
    c.unlock(&["pw"]);
    checks.check("mapped", c.mapped("v"));
    checks.check(
        "warned",
        c.log().contains("invalid LUKS option 'key-slot=abc'"),
    );

    checks.case("invalid keyfile-offset/keyfile-size are ignored");
    let c = case();
    c.crypttab(&[&format!(
        "v {gk} {} keyfile-offset=x,keyfile-size=-5",
        k("good.key")
    )]);
    c.unlock(&["pw"]);
    checks.check("mapped", c.mapped("v"));
    checks.check(
        "warned",
        c.log().contains("invalid LUKS option 'keyfile-offset=x'"),
    );

    checks.case("keyfile-offset + keyfile-size select the key inside a larger file");
    let c = case();
    c.crypttab(&[&format!(
        "v {gk} {} keyfile-offset=100,keyfile-size=64",
        k("padded.key")
    )]);
    c.unlock(&[]);
    checks.check("mapped without prompt", c.mapped("v"));

    checks.case("empty key file -> passphrase fallback");
    let c = case();
    c.crypttab(&[&format!("v {gk} {}", k("empty.key"))]);
    c.unlock(&["pw"]);
    checks.check("mapped", c.mapped("v"));

    checks.case("key file path is a directory -> passphrase fallback");
    let c = case();
    c.crypttab(&[&format!("v {gk} {}", k("dir.key"))]);
    c.unlock(&["pw"]);
    checks.check("mapped", c.mapped("v"));

    checks.case("invalid tries= keeps the default");
    let c = case();
    c.crypttab(&[&format!("v {gk} none tries=abc")]);
    c.unlock(&["x", "x", "pw"]);
    checks.check("mapped on third try", c.mapped("v"));

    checks.case("tries=1: one wrong passphrase, then retry offered");
    let c = case();
    c.crypttab(&[&format!("v {gk} none tries=1")]);
    c.unlock(&["x"]);
    checks.check(
        "retry/skip offered",
        c.timed_prompts().contains("timed_prompt"),
    );

    println!("### passphrases");
    checks.case("passphrase with spaces, quotes, backslash, $, unicode and trailing space");
    let c = case();
    c.crypttab(&[&format!("v {spec} none")]);
    c.unlock(&[SPECIAL]);
    checks.check("mapped", c.mapped("v"));

    checks.case("passphrase starting with a dash");
    let c = case();
    c.crypttab(&[&format!("v {lead} none")]);
    c.unlock(&["-starts-with-dash"]);
    checks.check("mapped", c.mapped("v"));

    checks.case("400-character passphrase");
    let c = case();
    c.crypttab(&[&format!("v {long} none")]);
    c.unlock(&[&long_passphrase]);
    checks.check("mapped", c.mapped("v"));

    checks.case("different passphrases: second volume prompts after cached one fails");
    let c = case();
    c.crypttab(&[&format!("v {gk} none"), &format!("w {lead} none")]);
    c.unlock(&["pw", "-starts-with-dash"]);
    checks.check("v mapped", c.mapped("v"));
    checks.check("w mapped", c.mapped("w"));

    println!("### key devices (USB sticks) and boot-time races");
    checks.case("key stick plugged in 3 s after unlocking starts");
    let c = case();
    c.cmdline("zbm.luks_token_wait", "5\n");
    c.crypttab(&[&format!("v {gk} /keys/gk.key:LABEL=FINALKEY")]);
    let stick_loop = thread::scope(|scope| {
        let late = scope.spawn(|| {
            thread::sleep(Duration::from_secs(3));
            let device = lab.attach(&stick, false);
            let _ = try_run("udevadm", &["settle"], b"", &[]);
            device
        });
        c.unlock_with(&LinesConsole::new(
            &[],
            Duration::ZERO,
            Duration::from_secs(20),
        ));
        late.join().unwrap()
    });
    checks.check("mapped from the stick, no passphrase prompt", c.mapped("v"));
    checks.check("no passphrase prompt", !c.out().contains("Passphrase"));
    checks.check("stick unmounted again", !mounted("luks-key"));
    for _ in 0..5 {
        if loops_of(&stick_text).is_empty() {
            break;
        }
        lab.detach(&stick_loop);
        thread::sleep(Duration::from_secs(1));
    }
    checks.check(
        "stick detached before the next case",
        loops_of(&stick_text).is_empty(),
    );

    checks.case("key stick absent -> short wait, then passphrase");
    let c = case();
    c.cmdline("zbm.luks_token_wait", "2\n");
    c.crypttab(&[&format!("v {gk} /keys/gk.key:LABEL=NOSUCHSTICK")]);
    c.unlock_with(&LinesConsole::new(
        &["pw"],
        Duration::from_secs(4),
        Duration::ZERO,
    ));
    checks.check("mapped via passphrase", c.mapped("v"));
    checks.check("waited for the stick", waited(&c));

    checks.case("key stick absent with zbm.luks_token_wait=0 -> no wait");
    let c = case();
    c.crypttab(&[&format!("v {gk} /keys/gk.key:LABEL=NOSUCHSTICK")]);
    c.unlock(&["pw"]);
    checks.check("mapped via passphrase", c.mapped("v"));
    checks.check("did not wait", !waited(&c));

    checks.case("zbm.luks_token_wait values: leading zeros, out of range, not a number");
    let c = case();
    let values = [
        "0",
        "7",
        "08",
        "010",
        "999999",
        "1000000",
        "-5",
        "+5",
        "abc",
        "1e3",
        "",
        "99999999999999999999",
    ];
    let expected = [
        "0", "7", "8", "10", "999999", "10", "10", "10", "10", "10", "10", "10",
    ];
    let console = LinesConsole::new(&[], Duration::ZERO, Duration::ZERO);
    let waits: Vec<String> = values
        .iter()
        .map(|value| {
            c.cmdline("zbm.luks_token_wait", value);
            c.with_session(&console, |session, _| session.token_wait().to_string())
        })
        .collect();
    for ((value, wait), want) in values.iter().zip(&waits).zip(expected) {
        checks.check(
            &format!("zbm.luks_token_wait [{value}]={want}"),
            wait == want,
        );
    }

    checks.case("key stick absent with zbm.luks_token_wait=08 -> waits 8 s, no arithmetic error");
    let c = case();
    c.cmdline("zbm.luks_token_wait", "08");
    c.crypttab(&[&format!("v {gk} /keys/gk.key:LABEL=NOSUCHSTICK")]);
    c.unlock_with(&LinesConsole::new(
        &["pw"],
        Duration::from_secs(10),
        Duration::ZERO,
    ));
    checks.check(
        "no base error",
        !c.out().contains("value too great for base"),
    );
    checks.check("waited 8 seconds", c.out().contains("Waiting 8 seconds"));
    checks.check("mapped after passphrase", c.mapped("v"));

    checks.case("two volumes on one missing stick: waits once, not per volume");
    let c = case();
    c.cmdline("zbm.luks_token_wait", "2\n");
    c.crypttab(&[
        &format!("v {gk} /keys/gk.key:LABEL=NOSUCHSTICK"),
        &format!("w {l1} /keys/l1.key:LABEL=NOSUCHSTICK"),
    ]);
    c.unlock_with(&LinesConsole::new(
        &["pw", "pw"],
        Duration::from_secs(4),
        Duration::ZERO,
    ));
    checks.check("v mapped", c.mapped("v"));
    checks.check("w mapped", c.mapped("w"));
    checks.check(
        "waited once",
        c.out().matches("Insert the key device").count() == 1,
    );

    checks.case("import retry with a missing stick does not wait");
    let c = case();
    c.cmdline("zbm.luks_token_wait", "5\n");
    c.crypttab(&[&format!("v {gk} /keys/gk.key:LABEL=NOSUCHSTICK")]);
    c.env.set("LUKS_RETRY", "1");
    c.unlock(&[]);
    checks.check("did not wait", !waited(&c));

    let stick_loop = lab.attach(&stick, false);
    checks.case("key stick present but key file missing on it");
    let c = case();
    c.crypttab(&[&format!("v {gk} /keys/missing.key:LABEL=FINALKEY")]);
    c.unlock(&["pw"]);
    checks.check("mapped via passphrase", c.mapped("v"));
    checks.check("warned", c.log().contains("not found on"));
    checks.check("stick unmounted", !mounted("luks-key"));
    lab.detach(&stick_loop);

    checks.finish(if partitions { 66 } else { 64 });
}
