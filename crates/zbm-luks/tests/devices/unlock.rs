//! Core unlocking and teardown, the Rust port of the Bash suite `unlock.sh`:
//! auto, command line and crypttab modes, key devices, key scripts, and
//! closing volumes before booting.

use std::time::Duration;

use crate::harness::{
    Case, Checks, Format, Lab, LinesConsole, enabled, has_line, mounted, run, script,
};

/// `ls /dev/mapper | grep -q <text>`.
fn listed(case: &Case<'_>, text: &str) -> bool {
    case.mappings().iter().any(|name| name.contains(text))
}

#[test]
fn unlock() {
    if !enabled("unlock") {
        return;
    }
    let lab = Lab::new("unlock", None);
    let dev_a = lab.luks("zfsA", b"secret1", Format::Luks2, true);
    let dev_b = lab.luks("zfsB", b"secret1", Format::Luks2, true);
    let dev_c = lab.luks("extC", b"other", Format::Luks2, true);
    let plain = lab.image("plain.img", 16);
    let dev_d = lab.attach(&plain, false);
    // key device: ext4 holding /keys/zfsA.key == "secret1"
    let key_image = lab.image("key.img", 16);
    let key_image_text = key_image.display().to_string();
    run(
        "mkfs.ext4",
        &["-q", "-L", "ZBMKEY", &key_image_text],
        b"",
        &[],
    );
    let dev_k = lab.attach(&key_image, false);
    let mount = lab.path("m");
    std::fs::create_dir_all(&mount).unwrap();
    let mount_text = mount.display().to_string();
    run("mount", &[&dev_k, &mount_text], b"", &[]);
    std::fs::create_dir_all(mount.join("keys")).unwrap();
    std::fs::write(mount.join("keys/zfsA.key"), "secret1").unwrap();
    run("umount", &[&mount_text], b"", &[]);
    let uuid_a = Lab::uuid(&dev_a);
    let uuid_b = Lab::uuid(&dev_b);
    let uuid_c = Lab::uuid(&dev_c);
    let mut checks = Checks::new("unlock");

    let case = || {
        let case = lab.case();
        case.content(&dev_a, "zfs_member");
        case.content(&dev_b, "zfs_member");
        case.content(&dev_c, "ext4");
        case
    };
    let npw = |case: &Case<'_>| case.prompts("Passphrase (leave empty");

    // ------------------------------------------------------------ auto mode
    checks.case("auto: shared passphrase prompts once; non-ZFS volume closed");
    let c = case();
    let rc = c.unlock(&["secret1", "other"]);
    checks.check("rc 0", rc == 0);
    checks.check("A mapped as luks-UUID", c.mapped(&format!("luks-{uuid_a}")));
    checks.check(
        "B mapped (cached passphrase)",
        c.mapped(&format!("luks-{uuid_b}")),
    );
    checks.check("C (ext4) closed again", !listed(&c, &uuid_c));
    checks.check("two passphrase prompts total (A, C)", npw(&c) == 2);
    checks.check(
        "state lists A",
        c.state()
            .lines()
            .any(|line| line.starts_with(&format!("luks-{uuid_a}\t"))),
    );
    checks.check("state omits C", !c.state().contains(&uuid_c));
    checks.check(
        "plain device D never touched",
        !c.calls()
            .iter()
            .any(|call| call.contains("open") && call.contains(&dev_d)),
    );

    checks.case("auto: empty passphrase skips volumes");
    let c = case();
    c.unlock(&["", "", ""]);
    checks.check("nothing mapped", c.mappings().is_empty());
    checks.check("skip logged", c.log().contains("skipping LUKS volume"));

    checks.case("auto: EOF on console fails gracefully");
    let c = case();
    c.unlock(&[]);
    checks.check("nothing mapped", c.mappings().is_empty());
    checks.check(
        "no console logged",
        c.log().contains("no console available"),
    );

    checks.case("auto: wrong passphrases exhaust tries, ESC skips");
    let c = case();
    c.answers("1\n");
    c.env.set("LUKS_DEFAULT_TRIES", "2");
    c.unlock(&["bad1", "bad2", "", ""]);
    checks.check(
        "retry prompt shown",
        c.timed_prompts().contains("Unable to unlock"),
    );
    checks.check(
        "error shown on 2nd try",
        c.out().contains("No key available"),
    );
    checks.check("A not mapped", !c.mapped(&format!("luks-{uuid_a}")));

    checks.case("auto: RETURN after failure retries and succeeds");
    let c = case();
    c.answers("0\n");
    c.env.set("LUKS_DEFAULT_TRIES", "1");
    c.unlock(&["bad", "secret1", "other"]);
    checks.check("A mapped after retry", c.mapped(&format!("luks-{uuid_a}")));

    // ---------------------------------------------------------- cmdline mode
    checks.case("zbm.luks=off disables everything");
    let c = case();
    c.cmdline("zbm.luks", "off\n");
    c.unlock(&[]);
    checks.check("no cryptsetup open calls", !c.called("cryptsetup open"));

    checks.case("zbm.luks=UUID=...,missing: unlock listed, prompt for missing");
    let c = case();
    c.cmdline(
        "zbm.luks",
        &format!("UUID={uuid_a},PARTLABEL=does-not-exist\n"),
    );
    c.answers("1\n");
    let rc = c.unlock(&["secret1"]);
    checks.check("A mapped", c.mapped(&format!("luks-{uuid_a}")));
    checks.check("B not mapped (not listed)", !listed(&c, &uuid_b));
    checks.check(
        "missing device prompt",
        c.timed_prompts().contains("does-not-exist is missing"),
    );
    checks.check("rc 0 (one unlocked)", rc == 0);

    checks.case("zbm.luks=luks-<uuid> accepted");
    let c = case();
    c.cmdline("zbm.luks", &format!("luks-{uuid_a}\n"));
    c.unlock(&["secret1"]);
    checks.check("A mapped", c.mapped(&format!("luks-{uuid_a}")));

    checks.case("zbm.luks_key=path:LABEL= unlocks from key device");
    let c = case();
    c.cmdline("zbm.luks", &format!("{dev_a}\n"));
    c.cmdline("zbm.luks_key", "/keys/zfsA.key:LABEL=ZBMKEY\n");
    c.unlock(&[]);
    checks.check(
        "A mapped without prompt",
        c.mapped(&format!("luks-{uuid_a}")),
    );
    checks.check("no passphrase prompt", npw(&c) == 0);
    checks.check("key device unmounted", !mounted("/tmp/luks-key"));

    // --------------------------------------------------------- crypttab mode
    checks.case("crypttab: key file, prompt w/ retry, noauto, nofail");
    let c = case();
    let plainkey = c.fake.join("plainkey");
    std::fs::write(&plainkey, "secret1").unwrap();
    let plainkey = plainkey.display().to_string();
    c.crypttab(&[
        "# name      device              key                  options",
        &format!("pool-a      UUID={uuid_a}        {plainkey}       discard,no-read-workqueue"),
        &format!("pool-b      {dev_b}              none                 tries=2"),
        &format!("ext-c       {dev_c}              -                    noauto"),
        "gone        PARTLABEL=nothere   none                 nofail",
    ]);
    let rc = c.unlock(&["wrong", "secret1"]);
    checks.check("pool-a mapped by keyfile", c.mapped("pool-a"));
    let discard =
        format!("open --type luks --key-file={plainkey} --allow-discards --perf-no_read_workqueue");
    checks.check(
        "discard flag passed",
        c.calls()
            .iter()
            .any(|call| call.contains(&discard) && call.contains("pool-a")),
    );
    checks.check("pool-b mapped on 2nd attempt", c.mapped("pool-b"));
    checks.check("ext-c not opened (noauto)", !c.mapped("ext-c"));
    checks.check(
        "nofail missing device: no prompt",
        !c.timed_prompts().contains("nothere"),
    );
    checks.check(
        "keyfile-unlocked A pass NOT cached -> B prompted",
        npw(&c) == 2,
    );
    checks.check("rc 0", rc == 0);

    checks.case("crypttab: keyscript");
    let c = case();
    let keyscript = c.fake.join("ks");
    script(
        &keyscript,
        "#!/bin/sh\n[ \"$CRYPTTAB_NAME\" = ks-a ] && printf secret1\n",
    );
    c.crypttab(&[&format!(
        "ks-a {dev_a} none keyscript={}",
        keyscript.display()
    )]);
    c.unlock(&[]);
    checks.check("ks-a mapped via keyscript", c.mapped("ks-a"));

    checks.case("crypttab: missing key file falls back to passphrase");
    let c = case();
    c.crypttab(&[&format!("k {dev_a} /no/such/key")]);
    c.unlock(&["secret1"]);
    checks.check("k mapped", c.mapped("k"));
    checks.check(
        "warning logged",
        c.log().contains("key file /no/such/key not found"),
    );

    checks.case("crypttab: duplicate device detected");
    let c = case();
    c.crypttab(&[&format!("a1 {dev_a}"), &format!("a2 UUID={uuid_a}")]);
    c.unlock(&["secret1"]);
    checks.check("a1 mapped", c.mapped("a1"));
    checks.check("a2 not mapped", !c.mapped("a2"));
    checks.check(
        "duplicate warning",
        c.log().contains("configured more than once"),
    );

    checks.case("already-open device is skipped on second run");
    let c = case();
    c.crypttab(&[&format!("a1 {dev_a}")]);
    c.unlock(&["secret1"]);
    c.unlock(&[]);
    checks.check("skip logged", c.log().contains("already in use"));
    checks.check(
        "a1 recorded once",
        c.state()
            .lines()
            .filter(|line| line.starts_with("a1"))
            .count()
            == 1,
    );

    // --------------------------------------------------------------- teardown
    checks.case("teardown: export LUKS pools, close mappings, leave others");
    let c = case();
    c.crypttab(&[&format!("a1 {dev_a}"), &format!("b1 {dev_b}")]);
    c.unlock(&["secret1"]);
    let a1 = c.mapper.join("a1").display().to_string();
    let b1 = c.mapper.join("b1").display().to_string();
    c.pool("zroot", &[&a1]);
    c.pool("tank", &[&b1]);
    c.pool("other", &["/dev/vda"]);
    c.close(None);
    checks.check(
        "zroot exported",
        c.exported().iter().any(|pool| pool == "zroot"),
    );
    checks.check(
        "tank exported",
        c.exported().iter().any(|pool| pool == "tank"),
    );
    checks.check("other pool untouched", c.has_pool("other"));
    checks.check("a1 closed", !c.mapped("a1"));
    checks.check("b1 closed", !c.mapped("b1"));
    checks.check("state emptied", c.state().is_empty());

    checks.case("teardown: busy mapping kept in state, rc 1");
    let c = case();
    c.crypttab(&[&format!("a1 {dev_a}"), &format!("b1 {dev_b}")]);
    c.unlock(&["secret1"]);
    c.busy("b1");
    let rc = c.close(None);
    checks.check("rc 1", rc == 1);
    checks.check("a1 closed", !c.mapped("a1"));
    checks.check(
        "b1 still recorded",
        c.state().lines().any(|line| line.starts_with("b1")),
    );

    checks.case("zluks close <name> closes only that mapping");
    let c = case();
    c.crypttab(&[&format!("a1 {dev_a}"), &format!("b1 {dev_b}")]);
    c.unlock(&["secret1"]);
    c.close(Some("b1"));
    checks.check("a1 still open", c.mapped("a1"));
    checks.check("b1 closed", !c.mapped("b1"));

    // ------------------------------------------------------------- resolution
    checks.case("device spec resolution");
    let c = case();
    let console = LinesConsole::new(&[], Duration::ZERO, Duration::ZERO);
    let resolved = c.with_session(&console, |_, system| {
        let resolve = |spec: &str| {
            zbm_luks::device::resolve_device(system, spec).map(|device| device.to_string())
        };
        resolve("LABEL=zfsA").as_deref() == Some(dev_a.as_str())
            && resolve("label=zfsA").as_deref() == Some(dev_a.as_str())
            && resolve(&uuid_a).as_deref() == Some(dev_a.as_str())
            && resolve("LABEL=ZBMKEY").as_deref() == Some(dev_k.as_str())
            && resolve("BOGUS=1").is_none()
            && resolve("notauuid").is_none()
    });
    checks.check("LABEL/label/bare UUID resolve; bogus rejected", resolved);
    let _ = has_line;

    checks.finish(52);
}
