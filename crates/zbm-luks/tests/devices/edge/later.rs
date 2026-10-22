//! The later sections of the edge suite: races, retries of the import loop,
//! failure paths and teardown matching.

use std::thread;
use std::time::Duration;

use zbm_platform::exec::{Command, Input, Runner};

use super::{Volumes, new_case, no_plugins};
use crate::harness::{Checks, Format, Lab, LinesConsole, has_line, no_leftovers, try_run};
use zbm_luks::mappings::Origin;

/// Runs the later sections with the volumes of the first.
pub(super) fn races_and_failures(lab: &Lab, checks: &mut Checks, volumes: &Volumes) {
    let Volumes {
        zfs,
        multi,
        stale,
        fido_only,
        plain,
        l1,
        gnupg,
    } = volumes;
    let case = |zfs: &[String]| new_case(lab, zfs);
    println!("### races");
    checks.case("device appears 2 s after unlock starts (crypttab)");
    let c = case(zfs);
    c.crypttab(&["v LABEL=zfsLATE none"]);
    c.answers("0\n0\n0\n0\n");
    thread::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(Duration::from_secs(2));
            let late = lab.image("late.img", 32);
            lab.format(
                &late.display().to_string(),
                b"pw",
                Format::Luks2,
                Some("zfsLATE"),
                None,
            );
            lab.attach(&late, false);
        });
        c.unlock(&["pw"]);
    });
    checks.check("mapped", c.mapped("v"));
    checks.check(
        "missing-device prompt shown first",
        c.timed_prompts().contains("zfsLATE is missing"),
    );

    checks.case("FIDO2 key plugged in 2 s into the wait");
    let c = case(zfs);
    no_plugins(&c);
    c.cmdline("zbm.luks_token_wait", "5\n");
    std::fs::rename("/dev/hidraw99", "/dev/xhid").unwrap();
    c.crypttab(&[&format!("v {fido_only} none")]);
    thread::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(Duration::from_secs(2));
            std::fs::rename("/dev/xhid", "/dev/hidraw99").unwrap();
        });
        c.unlock(&[]);
    });
    checks.check("mapped", c.mapped("v"));
    checks.check("waited", c.out().contains("Insert the FIDO2 security key"));

    checks.case("volume unlocked from another session while the prompt waits");
    let c = case(zfs);
    c.crypttab(&[&format!("v {plain} none")]);
    let rc = thread::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(Duration::from_secs(3));
            let open = Command::new("cryptsetup")
                .args(["open", "--key-file=-", plain, "ext"])
                .stdin(Input::Bytes(b"pw".to_vec()));
            let _ = c.runner.run(&open);
        });
        c.unlock_with(&LinesConsole::new(
            &[],
            Duration::ZERO,
            Duration::from_secs(15),
        ))
    });
    checks.check("rc 0", rc == 0);
    checks.check("noticed", c.log().contains("unlocked elsewhere"));
    checks.check("not recorded as ours", c.state().is_empty());

    checks.case("two sessions unlock the same volume at once");
    let c = case(zfs);
    c.crypttab(&[&format!("v {plain} none")]);
    thread::scope(|scope| {
        scope.spawn(|| c.unlock(&["pw"]));
        scope.spawn(|| c.unlock(&["pw"]));
    });
    checks.check(
        "exactly one mapping",
        c.mappings()
            .iter()
            .filter(|name| *name != "control")
            .count()
            == 1,
    );
    checks.check(
        "recorded once",
        c.state()
            .lines()
            .filter(|line| line.starts_with('v'))
            .count()
            == 1,
    );
    checks.check(
        "no ERROR crash",
        !c.out().contains("syntax") && !c.out().contains("command not found"),
    );

    checks.case("four sessions record the same mapping at once, 50 rounds");
    let c = case(zfs);
    let state = c.base.join("luks-mappings");
    let mut duplicate = false;
    for _ in 0..50 {
        let _ = std::fs::remove_file(&state);
        thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    let console = LinesConsole::new(&[], Duration::ZERO, Duration::ZERO);
                    c.with_session(&console, |session, _| {
                        session
                            .mappings()
                            .record("v", "/dev/loop99", Origin::Configured)
                    });
                });
            }
        });
        duplicate |= c
            .state()
            .lines()
            .filter(|line| line.starts_with('v'))
            .count()
            != 1;
    }
    checks.check("never recorded twice", !duplicate);
    checks.check("no lock left", !c.base.join("luks-mappings.lock").exists());

    checks.case("mapping names are compared as text, not as patterns");
    let c = case(zfs);
    let console = LinesConsole::new(&[], Duration::ZERO, Duration::ZERO);
    c.with_session(&console, |session, _| {
        session
            .mappings()
            .record("axb", "/dev/loop98", Origin::Configured);
        session
            .mappings()
            .record("a.b", "/dev/loop99", Origin::Configured);
        session
            .mappings()
            .record("a*", "/dev/loop97", Origin::Configured);
        session
            .mappings()
            .record("a.b", "/dev/loop96", Origin::Configured);
    });
    checks.check(
        "a.b recorded after axb",
        has_line(&c.state(), "a.b\t/dev/loop99\t0"),
    );
    checks.check("a* recorded", has_line(&c.state(), "a*\t/dev/loop97\t0"));
    checks.check(
        "a.b recorded once",
        c.state()
            .lines()
            .filter(|line| line.starts_with("a.b"))
            .count()
            == 1,
    );

    println!("### import-loop retries (late disks)");
    checks.case("volume skipped by the user is not prompted again on retry");
    let c = case(zfs);
    c.crypttab(&[&format!("v {plain} none")]);
    c.unlock(&[""]);
    c.env.set("LUKS_RETRY", "1");
    let rc = c.unlock(&["pw"]);
    checks.check("retry rc 1 (nothing new)", rc == 1);
    checks.check("only one prompt", c.prompts("Passphrase (leave") == 1);
    checks.check("not mapped", !c.mapped("v"));

    checks.case("retry: device missing -> no wait; device appears -> unlocked");
    let c = case(zfs);
    c.crypttab(&["v LABEL=zfsLATE2 none"]);
    c.env.set("LUKS_RETRY", "1");
    let first = c.unlock(&[]);
    let late = lab.image("late2.img", 32);
    lab.format(
        &late.display().to_string(),
        b"pw",
        Format::Luks2,
        Some("zfsLATE2"),
        None,
    );
    let late_device = lab.attach(&late, false);
    c.content(&late_device, "zfs_member");
    let second = c.unlock(&["pw"]);
    checks.check(
        "missing device: no wait prompt",
        c.timed_prompts().is_empty(),
    );
    checks.check("first retry rc 1, second rc 0", first == 1 && second == 0);
    checks.check("mapped", c.mapped("v"));

    checks.case("retry with everything already open does nothing");
    let c = case(zfs);
    c.crypttab(&[&format!("a {plain} none"), &format!("b {l1} none")]);
    c.unlock(&["pw"]);
    c.env.set("LUKS_RETRY", "1");
    let rc = c.unlock(&[]);
    checks.check("rc 1", rc == 1);
    checks.check("one prompt only", c.prompts("Passphrase (leave") == 1);

    checks.case("retry with zbm.luks=off reports nothing new (no endless import loop)");
    let c = case(zfs);
    c.cmdline("zbm.luks", "off\n");
    c.env.set("LUKS_RETRY", "1");
    checks.check("retry rc 1", c.unlock(&[]) == 1);

    checks.case("initial run with zbm.luks=off still succeeds");
    let c = case(zfs);
    c.cmdline("zbm.luks", "off\n");
    checks.check("rc 0", c.unlock(&[]) == 0);

    println!("### failure paths");
    checks.case("everything fails on EOF: nothing mapped, nothing left behind");
    let c = case(zfs);
    no_plugins(&c);
    c.env.set("LUKS_TPM2_TCTI", "device:/dev/no-such-tpm");
    c.env.set("LUKS_GNUPG_DIR", gnupg.only_b.as_str());
    std::fs::rename("/dev/hidraw99", "/dev/xhid").unwrap();
    c.crypttab(&[
        &format!("a {multi} {}", gnupg.encrypted),
        &format!("b {stale} none"),
    ]);
    let rc = c.unlock(&[]);
    std::fs::rename("/dev/xhid", "/dev/hidraw99").unwrap();
    checks.check("rc nonzero", rc != 0);
    checks.check(
        "nothing mapped",
        c.mappings().iter().all(|name| name == "control"),
    );
    checks.check("no leftovers", no_leftovers());
    let agents = try_run("pgrep", &["-f", "[g]pg-agent.*zbm-gpg"], b"", &[]);
    checks.check("no gpg agents left running", !agents.status.success());

    println!("### teardown matching");
    checks.case("pool on a partition of a mapping and a mixed LUKS/plain pool");
    let c = case(zfs);
    c.crypttab(&[&format!("a1 {plain} none"), &format!("b1 {l1} none")]);
    c.unlock(&["pw"]);
    let mapper = |name: &str| c.mapper.join(name).display().to_string();
    c.pool("onpart", &[&mapper("a1p1")]);
    c.pool("mixed", &[&mapper("b1"), "/dev/vda2"]);
    c.pool("plainpool", &["/dev/vdb"]);
    c.close(None);
    checks.check(
        "onpart exported",
        c.exported().iter().any(|pool| pool == "onpart"),
    );
    checks.check(
        "mixed exported",
        c.exported().iter().any(|pool| pool == "mixed"),
    );
    checks.check("plain pool untouched", c.has_pool("plainpool"));
    checks.check("a1 closed", !c.mapped("a1"));
    checks.check("b1 closed", !c.mapped("b1"));
}
