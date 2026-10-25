use pretty_assertions::assert_eq;

use super::healthy::{Expect, cmdline, expect_healthy};
use super::recover::{answer_prompts, imported, skip_prompts, unlock};
use super::setup::{crypttab_lines, crypttab_text};
use super::*;
use crate::boot::Boot;
use crate::settings::{PW, PW_OTHER};
use crate::testing::{Step, checks, cleanup, scripted, settings};

/// ZFSBootMenu's status of tank with `members` disks in raidz2-0, then
/// mixed and parts in `other` states.
fn status(members: usize, other: &str) -> String {
    let mut lines = vec![
        "  pool: tank".to_owned(),
        " state: ONLINE".to_owned(),
        "config:".to_owned(),
        "\tNAME                    STATE     READ WRITE CKSUM".to_owned(),
        "\ttank                    ONLINE       0     0     0".to_owned(),
        "\t  raidz2-0              ONLINE       0     0     0".to_owned(),
    ];
    for member in 0..members {
        lines.push(format!(
            "\t    /dev/mapper/luks-{member}  ONLINE       0     0     0"
        ));
    }
    for (section, mirror) in [("special", 1), ("dedup", 2), ("logs", 3)] {
        lines.push(format!("\t{section}"));
        lines.push(format!("\t  mirror-{mirror}  ONLINE       0     0     0"));
        lines.push(format!("\t    /dev/mapper/luks-s{mirror}  ONLINE  0 0 0"));
    }
    lines.extend([
        "\tcache".to_owned(),
        "\t  /dev/mapper/luks-10   ONLINE       0     0     0".to_owned(),
        "\tspares".to_owned(),
        "\t  /dev/mapper/luks-11   AVAIL".to_owned(),
        "errors: No known data errors".to_owned(),
        "  pool: mixed".to_owned(),
        format!(" state: {other}"),
        "  pool: parts".to_owned(),
        format!(" state: {other}"),
    ]);
    lines.join("\n")
}

/// The boot environment's report with the features `features` active.
fn be_report(features: &[&str]) -> String {
    let mut lines = vec![
        "BE-KERNEL: 7.3.1-zbm".to_owned(),
        "BE-ZFS-VERSION: zfs-2.4.4-1 zfs-kmod-2.4.4-1".to_owned(),
        "BE-ROOT: tank/ROOT/be / zfs rw".to_owned(),
    ];
    for pool in POOLS {
        lines.push(format!("BE|   pool: {pool}"));
        lines.push("BE|  state: ONLINE".to_owned());
    }
    lines.push("BE-WRITE: ok".to_owned());
    lines.push("BE-SCRUB: tank pool 'tank' is healthy".to_owned());
    for feature in features {
        lines.push(format!("BE-F| tank feature@{feature} active"));
    }
    lines.join("\n")
}

/// A boot that showed `status` and `be`.
fn canned(status: String, be: String, prompts: &[&str], pools: &str) -> Boot {
    let mut boot = Boot {
        prompts: prompts.iter().map(|&prompt| prompt.to_owned()).collect(),
        zbm_status: status,
        teardown: Some((String::new(), pools.to_owned(), String::new())),
        be,
        ..Boot::default()
    };
    for line in boot.be.lines() {
        if let Some((name, value)) = line.split_once(": ")
            && !name.contains('|')
        {
            boot.be_lines
                .entry(name.to_owned())
                .or_default()
                .push(value.to_owned());
        }
    }
    boot
}

/// The checks of the boot environment that pass in [`be_report`].
const BE_CHECKS: [&str; 8] = [
    "ok   - kexec into the ZFS 2.4.4 boot environment",
    "ok   - boot environment kernel 7.3.1-zbm",
    "ok   - boot environment runs ZFS 2.4.4 (zfs-2.4.4-1 zfs-kmod-2.4.4-1)",
    "ok   - root file system tank/ROOT/be / zfs rw",
    "ok   - boot environment imported tank: ONLINE (expected ONLINE)",
    "ok   - boot environment imported mixed: ONLINE (expected ONLINE)",
    "ok   - boot environment imported parts: ONLINE (expected ONLINE)",
    "ok   - root dataset is writable in the boot environment",
];

#[test]
fn a_healthy_boot_passes_every_check_but_those_of_what_is_missing() {
    let settings = settings("complex-healthy");
    let test = Test::new(&settings, "complex-healthy").unwrap();
    let features = &NEW_FEATURES[..10];
    let boot = canned(
        status(5, "ONLINE"),
        be_report(features),
        &["luks", "luks", "zfskey", "be-zfskey"],
        "",
    );

    expect_healthy(&test, &boot, Expect::default()).unwrap();

    let mut expected: Vec<String> = [
        "ok   - ZFSBootMenu imported tank: ONLINE (expected ONLINE)",
        "ok   - ZFSBootMenu imported mixed: ONLINE (expected ONLINE)",
        "ok   - ZFSBootMenu imported parts: ONLINE (expected ONLINE)",
        "ok   - tank shows its special section",
        "ok   - tank shows its dedup section",
        "ok   - tank shows its logs section",
        "ok   - tank shows its cache section",
        "ok   - tank shows its spares section",
        "ok   - cache vdev ONLINE",
        "ok   - hot spare AVAIL",
        "ok   - raidz2-0 has five members ONLINE after RAIDZ expansion: \
         ['ONLINE', 'ONLINE', 'ONLINE', 'ONLINE', 'ONLINE']",
        "ok   - teardown exported pools and closed volumes (dm=[] pools=[] state=[])",
        "ok   - boot environment asked for the ZFS key of its encrypted root",
    ]
    .iter()
    .chain(&BE_CHECKS)
    .map(|&line| line.to_owned())
    .collect();
    expected.push("ok   - scrub of tank in the boot environment: pool 'tank' is healthy".into());
    for feature in features {
        expected.push(format!(
            "ok   - boot environment sees tank feature {feature} active"
        ));
    }
    expected.push("FAIL - boot environment sees tank feature encryption active".into());
    assert_eq!(checks(&test), expected);
    cleanup(&settings);
}

#[test]
fn a_degraded_boot_skips_the_vdev_checks_of_tank() {
    let settings = settings("complex-degraded");
    let test = Test::new(&settings, "complex-degraded").unwrap();
    let boot = canned(
        status(4, "DEGRADED").replace(" state: ONLINE", " state: DEGRADED"),
        be_report(&NEW_FEATURES),
        &["luks"],
        "mixed parts",
    );
    let states = [("mixed", "DEGRADED"), ("parts", "DEGRADED")];
    let expect = Expect {
        states: &states,
        be_states: Some(&[]),
        readonly: &["mixed"],
    };

    expect_healthy(&test, &boot, expect).unwrap();

    let checks = checks(&test);
    assert_eq!(
        checks[..4],
        [
            "FAIL - ZFSBootMenu imported tank: DEGRADED (expected ONLINE)",
            "ok   - ZFSBootMenu imported mixed: DEGRADED (expected DEGRADED)",
            "ok   - ZFSBootMenu imported parts: DEGRADED (expected DEGRADED)",
            "ok   - tank shows its special section",
        ]
    );
    assert_eq!(
        checks[10..15],
        [
            "FAIL - raidz2-0 has five members ONLINE after RAIDZ expansion: \
             ['ONLINE', 'ONLINE', 'ONLINE', 'ONLINE']",
            "FAIL - teardown exported pools and closed volumes \
             (dm=[] pools=[mixed parts] state=[])",
            "FAIL - boot environment asked for the ZFS key of its encrypted root",
            "ok   - kexec into the ZFS 2.4.4 boot environment",
            "ok   - boot environment kernel 7.3.1-zbm",
        ]
    );
    assert_eq!(checks.iter().filter(|c| c.starts_with("FAIL")).count(), 4);
    cleanup(&settings);
}

#[test]
fn the_vdev_checks_of_tank_are_left_out_when_tank_is_expected_degraded() {
    let settings = settings("complex-tank-degraded");
    let test = Test::new(&settings, "complex-degraded").unwrap();
    let boot = canned(String::new(), String::new(), &[], "");
    let expect = Expect {
        states: &[("tank", "DEGRADED")],
        ..Expect::default()
    };

    expect_healthy(&test, &boot, expect).unwrap();

    let checks = checks(&test);
    assert_eq!(
        checks[0],
        "FAIL - ZFSBootMenu imported tank: None (expected DEGRADED)"
    );
    assert!(!checks.iter().any(|check| check.contains("cache vdev")));
    cleanup(&settings);
}

#[test]
fn the_boot_environment_command_line_lists_the_volumes_it_waits_for() {
    let volumes = vec!["u0".to_owned(), "u14".to_owned()];

    let all = cmdline(&volumes, Some("u12"), Some("u15"));
    let without = cmdline(&volumes, None, None);

    assert_eq!(
        all,
        "console=ttyS0 loglevel=4 rd.luks.key=/etc/zbmtest/be.key rd.luks.uuid=u0 \
         rd.luks.uuid=u14 rd.luks.key=u12=/etc/zbmtest/be-other.key rd.luks.uuid=u12 \
         rd.luks.uuid=u15"
    );
    assert_eq!(
        without,
        "console=ttyS0 loglevel=4 rd.luks.key=/etc/zbmtest/be.key rd.luks.uuid=u0 \
         rd.luks.uuid=u14"
    );
}

#[test]
fn the_crypttab_keeps_the_entries_of_the_output() {
    let out = "for i in\nluks-a UUID=a none discard\nRC=0\nluks-b ID=virtio-d1 none discard";

    let lines = crypttab_lines(out);

    assert_eq!(
        lines,
        [
            "luks-a UUID=a none discard",
            "luks-b ID=virtio-d1 none discard"
        ]
    );
    assert_eq!(
        crypttab_text(&lines),
        "luks-a UUID=a none discard\nluks-b ID=virtio-d1 none discard\n"
    );
}

#[test]
fn skipped_prompts_end_in_the_emergency_shell() {
    let settings = settings("complex-skip");
    let test = Test::new(&settings, "complex-skip-recover").unwrap();
    let script = vec![
        Step::Say("Passphrase (leave empty to skip): "),
        Step::Hear(""),
        Step::Say("Passphrase (leave empty to skip): "),
        Step::Hear(""),
        Step::Say("Dropping to an emergency shell\r\n"),
    ];
    let (mut vm, guest) = scripted(&test, script);

    let ended = skip_prompts(&mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(ended, (2, 2));
    cleanup(&settings);
}

#[test]
fn a_menu_without_boot_environments_opens_the_shell() {
    let settings = settings("complex-skip-menu");
    let test = Test::new(&settings, "complex-skip-recover").unwrap();
    let script = vec![
        Step::Say("Passphrase (leave empty to skip): "),
        Step::Hear(""),
        Step::Say("\x1b[1;1H BOOT ENVIRONMENTS \x1b[3;3HNo Boot Environments\x1b[4;3H"),
        Step::Keys("!\r"),
    ];
    let (mut vm, guest) = scripted(&test, script);

    let ended = skip_prompts(&mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(ended, (1, 2));
    let steps = std::fs::read_to_string(test.dir().join("steps.log")).unwrap();
    assert!(
        steps.contains("] menu -> ! Enter: the recovery shell\n"),
        "{steps}"
    );
    cleanup(&settings);
}

#[test]
fn zluks_unlock_gets_the_passphrase_then_the_other_one() {
    let settings = settings("complex-unlock");
    let test = Test::new(&settings, "complex-skip-recover").unwrap();
    let script = vec![
        Step::Hear("zluks unlock; echo RC=$?"),
        Step::Say("Passphrase (leave empty to skip): "),
        Step::Hear(PW),
        Step::Say("Passphrase (leave empty to skip): "),
        Step::Hear(PW_OTHER),
        Step::Say("unlocked 15 volumes\r\nRC=0\r\n"),
    ];
    let (mut vm, guest) = scripted(&test, script);

    let unlocked = unlock(&mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(unlocked, (2, "0".to_owned()));
    cleanup(&settings);
}

#[test]
fn the_older_image_gets_both_passphrases_then_the_first_again() {
    let settings = settings("complex-oldzfs");
    let test = Test::new(&settings, "complex-oldzfs").unwrap();
    let script = vec![
        Step::Say("Passphrase (leave empty to skip): "),
        Step::Hear(PW),
        Step::Say("Passphrase (leave empty to skip): "),
        Step::Hear(PW_OTHER),
        Step::Say("Passphrase (leave empty to skip): "),
        Step::Hear(PW),
        Step::Say("Unable to import tank, retrying in 4 seconds\r\n"),
    ];
    let (mut vm, guest) = scripted(&test, script);

    let ended = answer_prompts(&mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(ended, 1);
    cleanup(&settings);
}

#[test]
fn only_mixed_and_parts_online_count_as_imported() {
    let list = "zfs-2.3.0-1\nparts\tONLINE\toff\nmixed\tONLINE\toff\nxmixed\tONLINE\n\
                tank\tFAULTED\nmixed2 DEGRADED";

    assert_eq!(imported(list).unwrap(), ["mixed", "parts"]);
}

#[test]
fn the_members_of_a_vdev_are_counted_to_the_next_vdev() {
    let six = status(6, "ONLINE");
    assert_eq!(super::healthy::vdev_members(&six, "raidz2-0").len(), 6);
    assert_eq!(super::healthy::vdev_members(&six, "mirror-1"), ["ONLINE"]);
    assert!(super::healthy::vdev_members(&six, "raidz3-0").is_empty());
}
