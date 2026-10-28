//! The supervisor and the console on the display: the programs on its
//! terminal, its end, its hang-up and its restarts.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "tests may panic; the rules are for process 1"
)]
use super::*;

#[test]
fn programs_run_on_the_console_s_terminal_in_a_session_of_their_own() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let startup = dir.write_executable("startup", "#!/bin/sh\ntty\nps -o sid= -p $$\n");
    let supervisor = supervisor_in(&dir, startup);
    let (mut master, slave) = zbm_console::host::open_pty().unwrap();
    let attached = Some(Attached::for_tests(0, slave));
    let mut events = Vec::new();

    let begin = Begin::Startup { resume: false };
    supervisor.run_from(attached, begin, Some(1), &mut |event| events.push(event));
    let Some(&Event::Started(pid)) = events.first() else {
        panic!("{events:?}");
    };
    // The output may come in more than one read.
    let mut shown = String::new();
    while shown.lines().count() < 2 {
        let mut chunk = vec![0_u8; 256];
        let length = std::io::Read::read(&mut master, &mut chunk).unwrap();
        shown.push_str(&String::from_utf8_lossy(&chunk[..length]));
    }

    assert_eq!(events.get(1), Some(&Event::Ended(0)));
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
    let attached = Some(Attached::for_tests(console.id(), slave));
    let mut events = Vec::new();

    let begin = Begin::Startup { resume: false };
    supervisor.run_from(attached, begin, Some(1), &mut |event| events.push(event));

    // While the start-up program runs.
    assert!(
        matches!(
            events.as_slice(),
            [
                Event::Started(_),
                Event::Console(ended),
                Event::Ended(0),
                Event::Shell
            ] if ended == "ended 0"
        ),
        "{events:?}"
    );
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
    let attached = Some(Attached::for_tests(console.id(), slave));
    let mut events = Vec::new();

    supervisor.run_from(attached, Begin::Shell, Some(1), &mut |event| {
        events.push(event);
    });

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
    let attached = Some(Attached::for_tests(std::process::id(), slave));
    let mut events = Vec::new();

    let begin = Begin::Startup { resume: true };
    supervisor.run_from(attached, begin, Some(1), &mut |event| events.push(event));

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
fn a_console_that_ends_starts_again_until_its_tries_are_used_up() {
    let _guard = PROCESSES.lock().unwrap();
    let dir = TempDir::new("zbm-supervisor");
    let starts = dir.join("starts");
    let startup = dir.write_executable("startup", "#!/bin/sh\n");
    let mut supervisor = supervisor_in(&dir, startup);
    supervisor.console = Some(Program {
        path: dir.write_executable(
            "display",
            format!("#!/bin/sh\necho $$ >> {}\n", starts.display()),
        ),
        args: Vec::new(),
    });
    supervisor.console_restart = Backoff {
        tries: 2,
        first: Duration::from_millis(10),
        max: Duration::from_millis(20),
        healthy: Duration::from_secs(60),
    };
    let mut events = Vec::new();

    supervisor.run(Some(1), &mut |event| events.push(event));
    let _ = zbm_sys::restore_default(Signal::Interrupt);

    let console: Vec<String> = events
        .iter()
        .filter_map(|event| match event {
            Event::Console(text) => Some(text.split(' ').next().unwrap_or("").to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(
        console,
        [
            "started",
            "ended",
            "restarted",
            "ended",
            "restarted",
            "ended",
            "given"
        ],
        "{events:?}"
    );
    assert_eq!(events.last(), Some(&Event::Shell));
    let text = std::fs::read_to_string(&starts).unwrap();
    assert_eq!(text.lines().count(), 3, "{text}");
    let kmsg = std::fs::read_to_string(dir.join("kmsg")).unwrap();
    assert!(kmsg.contains("ended 3 times in a row"), "{kmsg}");
}

#[test]
fn programs_on_the_console_get_its_terminal_description() {
    let (_master, slave) = zbm_console::host::open_pty().unwrap();
    let attached = Attached::for_tests(0, slave);
    let program = Program {
        path: PathBuf::from("/bin/sh"),
        args: Vec::new(),
    };

    let command = attached.command(&program, &[]).unwrap();

    let vars: Vec<(String, String)> = command
        .get_envs()
        .filter_map(|(key, value)| Some((key.to_str()?.to_owned(), value?.to_str()?.to_owned())))
        .collect();
    let get = |name: &str| {
        vars.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    assert_eq!(get("TERM"), Some("xterm-256color"));
    assert_eq!(get("COLORTERM"), Some("truecolor"));
    assert_eq!(get(crate::supervisor::console::MARKER), Some("drm"));
}
