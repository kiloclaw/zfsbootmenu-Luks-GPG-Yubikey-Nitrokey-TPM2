use pretty_assertions::assert_eq;

use super::*;

/// An event of `measured` with the digests of `bytes`.
fn event(measured: Measured, bytes: &[u8]) -> Event {
    Event {
        measured,
        digests: Digests::of_bytes(bytes),
    }
}

/// The boot's events: the kernel, the initramfs and the separator in PCR 9,
/// the command line in PCR 8.
fn boot() -> Vec<Event> {
    vec![
        event(Measured::Kernel("/boot/vmlinuz".into()), b"KERNEL"),
        event(Measured::Initramfs("/boot/initramfs.img".into()), b"INITRD"),
        Event::command_line("quiet"),
        Event::unsealed(),
    ]
}

#[test]
fn each_bank_gets_its_own_digest() {
    let digests = Digests::of_bytes(b"abc");
    assert_eq!(
        hex(digests.get(Bank::Sha1).unwrap()),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    assert!(hex(digests.get(Bank::Sha384).unwrap()).starts_with("cb00753f45a35e8b"));
    for bank in Bank::ALL {
        assert_eq!(digests.get(bank).unwrap().len(), bank.length());
        assert_eq!(Bank::named(bank.name()), Some(bank));
    }
    assert_eq!(Bank::named("md5"), None);
}

#[test]
fn the_pcrs_replay_as_the_tpm_extends_them() {
    let events = boot();
    // As Python's hashlib computes them.
    assert_eq!(
        hex(&replay(&events, COMMAND_LINE_PCR, Bank::Sha256)),
        "49a10009cbe7dd647d343d2e5862901e2a9b41ea8dd841262126888cfc397051"
    );
    assert_eq!(
        hex(&replay(&events, FILES_PCR, Bank::Sha256)),
        "091e7687d69b539331ffbdeea8c0e2d47dcedc4513adbc1b87b5214ceaca6b8f"
    );
    assert_eq!(
        hex(&replay(&events, FILES_PCR, Bank::Sha1)),
        "976362e3fae2da8e4f029e3b68a3f7c8832dd1f5"
    );
    // A PCR nothing went into stays zero.
    assert_eq!(replay(&events, 7, Bank::Sha256), vec![0; 32]);
}

#[test]
fn the_log_is_a_sequence_of_cel_json_records_that_reads_back() {
    let events = boot();
    let banks = [Bank::Sha1, Bank::Sha256];
    let log: String = events
        .iter()
        .zip(0..)
        .map(|(event, recnum)| record(recnum, event, &banks))
        .collect();
    assert!(log.starts_with('\u{1e}') && log.ends_with('\n'));
    let first = log.split('\u{1e}').nth(1).unwrap();
    let value: serde_json::Value = serde_json::from_str(first).unwrap();
    assert_eq!(value["recnum"], 0);
    assert_eq!(value["pcr"], 9);
    assert_eq!(value["content"]["event"], "kernel");
    assert_eq!(value["digests"].as_array().unwrap().len(), 2);

    let read = parse_log(&log).unwrap();
    assert_eq!(read.len(), 4);
    assert_eq!(read[2].measured, Measured::CommandLine("quiet".into()));
    // Only the banks logged, which replay the same.
    assert_eq!(read[0].digests.get(Bank::Sha384), None);
    assert_eq!(
        replay(&read, FILES_PCR, Bank::Sha256),
        replay(&events, FILES_PCR, Bank::Sha256)
    );
}

#[test]
fn a_record_of_another_kind_or_a_wrong_digest_is_not_read() {
    let good = record(0, &Event::unsealed(), &[Bank::Sha256]);
    let wrong_pcr = good.replace("\"pcr\":9", "\"pcr\":8");
    let short = good.replace("\"digest\":\"", "\"digest\":\"ab");
    let other = good.replace("zfsbootmenu", "systemd");
    for (index, bad) in ["{", &wrong_pcr, &short, &other].iter().enumerate() {
        let log = format!("{good}\u{1e}{}", bad.trim_start_matches('\u{1e}'));
        assert_eq!(
            parse_log(&log).map(|events| events.len()),
            Err(1),
            "{index}"
        );
    }
    assert_eq!(unhex("abc"), None);
    assert_eq!(unhex("zz"), None);
}

#[test]
fn the_log_goes_in_a_newc_archive_with_its_directories() {
    let archive = cpio(LOG_PATH, b"{}\n");
    assert_eq!(archive.len() % 512, 0);
    let text = String::from_utf8_lossy(&archive);
    assert_eq!(text.matches("070701").count(), 4);
    for name in [".extra\0", ".extra/zfsbootmenu\0", LOG_PATH, "TRAILER!!!\0"] {
        assert!(text.contains(name), "{name}");
    }
    // The file's mode and size: 0100644, 3 bytes.
    let file = text.find(&format!("{LOG_PATH}\0")).unwrap();
    let header = &text[text[..file].rfind("070701").unwrap()..file];
    assert_eq!(&header[14..22], "000081a4");
    assert_eq!(&header[54..62], "00000003");
    assert!(text[file..].starts_with(&format!("{LOG_PATH}\0")));
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "the archive is read by the real cpio, from a file of its own"
)]
fn cpio_itself_reads_the_archive() {
    let Ok(listed) = std::process::Command::new("cpio")
        .args(["-it", "--quiet"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child
                .stdin
                .take()
                .map(|mut stdin| stdin.write_all(&cpio(LOG_PATH, b"{}\n")));
            child.wait_with_output()
        })
    else {
        return;
    };
    assert!(listed.status.success());
    assert_eq!(
        String::from_utf8_lossy(&listed.stdout),
        format!(".extra\n.extra/zfsbootmenu\n{LOG_PATH}\n")
    );
}

#[test]
fn a_start_sets_the_value_the_replay_continues_from() {
    let start = vec![0x42; 32];
    let digests = Digests::new(vec![(Bank::Sha256, start.clone())]);
    let mut events = vec![Event::start(9, digests)];
    assert_eq!(replay(&events, 9, Bank::Sha256), start);
    events.push(Event::unsealed());
    let separator = Digests::of_bytes(UNSEALED.as_bytes());
    let expected = Bank::Sha256.digest(&[&start, separator.get(Bank::Sha256).unwrap()]);
    assert_eq!(replay(&events, 9, Bank::Sha256), expected);
    // PCR 8 is not started by PCR 9's start; a bank the start leaves out
    // starts from zeros.
    assert_eq!(replay(&events, 8, Bank::Sha256), vec![0; 32]);
    assert_ne!(replay(&events, 9, Bank::Sha1), vec![0; 20]);

    // In the log, a start names no value and keeps its PCR.
    let text: String = (0..)
        .zip(&events)
        .map(|(recnum, event)| record(recnum, event, &[Bank::Sha256]))
        .collect();
    assert!(
        text.contains(r#""content":{"event":"start","value":""}"#),
        "{text}"
    );
    let read = parse_log(&text).unwrap();
    assert_eq!(read[0].measured, Measured::Start(9));
    assert_eq!(read[0].measured.kind(), "start");
    assert_eq!(read[0].measured.value(), "");
    assert_eq!(replay(&read, 9, Bank::Sha256), expected);
    // A start of another PCR, or with a value, is none of ours.
    let other = text.replacen(r#""pcr":9"#, r#""pcr":7"#, 1);
    assert_eq!(parse_log(&other), Err(0));
    let valued = text.replacen(r#""value":"""#, r#""value":"x""#, 1);
    assert_eq!(parse_log(&valued), Err(0));
}

#[test]
fn hooks_go_into_pcr_9_before_the_boot_and_set_the_value_tokens_unseal_at() {
    let hook = Event::hook("early-setup.d/10-hello", b"echo hello\n");
    assert_eq!(hook.pcr(), FILES_PCR);
    assert_eq!(hook.measured.kind(), "hook");
    assert!(!hook.is_of_the_boot());
    // Its name is measured with it: moved, it is another hook.
    let moved = Event::hook("teardown.d/10-hello", b"echo hello\n");
    assert_ne!(hook.digests, moved.digests);
    let expected = Bank::Sha256.digest(&[b"early-setup.d/10-hello", &[0], b"echo hello\n"]);
    assert_eq!(hook.digests.get(Bank::Sha256), Some(expected.as_slice()));

    let start = Event::start(9, Digests::new(vec![(Bank::Sha256, vec![7; 32])]));
    let mut events = vec![start, hook];
    let at = unsealed_at(&events, Bank::Sha256);
    assert_eq!(at, replay(&events, 9, Bank::Sha256));
    events.extend(boot());
    // The boot's files and separator come after the unseal.
    assert_eq!(unsealed_at(&events, Bank::Sha256), at);
    assert_ne!(replay(&events, 9, Bank::Sha256), at);
    assert!(boot().iter().all(Event::is_of_the_boot));

    let text: String = (0..)
        .zip(&events)
        .map(|(recnum, event)| record(recnum, event, &[Bank::Sha256]))
        .collect();
    let read = parse_log(&text).unwrap();
    assert_eq!(
        read[1].measured,
        Measured::Hook("early-setup.d/10-hello".into())
    );
    assert_eq!(unsealed_at(&read, Bank::Sha256), at);
    // Without a start or hooks, tokens unseal at zeros.
    assert_eq!(unsealed_at(&boot(), Bank::Sha256), vec![0; 32]);
}
