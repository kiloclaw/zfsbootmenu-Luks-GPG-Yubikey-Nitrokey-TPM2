//! `generate-zbm` on a pipe whose reader is gone.

use std::io::Read as _;
use std::os::unix::process::ExitStatusExt as _;
use std::process::{Command, Stdio};

#[test]
fn a_closed_pipe_does_not_end_the_program() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_generate-zbm"))
        .arg("--help")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("generate-zbm starts");
    // The reader goes away before the program writes.
    drop(child.stdout.take());
    let status = child.wait().expect("generate-zbm ends");
    // It ends as `--help` always does (status 1, like pod2usage), not by
    // SIGPIPE.
    assert_eq!(
        (status.signal(), status.code()),
        (None, Some(1)),
        "{status:?}"
    );

    // On an open pipe it still prints everything.
    let mut output = Command::new(env!("CARGO_BIN_EXE_generate-zbm"))
        .arg("--help")
        .stdout(Stdio::piped())
        .spawn()
        .expect("generate-zbm starts");
    let mut text = String::new();
    if let Some(mut stdout) = output.stdout.take() {
        stdout.read_to_string(&mut text).expect("the help is read");
    }
    assert_eq!(output.wait().expect("generate-zbm ends").code(), Some(1));
    assert!(text.contains("generate-zbm [options]"), "{text}");
}
