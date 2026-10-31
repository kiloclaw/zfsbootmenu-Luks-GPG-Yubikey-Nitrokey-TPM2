use std::fmt::Write as _;

use pretty_assertions::assert_eq;
use sha2::{Digest as _, Sha256};

use super::*;
use crate::testing::{Step, checks, cleanup, scripted, settings};

#[test]
fn the_other_host_id_is_written_little_endian() {
    assert_eq!(
        foreign_import(),
        r"printf '\xef\xbe\xad\xde' > /etc/hostid && zpool import -N tforeign"
    );
}

/// What the boot of `name` showed: ZFSBootMenu's pool status and prompts.
fn boot(status: &str, prompts: &[&str]) -> Boot {
    Boot {
        zbm_status: status.into(),
        prompts: prompts.iter().map(|&prompt| prompt.to_owned()).collect(),
        ..Boot::default()
    }
}

#[test]
fn a_volume_that_unlocked_itself_holds_start_up_to_its_pool() {
    let settings = settings("integrity-unattended-checks");
    let test = Test::new(&settings, "integrity-unattended").unwrap();
    let status = "  pool: tauto\n state: ONLINE\n  pool: tplain\n state: ONLINE";
    let log = format!("ZFSBootMenu: the volumes unlocked themselves: {HELD}\n");

    check_unlocked_itself(&test, &boot(status, &[]), &log);
    // Asked, not held, and the other host's pool adopted: every check fails.
    let adopted = format!("{status}\n  pool: tforeign\n state: ONLINE");
    check_unlocked_itself(&test, &boot(&adopted, &["tpm2pin"]), "");
    check_waits(
        &test,
        "",
        &format!("ZFSBootMenu: tplain/ROOT/be {WAITS}: it boots only when chosen\n"),
    );
    check_waits(
        &test,
        "Booting vmlinuz for tplain/ROOT/be\nBE-KERNEL 7.3\n",
        "",
    );
    check_typed(
        &test,
        &boot(status, &["luks"]),
        "ZFSBootMenu: running init stage\n",
    );
    check_typed(&test, &boot(status, &[]), &log);
    // A boot that saved no kernel log says nothing of its defaults.
    check_typed(&test, &boot(status, &["luks"]), "");

    let results: Vec<bool> = checks(&test)
        .iter()
        .map(|line| line.starts_with("ok"))
        .collect();
    assert_eq!(&results[..3], [true, true, true]);
    assert_eq!(&results[3..6], [false, false, false]);
    assert_eq!(&results[6..8], [true, true]);
    assert_eq!(&results[8..10], [false, false]);
    assert_eq!(&results[10..12], [true, true]);
    assert_eq!(&results[12..14], [false, false]);
    assert_eq!(&results[14..], [true, false]);
    cleanup(&settings);
}

#[test]
fn under_lockdown_the_kernel_must_not_load() {
    let settings = settings("integrity-lockdown-checks");
    let test = Test::new(&settings, "integrity-lockdown").unwrap();
    let log = format!("ZFSBootMenu: {REFUSED} /mnt/boot/vmlinuz\n{NOT_LOADED}\n");

    check_lockdown(&test, true, &log);
    check_lockdown(&test, false, "BE-KERNEL 7.3\n");

    let results: Vec<bool> = checks(&test)
        .iter()
        .map(|line| line.starts_with("ok"))
        .collect();
    assert_eq!(results, [true, true, false, false]);
    cleanup(&settings);
}

#[test]
fn after_the_refusal_the_failed_boot_is_closed_and_the_kernel_log_read() {
    let settings = settings("integrity-lockdown-shell");
    let test = Test::new(&settings, "integrity-lockdown").unwrap();
    let script = vec![
        Step::Say(
            "\x1b[2J\x1b[1;1H BOOT ENVIRONMENTS \x1b[5;5H✗ unable to boot tplain/ROOT/be\r\n",
        ),
        Step::Keys("\r"),
        Step::Keys("!\r"),
        Step::Say("zfsbootmenu ~ > "),
        Step::Hear("stty -echo cols 250; export PS1='zbm# ' PS2=''"),
        Step::Hear("true; echo RC=$?"),
        Step::Say("RC=0\r\n"),
        Step::Hear(KERNEL_LOG_RC),
        Step::Say("[ 9.1] ZFSBootMenu: lockdown is on: the kernel refused /mnt/boot/vmlinuz\r\n"),
        Step::Say("RC=0\r\n"),
    ];
    assert_eq!(KERNEL_LOG_RC, format!("{}; echo RC=$?", logs::KERNEL_LOG));
    let (mut vm, guest) = scripted(&test, script);

    let log = after_refusal(&mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    cleanup(&settings);
    assert!(log.contains(REFUSED), "{log}");
}

/// The line the shell hears for [`logs::KERNEL_LOG`].
const KERNEL_LOG_RC: &str = "dmesg | grep -F -e 'host ID alone' -e 'unlocked itself' \
                             -e 'lockdown is on'; echo RC=$?";

/// The SHA-256 digest of `parts`, one after the other.
fn sha256(parts: &[&[u8]]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().to_vec()
}

/// The lower-case hexadecimal of `bytes`.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        write!(text, "{byte:02x}").unwrap();
        text
    })
}

/// A measured boot of the kernel `KERNEL` and initramfs `INITRD`, with
/// the command line `root=zfs:tauto/ROOT/be   quiet`, as the boot
/// environment printed it: its event log (records as ZFSBootMenu writes
/// them, separators printed as spaces) and its PCRs (`pcr9` in place of
/// PCR 9's value, if given), and the payload's digests.
fn measured_boot(pcr9: Option<&str>) -> (Boot, Payload) {
    let line = "root=zfs:tauto/ROOT/be   quiet";
    let zeros = vec![0; 32];
    let events: [(u64, &str, &str, Vec<u8>); 6] = [
        (8, "start", "", zeros.clone()),
        (9, "start", "", zeros.clone()),
        (9, "kernel", "/boot/vmlinuz", sha256(&[b"KERNEL"])),
        (9, "initramfs", "/boot/initramfs.img", sha256(&[b"INITRD"])),
        (8, "command-line", line, sha256(&[line.as_bytes()])),
        (9, "separator", "zbm-unsealed", sha256(&[b"zbm-unsealed"])),
    ];
    let mut be = String::from("BE-INIT-START\nBE-MEASURE-LOG-BEGIN\n");
    for (recnum, (pcr, event, value, digest)) in events.iter().enumerate() {
        let record = serde_json::json!({
            "recnum": recnum,
            "pcr": pcr,
            "digests": [
                {"hashAlg": "sha1", "digest": "00"},
                {"hashAlg": "sha256", "digest": hex(digest)},
            ],
            "content_type": "zfsbootmenu",
            "content": {"event": event, "value": value},
        });
        writeln!(be, "BE-ML|  {record}").unwrap();
    }
    be.push_str("BE-MEASURE-LOG-END\n");
    // The values the TPM computes: PCR 8 from zeros, then the command
    // line; PCR 9 from zeros, then the kernel, the initramfs and the
    // separator.
    let pcr8 = sha256(&[&zeros, &events[4].3]);
    let pcr9_value = [&events[2].3, &events[3].3, &events[5].3]
        .iter()
        .fold(zeros, |value, digest| sha256(&[&value, digest]));
    let pcrs = vec![
        format!("8:sha256={}", hex(&pcr8)),
        format!(
            "9:sha256={}",
            pcr9.map_or_else(|| hex(&pcr9_value), str::to_owned)
        ),
    ];
    let mut be_lines = std::collections::HashMap::new();
    be_lines.insert("BE-PCR".to_owned(), pcrs);
    be_lines.insert(
        "BE-CMDLINE".to_owned(),
        vec!["root=zfs:tauto/ROOT/be quiet".to_owned()],
    );
    let boot = Boot {
        be,
        be_lines,
        ..Boot::default()
    };
    let payload = Payload {
        kernel: sha256(&[b"KERNEL"]),
        initramfs: sha256(&[b"INITRD"]),
    };
    (boot, payload)
}

#[test]
fn a_measured_boot_replays_from_its_log_and_moves_pcr_9_on() {
    let settings = settings("integrity-measured-checks");
    let test = Test::new(&settings, "integrity-measured").unwrap();
    let (boot, payload) = measured_boot(None);

    check_measured(&test, &boot, &payload);
    // PCR 9 as auto0's token is bound to, another payload, another command
    // line and no log: every check fails.
    let (mut stuck, _) = measured_boot(Some(&"00".repeat(32)));
    let other = Payload {
        kernel: sha256(&[b"OTHER"]),
        initramfs: payload.initramfs.clone(),
    };
    stuck
        .be_lines
        .insert("BE-CMDLINE".to_owned(), vec!["quiet".to_owned()]);
    check_measured(&test, &stuck, &other);
    check_measured(&test, &Boot::default(), &payload);

    let results: Vec<bool> = checks(&test)
        .iter()
        .map(|line| line.starts_with("ok"))
        .collect();
    assert_eq!(&results[..6], [true; 6]);
    assert_eq!(&results[6..12], [true, true, false, false, false, false]);
    assert_eq!(&results[12..], [false; 6]);
    cleanup(&settings);
}

#[test]
fn a_pcr_the_boot_environment_did_not_read_is_none() {
    let (boot, _) = measured_boot(Some("zz"));
    assert_eq!(be_pcr(&boot, 9), None);
    assert_eq!(be_pcr(&boot, 7), None);
    assert_eq!(be_pcr(&Boot::default(), 8), None);
    assert_eq!(be_pcr(&boot, 8).map(|value| value.len()), Some(32));
    // A record that does not read ends the log.
    let log = format!("{}BE-ML| {{\"pcr\":9}}\nBE-ML| {}\n", boot.be, "{}");
    assert_eq!(printed_log(&log).len(), 6);
}

#[test]
fn the_payload_s_digests_are_its_files() {
    let settings = settings("integrity-measured-payload");
    let boot = settings.work.join("payload/boot");
    std::fs::create_dir_all(&boot).unwrap();
    std::fs::write(boot.join("vmlinuz-7.3"), b"KERNEL").unwrap();

    // Without its initramfs, the payload cannot be read.
    assert!(Payload::read(&settings, "7.3").is_err());
    std::fs::write(boot.join("initramfs-7.3.img"), b"INITRD").unwrap();
    let payload = Payload::read(&settings, "7.3").unwrap();
    assert_eq!(payload.kernel, sha256(&[b"KERNEL"]));
    assert_eq!(payload.initramfs, sha256(&[b"INITRD"]));
    cleanup(&settings);
}

/// A boot whose environment printed a log of `records`: (PCR, event,
/// value, SHA-256 digest).
fn printed(records: &[(u64, &str, &str, Vec<u8>)], prompts: &[&str]) -> Boot {
    let mut be = String::from("BE-INIT-START\n");
    for (recnum, (pcr, event, value, digest)) in records.iter().enumerate() {
        let record = serde_json::json!({
            "recnum": recnum,
            "pcr": pcr,
            "digests": [{"hashAlg": "sha256", "digest": hex(digest)}],
            "content_type": "zfsbootmenu",
            "content": {"event": event, "value": value},
        });
        writeln!(be, "BE-ML|  {record}").unwrap();
    }
    Boot {
        be,
        prompts: prompts.iter().map(|&prompt| prompt.to_owned()).collect(),
        ..Boot::default()
    }
}

#[test]
fn a_hook_root_unseals_its_token_and_another_asks_for_the_passphrase() {
    let settings = settings("integrity-hookroot-checks");
    let test = Test::new(&settings, "integrity-hookroot").unwrap();
    let hook = "#!/bin/sh\necho ZBMTEST-HOOKROOT-ENROLLED\n";
    let name = "early-setup.d/10-zbmtest";
    let digest = sha256(&[name.as_bytes(), &[0], hook.as_bytes()]);
    let records = [
        (8, "start", "", vec![0; 32]),
        (9, "start", "", vec![0; 32]),
        (9, "hook", name, digest),
        (9, "kernel", "/boot/vmlinuz", sha256(&[b"KERNEL"])),
        (9, "separator", "zbm-unsealed", sha256(&[b"zbm-unsealed"])),
    ];
    assert_eq!(
        hookroot::after_hook(hook).to_vec(),
        replay(&printed_log(&printed(&records[..3], &[]).be), 9)
    );

    hookroot::check_enrolled(&test, &printed(&records, &[]), "ZBMTEST-HOOKROOT-ENROLLED");
    hookroot::check_other(
        &test,
        &printed(&records, &["luks"]),
        "ZBMTEST-HOOKROOT-OTHER",
    );
    // Asked, without the hook, and another value: every check fails.
    let other = [(9, "start", "", vec![0; 32]), (9, "hook", "x", vec![1; 32])];
    hookroot::check_enrolled(&test, &printed(&other, &["luks"]), "");
    hookroot::check_other(&test, &printed(&other, &[]), "");

    let results: Vec<bool> = checks(&test)
        .iter()
        .map(|line| line.starts_with("ok"))
        .collect();
    assert_eq!(
        results,
        [
            true, true, true, true, true, true, false, false, false, false, false, false
        ]
    );
    cleanup(&settings);
}
