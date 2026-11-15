use super::*;
use pretty_assertions::assert_eq;
use zbm_platform::{KernelLog, Output, Status};
use zbm_testkit::FakeRunner;
use zbm_testkit::fakes::FakeSystem;

#[test]
fn a_session_is_flushed_on_every_path() {
    let runner = FakeRunner::new();
    runner.respond_program("tpm2_unseal", 1, "");
    let tpm = Tpm2::new(&runner, "swtpm:port=2321");
    let work = Path::new("/w");
    {
        let session = tpm.policy_session(&work.join("s.ctx")).unwrap();
        session.auth_value().unwrap();
        assert!(session.unseal(&work.join("o.ctx"), None).is_none());
    }
    tpm.flush_transient();

    assert_eq!(
        runner.lines(),
        [
            "tpm2_startauthsession --policy-session -c 0x81000001 -S /w/s.ctx",
            "tpm2_sessionconfig /w/s.ctx --enable-encrypt",
            "tpm2_policyauthvalue -S /w/s.ctx",
            "tpm2_unseal -c /w/o.ctx -p session:/w/s.ctx",
            "tpm2_flushcontext /w/s.ctx",
            "tpm2_flushcontext -t",
        ]
    );
    assert!(runner.calls().iter().all(|command| {
        command.get_env().iter().any(|(key, value)| {
            key == "TPM2TOOLS_TCTI" && value.as_deref() == Some("swtpm:port=2321".as_ref())
        })
    }));
}

#[test]
fn the_authorization_value_goes_to_standard_input() {
    let runner = FakeRunner::new();
    runner.on(|command| {
        (command.program() == "tpm2_unseal").then(|| {
            let given = matches!(
                command.get_stdin(),
                Input::Secret(secret) if secret.expose() == b"hex:abcd"
            );
            Ok(if given {
                Output::with_secret(Status::exited(0), Secret::copy_from(b"key"))
            } else {
                Output::new(Status::exited(1), "")
            })
        })
    });
    let tpm = Tpm2::new(&runner, "device:/dev/tpmrm0");
    let session = tpm.policy_session(Path::new("/s")).unwrap();

    let key = session.unseal(Path::new("/o"), Some(&Secret::copy_from(b"abcd")));
    assert_eq!(key.unwrap().expose(), b"key");
    assert!(runner.lines()[2].ends_with("-p session:/s+file:-"));
    assert!(
        session
            .unseal(Path::new("/o"), Some(&Secret::copy_from(b"0000")))
            .is_none()
    );
}

#[test]
fn a_session_that_cannot_encrypt_is_flushed_and_refused() {
    let runner = FakeRunner::new();
    runner.respond_program("tpm2_sessionconfig", 1, "");
    let tpm = Tpm2::new(&runner, "x");

    let refused = tpm.policy_session(Path::new("/s")).err();

    assert_eq!(refused, Some(ToolFailed("tpm2_sessionconfig")));
    assert_eq!(
        runner.lines().last().map(String::as_str),
        Some("tpm2_flushcontext /s")
    );
}

#[test]
fn lockout_is_read_from_the_variable_properties() {
    let runner = FakeRunner::new();
    runner.respond(
        "tpm2_getcap properties-variable",
        0,
        "TPM2_PT_PERMANENT:\n  inLockout:                 1\n",
    );
    assert!(Tpm2::new(&runner, "x").in_lockout());
    let clear = FakeRunner::new();
    clear.respond("tpm2_getcap properties-variable", 0, "  inLockout: 0\n");
    assert!(!Tpm2::new(&clear, "x").in_lockout());
    let failing = FakeRunner::new();
    failing.on(|_| Some(Err(std::io::Error::other("no tool"))));
    let tpm = Tpm2::new(&failing, "x");
    assert!(!tpm.in_lockout());
    assert_eq!(
        tpm.policy_session(Path::new("/s")).err(),
        Some(ToolFailed("tpm2_startauthsession"))
    );
    assert_eq!(
        tpm.load(Path::new("/p"), Path::new("/r"), Path::new("/c")),
        Err(ToolFailed("tpm2_load"))
    );
}

#[test]
fn the_active_banks_are_those_with_pcrs_selected() {
    let runner = FakeRunner::new();
    runner.respond(
        "tpm2_getcap pcrs",
        0,
        "selected-pcrs:\n  - sha1: [ 0, 1, 2 ]\n  - sha256: [ 0, 1, 2 ]\n  - sha384: [ ]\n",
    );
    let banks = Tpm2::new(&runner, "x").active_banks();
    assert_eq!(banks, Some(vec![Bank::Sha1, Bank::Sha256]));
    let refusing = FakeRunner::new();
    refusing.respond("tpm2_getcap pcrs", 1, "");
    assert_eq!(Tpm2::new(&refusing, "x").active_banks(), None);
    let failing = FakeRunner::new();
    failing.on(|_| Some(Err(std::io::Error::other("no tool"))));
    assert_eq!(Tpm2::new(&failing, "x").active_banks(), None);
}

#[test]
fn the_values_of_pcrs_are_read_in_each_bank() {
    let runner = FakeRunner::new();
    let sha1 = "0x00000000000000000000000000000000000000AB";
    let sha256 = format!("0x{}", "11".repeat(32));
    runner.respond(
        "tpm2_pcrread sha1:8,9+sha256:8,9",
        0,
        &format!(
            "  sha1:\n    8 : {sha1}\n    9 : {sha1}\n  sha256:\n    8 : {sha256}\n    \
             9 : {sha256}\n"
        ),
    );
    let tpm = Tpm2::new(&runner, "x");

    let values = tpm.read_pcrs(&[8, 9], &[Bank::Sha1, Bank::Sha256]).unwrap();

    assert_eq!(values.len(), 2);
    assert_eq!(values[1].0, 9);
    let mut expected = vec![0; 20];
    expected[19] = 0xab;
    assert_eq!(values[1].1.get(Bank::Sha1), Some(expected.as_slice()));
    assert_eq!(values[0].1.get(Bank::Sha256), Some([0x11; 32].as_slice()));
    // A bank it left out, or a TPM that refuses, gives nothing.
    assert_eq!(tpm.read_pcrs(&[8], &[Bank::Sha384]), None);
    let refusing = FakeRunner::new();
    refusing.respond_program("tpm2_pcrread", 1, "");
    assert_eq!(
        Tpm2::new(&refusing, "x").read_pcrs(&[8], &[Bank::Sha1]),
        None
    );
    let failing = FakeRunner::new();
    failing.on(|_| Some(Err(std::io::Error::other("no tool"))));
    assert_eq!(
        Tpm2::new(&failing, "x").read_pcrs(&[8], &[Bank::Sha1]),
        None
    );
    // A value of the wrong length, or no number, is no value.
    let values = pcr_values("  sha1:\n    8 : 0x00\n    x : 0x00\n  md5:\n    8 : 0x00\n");
    assert!(values.is_empty());
}

#[test]
fn a_pcr_is_extended_in_each_bank_given() {
    let runner = FakeRunner::new();
    let tpm = Tpm2::new(&runner, "x");
    let digests = Digests::of_bytes(b"quiet");

    tpm.extend(8, &digests, &[Bank::Sha1, Bank::Sha256])
        .unwrap();

    let line = &runner.lines()[0];
    let sha1 = measure::hex(digests.get(Bank::Sha1).unwrap());
    let sha256 = measure::hex(digests.get(Bank::Sha256).unwrap());
    assert_eq!(
        line,
        &format!("tpm2_pcrextend 8:sha1={sha1},sha256={sha256}")
    );
    runner.respond_program("tpm2_pcrextend", 1, "");
    assert_eq!(
        tpm.extend(9, &digests, &[Bank::Sha256]),
        Err(ToolFailed("tpm2_pcrextend"))
    );
}

#[test]
fn every_command_ends_after_its_time_out() {
    let runner = FakeRunner::new();
    let _ = Tpm2::new(&runner, "device:/dev/tpmrm0").in_lockout();
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].get_timeout(), Some(TIMEOUT));
    // What a tool prints on standard error is read as a secret is.
    assert_eq!(calls[0].get_stderr(), Stream::CaptureSecret);
}

/// The records of the journal that `body` wrote at debug, on `fake`'s files.
fn journal(fake: &FakeSystem, body: impl FnOnce()) -> Vec<serde_json::Value> {
    fake.files.dir("/zfsbootmenu");
    {
        let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
        body();
    }
    let text = fake
        .files
        .text("/zfsbootmenu/journal.jsonl")
        .unwrap_or_default();
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_failed_tpm2_tool_s_error_reaches_the_debug_log() {
    let fake = FakeSystem::new();
    fake.runner.on(|command| {
        (command.program() == "tpm2_load").then(|| {
            let mut output = Output::new(Status::exited(1), "");
            output.stderr = b"WARNING:esys:src/tss2-esys/api/Esys_Load.c:324:Esys_Load_Finish() \
                Received TPM Error\nERROR:esys:src/tss2-esys/api/Esys_Load.c:112:Esys_Load() \
                Esys Finish ErrorCode (0x000001df)\nERROR: Unable to run tpm2_load\n"
                .to_vec();
            Ok(output)
        })
    });
    let tpm = Tpm2::new(&fake.runner, "x");

    let records = journal(&fake, || {
        let failed = tpm.load(Path::new("/p"), Path::new("/r"), Path::new("/c"));
        assert_eq!(failed, Err(ToolFailed("tpm2_load")));
        // A tool that succeeds leaves nothing.
        assert!(tpm.policy_session(Path::new("/s")).is_ok());
    });

    assert_eq!(records.len(), 1, "{records:?}");
    let record = &records[0];
    assert_eq!(record["level"], "DEBUG");
    assert_eq!(record["fields"]["program"], "tpm2_load");
    assert_eq!(record["fields"]["status"], 1);
    let message = record["message"].as_str().unwrap();
    assert!(
        message.starts_with("tpm2_load failed (1): WARNING:esys:"),
        "{message}"
    );
    assert!(
        message.ends_with("ErrorCode (0x000001df) | ERROR: Unable to run tpm2_load"),
        "{message}"
    );
}

#[test]
fn a_tool_that_does_not_start_or_says_nothing_is_logged_too() {
    let fake = FakeSystem::new();
    fake.runner.on(|command| {
        Some(if command.program() == "tpm2_pcrread" {
            Err(std::io::Error::other("no such file"))
        } else {
            Ok(Output::new(Status::exited(3), ""))
        })
    });
    let tpm = Tpm2::new(&fake.runner, "x");

    let records = journal(&fake, || {
        assert_eq!(tpm.read_pcrs(&[8], &[Bank::Sha256]), None);
        assert_eq!(tpm.active_banks(), None);
    });

    let messages: Vec<&str> = records
        .iter()
        .map(|record| record["message"].as_str().unwrap())
        .collect();
    assert_eq!(
        messages,
        [
            "tpm2_pcrread did not start: no such file",
            "tpm2_getcap failed (3)"
        ]
    );
}

#[test]
fn the_flushes_and_the_lockout_check_are_logged_when_they_fail() {
    let fake = FakeSystem::new();
    fake.runner.on(|command| {
        let mut output = Output::new(Status::exited(1), "  inLockout: 1\n");
        output.stderr =
            format!("ERROR: Unable to run {}\n", command.program().display()).into_bytes();
        Some(Ok(output))
    });
    let tpm = Tpm2::new(&fake.runner, "x");

    let records = journal(&fake, || {
        // A failed check is no lockout, whatever it printed.
        assert!(!tpm.in_lockout());
        drop(PolicySession {
            tpm: &tpm,
            context: PathBuf::from("/s"),
        });
        tpm.flush_transient();
    });

    let messages: Vec<&str> = records
        .iter()
        .map(|record| record["message"].as_str().unwrap())
        .collect();
    assert_eq!(
        messages,
        [
            "tpm2_getcap failed (1): ERROR: Unable to run tpm2_getcap",
            "tpm2_flushcontext failed (1): ERROR: Unable to run tpm2_flushcontext",
            "tpm2_flushcontext failed (1): ERROR: Unable to run tpm2_flushcontext",
        ]
    );
}

#[test]
fn the_authorization_value_never_reaches_the_log_even_if_a_tool_echoes_it() {
    let fake = FakeSystem::new();
    // A tool that prints what it was given on standard input, the digits in
    // capitals, and the bytes they stand for.
    fake.runner.on(|command| {
        (command.program() == "tpm2_unseal").then(|| {
            let Input::Secret(given) = command.get_stdin() else {
                panic!("the authorization value is not a secret");
            };
            let mut said = b"read ".to_vec();
            said.extend_from_slice(given.expose());
            said.extend_from_slice(b"\nauth ");
            said.extend_from_slice(&given.expose()[4..].to_ascii_uppercase());
            said.extend_from_slice(b" raw ");
            said.extend_from_slice(&[0xab, 0xcd, 0x01, 0x23]);
            said.extend_from_slice(b"\nERROR: authorization failed\n");
            let mut output = Output::new(Status::exited(1), "");
            output.stderr = said;
            Ok(output)
        })
    });
    let tpm = Tpm2::new(&fake.runner, "x");
    let auth = Secret::copy_from(b"abcd0123");

    let records = journal(&fake, || {
        let session = tpm.policy_session(Path::new("/s")).unwrap();
        assert!(session.unseal(Path::new("/o"), Some(&auth)).is_none());
    });

    let messages: Vec<&str> = records
        .iter()
        .map(|record| record["message"].as_str().unwrap())
        .collect();
    assert_eq!(
        messages,
        [
            "tpm2_unseal failed (1): read <auth> | auth <auth> raw <auth> | ERROR: authorization \
          failed"
        ]
    );
    for form in [&b"abcd0123"[..], b"ABCD0123", &[0xab, 0xcd, 0x01, 0x23]] {
        fake.assert_no_leak(form);
    }
}

#[test]
fn a_value_that_is_not_hexadecimal_is_scrubbed_as_it_is() {
    let auth = Secret::copy_from(b"xyz");
    assert_eq!(
        scrub(b"hex:xyz xyz XYZ x", Some(&auth)).expose(),
        b"<auth> <auth> <auth> x"
    );
    assert_eq!(scrub(b"hex:xyz", None).expose(), b"hex:xyz");
    assert_eq!(scrub(b"said", Some(&Secret::new())).expose(), b"said");
    assert_eq!(replace(b"aaa", b"aa", b"b").expose(), b"ba");
    assert_eq!(replace(b"aaa", b"", b"b").expose(), b"aaa");
}
