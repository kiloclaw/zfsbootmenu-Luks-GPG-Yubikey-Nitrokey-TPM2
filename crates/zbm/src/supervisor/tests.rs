//! The supervisor: restarts, crash reports, programs that cannot start,
//! and the console that cannot.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "tests may panic; the rules are for process 1"
)]
use super::*;
use zbm_testkit::TempDir;

/// `waitpid(-1)` collects any child, so tests that start processes take
/// turns.
static PROCESSES: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn restarts_the_program_after_a_shell() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let calls = dir.join("calls");
    let startup = dir.write_executable(
        "startup",
        format!(
            "#!/bin/sh\necho \"startup $*\" >> {}\nexit 3\n",
            calls.display()
        ),
    );
    let shell = dir.write_executable(
        "shell",
        format!(
            "#!/bin/sh\ngrep SigIgn /proc/self/status >> {}\n",
            calls.display()
        ),
    );
    let log = dir.write("kmsg", "");
    let supervisor = Supervisor {
        startup: Program {
            path: startup,
            args: vec!["zfsbootmenu-init".to_owned()],
        },
        shell: Program {
            path: shell,
            args: Vec::new(),
        },
        retry: Duration::from_millis(10),
        redirect_console: false,
        fallback: None,
        log: log.clone(),
        base: dir.path().to_owned(),
        console: None,
    };
    let mut events = Vec::new();
    supervisor.run(Some(2), &mut |event| events.push(event));
    let _ = zbm_sys::restore_default(Signal::Interrupt);

    let text = std::fs::read_to_string(&calls).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "{text}");
    assert_eq!(lines.first(), Some(&"startup zfsbootmenu-init"));
    assert_eq!(lines.get(2), Some(&"startup zfsbootmenu-init --resume"));
    // The shell gets SIGINT back.
    let mask = lines
        .get(1)
        .and_then(|line| line.strip_prefix("SigIgn:"))
        .map(|mask| u64::from_str_radix(mask.trim(), 16).unwrap_or(u64::MAX));
    assert_eq!(mask.map(|mask| mask & 2), Some(0), "{text}");
    let ended: Vec<&Event> = events
        .iter()
        .filter(|event| matches!(event, Event::Ended(_)))
        .collect();
    assert_eq!(ended, [&Event::Ended(3 << 8), &Event::Ended(3 << 8)]);
    let kmsg = std::fs::read_to_string(&log).unwrap();
    assert!(kmsg.contains("<3>ZFSBootMenu: zfsbootmenu-init ended unexpectedly"));
}

#[test]
fn a_program_killed_by_a_signal_leaves_a_crash_report() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let startup = dir.write_executable("startup", "#!/bin/sh\nkill -SEGV $$\n");
    let log = dir.write("kmsg", "");
    let supervisor = Supervisor {
        startup: Program {
            path: startup,
            args: Vec::new(),
        },
        shell: Program {
            path: PathBuf::from("true"),
            args: Vec::new(),
        },
        retry: Duration::from_millis(10),
        redirect_console: false,
        fallback: None,
        log: log.clone(),
        base: dir.path().to_owned(),
        console: None,
    };

    let mut events = Vec::new();
    supervisor.run(Some(1), &mut |event| events.push(event));
    let _ = zbm_sys::restore_default(Signal::Interrupt);

    let Some(Event::Started(pid)) = events.first() else {
        panic!("{events:?}");
    };
    let name = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.unwrap().file_name().into_string().ok())
        .find(|name| name.starts_with("crash-"))
        .unwrap();
    assert!(name.ends_with(&format!("-{pid}.txt")), "{name}");
    let report = Report::parse(&std::fs::read_to_string(dir.join(&name)).unwrap()).unwrap();
    assert_eq!(
        report.reason,
        Reason::Signal {
            number: 11,
            core: false
        }
    );
    let kmsg = std::fs::read_to_string(&log).unwrap();
    assert!(kmsg.contains(&format!("crash report: {}", dir.join(&name).display())));
}

#[test]
fn a_program_that_lost_its_terminal_did_not_crash() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let startup = dir.write_executable("startup", "#!/bin/sh\nkill -HUP $$\n");
    let log = dir.write("kmsg", "");
    let supervisor = Supervisor {
        startup: Program {
            path: startup,
            args: Vec::new(),
        },
        shell: Program {
            path: PathBuf::from("true"),
            args: Vec::new(),
        },
        retry: Duration::from_millis(10),
        redirect_console: false,
        fallback: None,
        log: log.clone(),
        base: dir.path().to_owned(),
        console: None,
    };

    let mut events = Vec::new();
    supervisor.run(Some(1), &mut |event| events.push(event));
    let _ = zbm_sys::restore_default(Signal::Interrupt);

    assert_eq!(events.get(1), Some(&Event::Ended(1)));
    let kmsg = std::fs::read_to_string(&log).unwrap();
    assert!(!kmsg.contains("crash report"), "{kmsg}");
}

#[test]
fn a_crash_report_the_program_wrote_is_named() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let startup = dir.write_executable(
        "startup",
        format!(
            "#!/bin/sh\necho report > {}/crash-1-$$.txt\nexit 101\n",
            dir.path().display()
        ),
    );
    let log = dir.write("kmsg", "");
    let supervisor = Supervisor {
        startup: Program {
            path: startup,
            args: Vec::new(),
        },
        shell: Program {
            path: PathBuf::from("true"),
            args: Vec::new(),
        },
        retry: Duration::from_millis(10),
        redirect_console: false,
        fallback: None,
        log: log.clone(),
        base: dir.path().to_owned(),
        console: None,
    };

    supervisor.run(Some(1), &mut |_| {});
    let _ = zbm_sys::restore_default(Signal::Interrupt);

    let kmsg = std::fs::read_to_string(&log).unwrap();
    assert!(kmsg.contains("crash report: "), "{kmsg}");
    let reports = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.unwrap().file_name().into_string().ok())
        .filter(|name| name.starts_with("crash-"))
        .count();
    assert_eq!(reports, 1);
}

#[test]
fn without_a_console_the_programs_run_on_the_supervisor_s_terminal() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let log = dir.write("kmsg", "");
    let supervisor = Supervisor {
        startup: Program {
            path: PathBuf::from("true"),
            args: Vec::new(),
        },
        shell: Program {
            path: PathBuf::from("true"),
            args: Vec::new(),
        },
        retry: Duration::from_millis(10),
        redirect_console: false,
        fallback: None,
        log: log.clone(),
        base: dir.path().to_owned(),
        console: Some(Program {
            path: dir.join("missing"),
            args: Vec::new(),
        }),
    };

    let mut events = Vec::new();
    supervisor.run(Some(1), &mut |event| events.push(event));
    let _ = zbm_sys::restore_default(Signal::Interrupt);

    assert!(
        matches!(events.first(), Some(Event::Console(what)) if what.starts_with("unavailable")),
        "{events:?}"
    );
    assert!(matches!(events.get(1), Some(Event::Started(_))));
    assert_eq!(events.get(2), Some(&Event::Ended(0)));
    let kmsg = std::fs::read_to_string(&log).unwrap();
    assert!(kmsg.contains("using the kernel console"), "{kmsg}");
}

#[test]
fn programs_that_cannot_start_are_logged() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let log = dir.write("kmsg", "");
    let supervisor = Supervisor {
        startup: Program {
            path: dir.join("missing"),
            args: Vec::new(),
        },
        shell: Program {
            path: PathBuf::from("true"),
            args: Vec::new(),
        },
        retry: Duration::from_millis(10),
        redirect_console: false,
        fallback: None,
        log: log.clone(),
        base: dir.path().to_owned(),
        console: None,
    };
    let mut events = Vec::new();
    supervisor.run(Some(1), &mut |event| events.push(event));
    let _ = zbm_sys::restore_default(Signal::Interrupt);
    assert!(matches!(events.first(), Some(Event::Failed(_))));
    assert_eq!(events.last(), Some(&Event::Shell));
    let kmsg = std::fs::read_to_string(&log).unwrap();
    assert!(kmsg.contains("unable to start"));
    assert_eq!(
        Supervisor::new(PathBuf::from("/libexec/zbm")).startup.args,
        ["zfsbootmenu-init", "--console"]
    );
}

/// A supervisor whose programs are `startup` and `true`, logging in `dir`.
fn supervisor_in(dir: &TempDir, startup: PathBuf) -> Supervisor {
    Supervisor {
        startup: Program {
            path: startup,
            args: Vec::new(),
        },
        shell: Program {
            path: PathBuf::from("true"),
            args: Vec::new(),
        },
        retry: Duration::from_millis(10),
        redirect_console: false,
        fallback: None,
        log: dir.write("kmsg", ""),
        base: dir.path().to_owned(),
        console: None,
    }
}

#[test]
fn programs_run_on_the_console_s_terminal_in_a_session_of_their_own() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let startup = dir.write_executable("startup", "#!/bin/sh\ntty\nps -o sid= -p $$\n");
    let supervisor = supervisor_in(&dir, startup);
    let (mut master, slave) = zbm_console::host::open_pty().unwrap();
    let mut attached = Some(Attached::for_tests(0, slave));

    let pid = supervisor
        .spawn(&supervisor.startup, &[], false, attached.as_ref())
        .unwrap();
    let status = supervisor.wait_for(pid, &mut attached, &mut |_| {});
    // The output may come in more than one read.
    let mut shown = String::new();
    while shown.lines().count() < 2 {
        let mut chunk = vec![0_u8; 256];
        let length = std::io::Read::read(&mut master, &mut chunk).unwrap();
        shown.push_str(&String::from_utf8_lossy(&chunk[..length]));
    }

    assert_eq!(status, Some(0));
    assert!(shown.starts_with("/dev/pts/"), "{shown:?}");
    assert_eq!(
        shown.lines().nth(1).map(str::trim),
        Some(pid.to_string().as_str())
    );
}

#[test]
#[allow(
    clippy::zombie_processes,
    reason = "the supervisor collects the console with waitpid(-1)"
)]
fn when_the_console_ends_the_terminal_goes_back_to_the_kernel_console() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let startup = dir.write_executable("startup", "#!/bin/sh\nsleep 0.3\n");
    let supervisor = supervisor_in(&dir, startup);
    let console = std::process::Command::new("true").spawn().unwrap();
    let (_master, slave) = zbm_console::host::open_pty().unwrap();
    dir.write("console", format!("{}\n", console.id()));
    let mut attached = Some(Attached::for_tests(console.id(), slave));
    let mut events = Vec::new();

    let pid = supervisor
        .spawn(&supervisor.startup, &[], false, None)
        .unwrap();
    let status = supervisor.wait_for(pid, &mut attached, &mut |event| events.push(event));

    assert_eq!(status, Some(0));
    assert!(attached.is_none());
    assert_eq!(events, [Event::Console("ended 0".to_owned())]);
    assert!(!dir.join("console").exists());
    let kmsg = std::fs::read_to_string(dir.join("kmsg")).unwrap();
    assert!(
        kmsg.contains("the console on the display ended (wait status 0)"),
        "{kmsg}"
    );
}

#[test]
fn with_a_console_the_programs_and_the_message_go_to_its_terminal() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let shown = dir.join("shown");
    // The console keeps what the programs write.
    let console = dir.write_executable(
        "display",
        format!("#!/bin/sh\nexec cat > {}\n", shown.display()),
    );
    let startup = dir.write_executable("startup", "#!/bin/sh\necho started on $(tty)\n");
    let mut supervisor = supervisor_in(&dir, startup);
    supervisor.console = Some(Program {
        path: console,
        args: Vec::new(),
    });

    let mut events = Vec::new();
    supervisor.run(Some(1), &mut |event| events.push(event));
    let _ = zbm_sys::restore_default(Signal::Interrupt);

    let Some(Event::Console(started)) = events.first() else {
        panic!("{events:?}");
    };
    let pid = started.strip_prefix("started ").unwrap().to_owned();
    // The console records itself (`zbm console`); this one does not.
    assert!(!dir.join("console").exists());
    assert_eq!(events.last(), Some(&Event::Shell));
    std::thread::sleep(Duration::from_millis(200));
    let _ = std::process::Command::new("kill").arg(&pid).status();
    let text = std::fs::read_to_string(&shown).unwrap();
    assert!(text.contains("started on /dev/pts/"), "{text:?}");
    assert!(
        text.contains("type 'exit' to start it again\r\n"),
        "{text:?}"
    );
}

#[test]
fn after_a_crash_the_logs_are_saved_by_a_process_of_its_own_when_asked() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let calls = dir.join("calls");
    let startup = dir.write_executable(
        "startup",
        format!("#!/bin/sh\necho \"$*\" >> {}\n", calls.display()),
    );
    let supervisor = supervisor_in(&dir, startup);

    supervisor.run(Some(1), &mut |_| {});
    dir.write("zbm.cmdline", "zbm.save_logs\n");
    supervisor.run(Some(1), &mut |_| {});
    let _ = zbm_sys::restore_default(Signal::Interrupt);
    // The saving process is not waited for.
    let mut text = String::new();
    for _ in 0..100 {
        text = std::fs::read_to_string(&calls).unwrap_or_default();
        if text.lines().count() == 3 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = zbm_sys::wait_any();

    assert_eq!(text, "\n\ndebug save --after-crash\n");
}

#[test]
fn the_recovery_shell_gets_the_terminal_s_modes_back() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let modes = dir.join("modes");
    let startup = dir.write_executable("startup", "#!/bin/sh\nstty raw -echo\nkill -9 $$\n");
    let mut supervisor = supervisor_in(&dir, startup);
    supervisor.shell = Program {
        path: dir.write_executable(
            "shell",
            format!("#!/bin/sh\nstty -a > {}\n", modes.display()),
        ),
        args: Vec::new(),
    };
    let (mut master, slave) = zbm_console::host::open_pty().unwrap();
    let reader = std::thread::spawn(move || {
        let mut shown = Vec::new();
        let _ = std::io::Read::read_to_end(&mut master, &mut shown);
        shown
    });
    let mut attached = Some(Attached::for_tests(0, slave));

    supervisor.round(false, &mut attached, &mut |_| {});
    let _ = zbm_sys::restore_default(Signal::Interrupt);
    drop(attached);

    let shown = String::from_utf8_lossy(&reader.join().unwrap()).into_owned();
    let text = std::fs::read_to_string(&modes).unwrap();
    let flags: Vec<&str> = text.split_whitespace().collect();
    assert!(flags.contains(&"isig") && flags.contains(&"echo"), "{text}");
    assert!(shown.contains("\x1b[?25h"), "{shown:?}");
}

/// A supervisor whose recovery shell writes `ran` to `shell-ran`.
fn with_marking_shell(dir: &TempDir) -> Supervisor {
    let startup = dir.write_executable("startup", "#!/bin/sh\n");
    let mut supervisor = supervisor_in(dir, startup);
    let marker = dir.join("shell-ran");
    supervisor.shell = Program {
        path: dir.write_executable(
            "shell",
            format!("#!/bin/sh\necho ran > {}\n", marker.display()),
        ),
        args: Vec::new(),
    };
    supervisor
}

#[test]
#[allow(
    clippy::zombie_processes,
    reason = "the supervisor collects the console with waitpid"
)]
fn a_shell_after_the_console_ended_runs_on_the_kernel_console() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let supervisor = with_marking_shell(&dir);
    // The console ended, closing its side of the terminal, and the start-up
    // program, which lost the terminal with it, was collected first.
    let console = std::process::Command::new("true").spawn().unwrap();
    let (master, slave) = zbm_console::host::open_pty().unwrap();
    drop(master);
    let mut attached = Some(Attached::for_tests(console.id(), slave));
    let mut events = Vec::new();

    supervisor.recovery_shell(None, &mut attached, &mut |event| events.push(event));

    assert!(attached.is_none());
    assert_eq!(events, [Event::Console("ended 0".to_owned()), Event::Shell]);
    // Not on the dead terminal, where it would have ended at once.
    assert_eq!(
        std::fs::read_to_string(dir.join("shell-ran")).unwrap(),
        "ran\n"
    );
    let kmsg = std::fs::read_to_string(dir.join("kmsg")).unwrap();
    assert!(
        kmsg.contains("the console on the display ended (wait status 0)"),
        "{kmsg}"
    );
}

#[test]
fn a_hung_up_terminal_is_given_up_even_if_its_console_is_not_collected() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let supervisor = with_marking_shell(&dir);
    dir.write("console", "1\n");
    let (master, slave) = zbm_console::host::open_pty().unwrap();
    drop(master);
    // Not a child of the supervisor: it cannot be waited for.
    let mut attached = Some(Attached::for_tests(std::process::id(), slave));
    let mut events = Vec::new();

    supervisor.round(true, &mut attached, &mut |event| events.push(event));
    let _ = zbm_sys::restore_default(Signal::Interrupt);

    assert!(attached.is_none());
    assert_eq!(events.first(), Some(&Event::Console("hung up".to_owned())));
    assert_eq!(events.last(), Some(&Event::Shell));
    assert!(!dir.join("console").exists());
    assert!(dir.join("shell-ran").exists());
    let kmsg = std::fs::read_to_string(dir.join("kmsg")).unwrap();
    assert!(
        kmsg.contains("hung up; going on on the kernel console"),
        "{kmsg}"
    );
}

#[test]
fn bash_opens_when_the_shell_fails_at_once_or_cannot_start() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let mut supervisor = with_marking_shell(&dir);
    supervisor.fallback = Some(supervisor.shell.clone());
    supervisor.shell = Program {
        path: dir.write_executable("brush", "#!/bin/sh\nexit 1\n"),
        args: Vec::new(),
    };
    let mut events = Vec::new();

    supervisor.recovery_shell(None, &mut None, &mut |event| events.push(event));

    assert_eq!(events, [Event::Shell, Event::Shell]);
    assert_eq!(
        std::fs::read_to_string(dir.join("shell-ran")).unwrap(),
        "ran\n"
    );
    let kmsg = std::fs::read_to_string(dir.join("kmsg")).unwrap();
    assert!(kmsg.contains("brush did not open; starting"), "{kmsg}");

    std::fs::remove_file(dir.join("shell-ran")).unwrap();
    supervisor.shell.path = dir.join("missing");
    let mut events = Vec::new();
    supervisor.recovery_shell(None, &mut None, &mut |event| events.push(event));
    let _ = zbm_sys::restore_default(Signal::Interrupt);
    assert!(matches!(
        events.as_slice(),
        [Event::Failed(_), Event::Shell]
    ));
    assert!(dir.join("shell-ran").exists());
}

#[test]
fn without_a_chosen_shell_the_recovery_shell_is_bash() {
    let supervisor = Supervisor::new(PathBuf::from("/libexec/zbm"));
    assert_eq!(supervisor.shell.path, PathBuf::from("/bin/bash"));
    assert_eq!(supervisor.shell.args, ["-i"]);
    assert_eq!(supervisor.fallback, None);
}
