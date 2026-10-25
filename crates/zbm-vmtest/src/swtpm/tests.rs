use std::process::Command;
use std::time::Instant;

use pretty_assertions::assert_eq;

use super::*;

fn failed(output: &str) -> Error {
    Error::Command {
        command: "swtpm socket".into(),
        status: "exit status: 1".into(),
        output: output.into(),
    }
}

#[test]
fn the_port_and_the_next_are_free() {
    let port = free_port().unwrap();
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    assert!(TcpListener::bind(("127.0.0.1", port + 1)).is_ok());
}

#[test]
fn only_a_taken_port_is_tried_again() {
    let taken = "swtpm: Could not open TCP socket: Address already in use\n";
    assert!(is_port_taken(&failed(taken)));
    assert!(!is_port_taken(&failed(
        "swtpm: Could not lock access to lockfile"
    )));
    let io = Error::io("reading")(std::io::Error::other("Address already in use"));
    assert!(!is_port_taken(&io));
}

/// Starts a software TPM with `outcomes` as what `swtpm` does, in turn;
/// returns the result and the commands run.
fn started(outcomes: Vec<Result<String>>) -> (Result<u16>, Vec<String>) {
    let mut outcomes = outcomes.into_iter();
    let mut commands = Vec::new();
    let result = start_tcp_with(
        Path::new("/tpm"),
        Path::new("/tpm.pid"),
        "startup-clear",
        |c| {
            commands.push(format!("{c:?}"));
            outcomes.next().unwrap()
        },
    );
    (result, commands)
}

#[test]
fn swtpm_gets_its_ports_its_state_and_its_process_id_file() {
    let (result, commands) = started(vec![Ok(String::new())]);
    let port = result.unwrap();
    assert_eq!(commands.len(), 1);
    let command = &commands[0];
    assert!(
        command.contains(&format!("\"type=tcp,port={port}\"")),
        "{command}"
    );
    let control = format!("\"--ctrl\" \"type=tcp,port={}\"", port + 1);
    assert!(command.contains(&control), "{command}");
    for part in ["\"dir=/tpm\"", "\"startup-clear\"", "\"file=/tpm.pid\""] {
        assert!(command.contains(part), "{part} in {command}");
    }
}

#[test]
fn a_taken_port_is_given_up_for_others_three_times_at_most() {
    let taken = || Err(failed("Could not open TCP socket: Address already in use"));
    let (result, commands) = started(vec![taken(), Ok(String::new())]);
    assert!(result.is_ok());
    assert_eq!(commands.len(), 2);
    let (result, commands) = started(vec![taken(), taken(), taken()]);
    assert!(is_port_taken(&result.unwrap_err()));
    assert_eq!(commands.len(), 3);
}

#[test]
fn another_failure_is_not_tried_again() {
    let (result, commands) = started(vec![Err(failed("swtpm: no such directory"))]);
    assert!(!is_port_taken(&result.unwrap_err()));
    assert_eq!(commands.len(), 1);
}

#[test]
fn stopping_waits_for_the_process_to_exit() {
    let mut child = Command::new("sleep").arg("30").spawn().unwrap();
    let pid_file = std::env::temp_dir().join(format!("zbm-vmtest-swtpm-{}.pid", child.id()));
    fs::write(&pid_file, format!("{}\n", child.id())).unwrap();
    assert!(is_running(child.id()));
    let started = Instant::now();
    stop_pid_file(&pid_file);
    let waited = started.elapsed();
    let exited = child.try_wait().unwrap();
    fs::remove_file(&pid_file).ok();
    assert!(!is_running(child.id()));
    assert!(waited < Duration::from_secs(5), "{waited:?}");
    assert_eq!(exited.map(|status| status.success()), Some(false));
}

#[test]
fn without_a_process_id_there_is_nothing_to_stop() {
    let missing = std::env::temp_dir().join("zbm-vmtest-swtpm-missing.pid");
    stop_pid_file(&missing);
    assert!(!is_running(u32::MAX));
    assert!(is_running(std::process::id()));
}
