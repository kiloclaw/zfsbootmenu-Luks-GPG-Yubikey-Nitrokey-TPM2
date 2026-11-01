//! `generate-zbm` given an argument that is not valid UTF-8.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::process::Command;

#[test]
fn an_argument_that_is_not_utf8_is_refused_without_a_panic() {
    let output = Command::new(env!("CARGO_BIN_EXE_generate-zbm"))
        .arg("-c")
        .arg(OsString::from_vec(b"/etc/\xff.yaml".to_vec()))
        .output()
        .expect("generate-zbm starts");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("an argument is not valid UTF-8"),
        "{stderr}"
    );
    assert!(!stderr.contains("panicked"), "{stderr}");
}
