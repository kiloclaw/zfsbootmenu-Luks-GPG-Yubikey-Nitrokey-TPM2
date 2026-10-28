//! Tests of running programs on the system.

use super::*;

#[test]
fn programs_can_start_with_default_signal_actions() {
    let interrupt = 1_u64 << (Signal::Interrupt.number() - 1);
    let mask = |command: Command| {
        let output = SystemRunner
            .run(&command.args(["SigIgn", "/proc/self/status"]))
            .unwrap();
        let text = output.stdout_text();
        u64::from_str_radix(text.trim().trim_start_matches("SigIgn:").trim(), 16).unwrap()
    };
    // Children of this test may run meanwhile; ignoring SIGINT does them no harm.
    zbm_sys::ignore_signal(Signal::Interrupt).unwrap();
    let inherited = mask(Command::new("grep"));
    let restored = mask(Command::new("grep").default_signal(Signal::Interrupt));
    let merged = mask(
        Command::new("grep")
            .default_signal(Signal::Interrupt)
            .merge_stderr(),
    );
    zbm_sys::restore_default(Signal::Interrupt).unwrap();
    assert_ne!(inherited & interrupt, 0);
    assert_eq!(restored & interrupt, 0);
    assert_eq!(merged & interrupt, 0);
    assert_eq!(
        Command::new("bash")
            .default_signal(Signal::Interrupt)
            .get_default_signals(),
        [Signal::Interrupt]
    );
}

#[test]
fn captures_output_and_status() {
    let output = SystemRunner
        .run(&Command::shell("echo out; echo err >&2; exit 3").stderr(Stream::Capture))
        .unwrap();
    assert_eq!(output.stdout, b"out\n");
    assert_eq!(output.stderr, b"err\n");
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(output.status.perl_code(), 3);
    assert_eq!(output.status.shell_code(), 3);
    assert!(!output.status.is_success());
}

#[test]
fn merged_streams_keep_their_order() {
    let output = SystemRunner
        .run(
            &Command::new("sh")
                .args(["-c", "echo one; echo two >&2; echo three"])
                .merge_stderr(),
        )
        .unwrap();
    assert_eq!(output.stdout_text(), "one\ntwo\nthree\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn merged_streams_need_no_shell() {
    // No `sh` can be found on this `PATH`: the program gets the pipe itself.
    let output = SystemRunner
        .run(
            &Command::new("/bin/ls")
                .args(["/proc/self/fd/1", "/zbm-no-such-file"])
                .env("PATH", "/zbm-nowhere")
                .merge_stderr(),
        )
        .unwrap();
    let text = output.stdout_text();
    assert!(text.contains("/proc/self/fd/1"), "{text}");
    assert!(text.contains("zbm-no-such-file"), "{text}");
    assert_eq!(output.status.code(), Some(2));
    // Into nothing, and where this process's standard output goes.
    let quiet = Command::new("/bin/ls")
        .arg("/zbm-no-such-file")
        .stdout(Stream::Null)
        .merge_stderr();
    let output = SystemRunner.run(&quiet).unwrap();
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    let shown = Command::new("/bin/true")
        .stdout(Stream::Inherit)
        .merge_stderr();
    assert!(SystemRunner.run(&shown).unwrap().status.is_success());
}

#[test]
fn merged_arguments_are_passed_unchanged() {
    let output = SystemRunner
        .run(
            &Command::new("printf")
                .args(["[%s]", "a b", "$HOME", "\"q\""])
                .merge_stderr(),
        )
        .unwrap();
    assert_eq!(output.stdout_text(), "[a b][$HOME][\"q\"]");
}

#[test]
fn feeds_standard_input() {
    let big = "x".repeat(1 << 20);
    let output = SystemRunner
        .run(
            &Command::new("wc")
                .arg("-c")
                .stdin(Input::Bytes(big.into_bytes())),
        )
        .unwrap();
    assert_eq!(output.stdout_text().trim(), (1 << 20).to_string());
}

#[test]
fn secrets_travel_through_pipes() {
    let output = SystemRunner
        .run(
            &Command::new("sh")
                .args(["-c", "tr a-z A-Z; echo done >&2"])
                .stdin(Input::secret(Secret::copy_from(b"hunter2")))
                .stdout(Stream::CaptureSecret)
                .stderr(Stream::Capture),
        )
        .unwrap();
    assert!(output.status.is_success());
    assert_eq!(output.secret.as_ref().unwrap().expose(), b"HUNTER2");
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"done\n");

    let missing = SystemRunner
        .run(
            &Command::new("zbm-no-such-program")
                .stdin(Input::secret(Secret::copy_from(b"x")))
                .stdout(Stream::CaptureSecret)
                .merge_stderr(),
        )
        .unwrap();
    assert_eq!(missing.status.code(), Some(127));
    assert!(missing.secret.is_some());
}

#[test]
#[allow(
    clippy::large_stack_arrays,
    reason = "vec! builds the input on the heap"
)]
fn input_the_command_does_not_read_is_not_an_error() {
    // More than a pipe holds, to a program that exits at once.
    let big = vec![b'x'; 1 << 20];
    let output = SystemRunner
        .run(&Command::new("true").stdin(Input::Bytes(big)))
        .unwrap();
    assert!(output.status.is_success());
    let output = SystemRunner
        .run(&Command::new("false").stdin(Input::secret(Secret::copy_from(&[0; 1 << 20]))))
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn a_secret_input_does_not_show_in_debug_output() {
    let command = Command::new("cryptsetup")
        .arg("open")
        .stdin(Input::secret(Secret::copy_from(b"hunter2")));
    assert!(!format!("{command:?}").contains("hunter2"));
    let output = Output::with_secret(Status::exited(0), Secret::copy_from(b"key"));
    assert!(!format!("{output:?}").contains("key\""));
    assert_eq!(output.secret.unwrap().expose(), b"key");
}

#[test]
fn missing_program_reports_127() {
    let output = SystemRunner
        .run(&Command::new("zbm-no-such-program").stderr(Stream::Capture))
        .unwrap();
    assert_eq!(output.status.code(), Some(127));
    assert_eq!(
        output.stderr_text(),
        "zbm-no-such-program: command not found\n"
    );

    let merged = SystemRunner
        .run(&Command::new("zbm-no-such-program").merge_stderr())
        .unwrap();
    assert_eq!(merged.status.code(), Some(127));
}

#[test]
fn a_missing_program_is_reported_where_standard_error_goes() {
    let missing = |stderr: Stream| {
        SystemRunner
            .run(&Command::new("zbm-no-such-program").stderr(stderr))
            .unwrap()
    };

    let secret = missing(Stream::CaptureSecret);
    let discarded = missing(Stream::Null);
    let shown = missing(Stream::Inherit);

    let message = secret.secret.unwrap();
    assert_eq!(
        message.expose(),
        b"zbm-no-such-program: command not found\n"
    );
    assert_eq!(discarded.stderr_text(), "");
    assert_eq!(shown.status.code(), Some(127));
}

#[test]
fn signal_statuses_follow_perl_and_shell() {
    let output = SystemRunner.run(&Command::shell("kill -9 $$")).unwrap();
    assert_eq!(output.status.signal(), Some(9));
    assert_eq!(output.status.code(), None);
    assert_eq!(output.status.perl_code(), 0);
    assert_eq!(output.status.shell_code(), 137);
}

#[test]
fn environment_changes_apply() {
    let output = SystemRunner
        .run(
            &Command::shell("echo \"${ZBM_A:-unset} ${HOME:-unset}\"")
                .env("ZBM_A", "set")
                .env_remove("HOME"),
        )
        .unwrap();
    assert_eq!(output.stdout_text(), "set unset\n");
}

#[test]
fn display_joins_program_and_arguments() {
    let command = Command::new("zfs").args(["get", "-H", "name"]);
    assert_eq!(command.display(), "zfs get -H name");
    assert_eq!(command.program(), "zfs");
    assert_eq!(command.get_args().len(), 3);
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "the test measures that the program was stopped in real time"
)]
fn a_program_past_its_time_is_killed() {
    let started = std::time::Instant::now();
    let output = SystemRunner
        .run(
            &Command::new("sleep")
                .arg("30")
                .timeout(Duration::from_millis(100)),
        )
        .unwrap();

    assert_eq!(output.status.signal(), Some(Signal::Kill.number()));
    assert!(started.elapsed() < Duration::from_secs(10));

    let quick = SystemRunner
        .run(&Command::shell("echo done").timeout(Duration::from_secs(30)))
        .unwrap();
    assert_eq!(quick.stdout_text(), "done\n");
    assert!(quick.status.is_success());
    let limited = Command::new("x")
        .timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(9));
    assert_eq!(limited.get_timeout(), Some(Duration::from_secs(5)));
    assert_eq!(Command::new("x").get_timeout(), None);
}

#[test]
fn output_past_its_limit_is_an_error_and_ends_the_program() {
    // `yes` writes for ever: past the limit its pipe is closed, and it ends.
    let flood = Command::new("yes").output_limit(1000);
    let error = SystemRunner.run(&flood).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("1000 bytes"), "{error}");
    let secret = Command::new("yes")
        .stdout(Stream::CaptureSecret)
        .output_limit(10);
    assert!(SystemRunner.run(&secret).is_err());
    let noisy = Command::new("sh")
        .args(["-c", "yes >&2"])
        .stderr(Stream::Capture)
        .output_limit(10);
    assert!(SystemRunner.run(&noisy).is_err());
    // Up to the limit, everything is taken.
    let exact = Command::new("printf").arg("0123456789").output_limit(10);
    assert_eq!(SystemRunner.run(&exact).unwrap().stdout, b"0123456789");
    assert_eq!(
        Command::new("x").get_output_limit(Stream::Capture),
        OUTPUT_LIMIT
    );
    assert_eq!(
        Command::new("x").get_output_limit(Stream::CaptureSecret),
        SECRET_LIMIT
    );
}
