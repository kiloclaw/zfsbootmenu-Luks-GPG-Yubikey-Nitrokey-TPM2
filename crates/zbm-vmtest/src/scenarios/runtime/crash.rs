//! `runtime-rust-crash`: a panic in the Rust image leaves a crash report
//! that the image's debug file resolves: the frame of the panicking
//! function names its file and line (`REFACTOR-ROADMAP.md`, "Crash
//! reports").

use std::fs;
use std::path::Path;
use std::process::Command;

use super::host::{host_user_command, is_on_path};
use crate::error::{Error, Result};
use crate::expect::regex;
use crate::images::{ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

/// An early-setup hook that panics, then prints the crash report and where
/// else the crash was recorded.
const HOOK: &str = r##"#!/bin/bash
/libexec/zbm debug panic >/dev/null 2>&1
echo "ZBMTEST-CRASH-BEGIN"
/libexec/zbm debug crash
echo "ZBMTEST-CRASH-END"
echo "ZBMTEST-CRASH-KMSG $(dmesg | grep -c 'debug crashed: zbm debug panic')"
echo "ZBMTEST-CRASH-JOURNAL $(grep -c '"crash report"' /zfsbootmenu/journal.jsonl)"
"##;

/// What the console showed of the crash.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Crash {
    /// The crash report.
    report: String,
    /// How many kernel log lines name the crash.
    kmsg: u64,
    /// How many journal records hold the report.
    journal: u64,
}

/// What a host command printed and how it ended.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Panics in the image, then symbolizes the crash report on the host.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let spec = ImageSpec {
        runtime: Some(ZbmRuntime::Rust),
        features: Some("zbm/crash-test".into()),
        files: vec![(
            "hooks/early-setup.d/95-zbmtest-crash".into(),
            HOOK.as_bytes().to_vec(),
        )],
        ..ImageSpec::new("auto-crash")
    };
    let image = zbm_image(settings, &spec)?;
    let machine =
        Machine::boot(&image, super::ESCAPE_APPEND).with_disks([super::disk(settings, 0)?]);
    let mut vm = Vm::start(test, "boot", &machine)?;
    let crash = read_crash(&mut vm)?;
    vm.kill();
    let path = test.dir().join("crash-report.txt");
    fs::write(&path, &crash.report).map_err(Error::io(format!("writing {}", path.display())))?;
    test.note(&crash.report);
    check_crash(test, &crash);
    symbolize(test, &path, &image.debug)
}

/// Reads the crash report and the counts the hook prints.
fn read_crash(vm: &mut Vm<'_>) -> Result<Crash> {
    vm.expect1("ZBMTEST-CRASH-BEGIN", 1200)?;
    vm.expect1("ZBMTEST-CRASH-END", 60)?;
    let report = format!("{}\n", vm.before().trim_matches('\n'));
    vm.expect1(r"ZBMTEST-CRASH-KMSG (\d+)", 60)?;
    let kmsg = count(&vm.group(1))?;
    vm.expect1(r"ZBMTEST-CRASH-JOURNAL (\d+)", 60)?;
    let journal = count(&vm.group(1))?;
    Ok(Crash {
        report,
        kmsg,
        journal,
    })
}

/// A count the hook printed.
fn count(text: &str) -> Result<u64> {
    text.parse()
        .map_err(|_| Error::Setup(format!("not a count: {text}")))
}

/// Checks the report and where else the crash was recorded.
fn check_crash(test: &Test, crash: &Crash) {
    let report = &crash.report;
    test.check(
        report.starts_with("ZFSBootMenu crash report\n"),
        "zbm debug crash printed the report",
    );
    test.check(
        report.contains("reason: panicked at crates/zbm/src/debug.rs:"),
        "the report says where the panic was",
    );
    test.check(
        report.contains("crash_test{reason=zbm debug panic}"),
        "the report names the open span",
    );
    test.check(crash.kmsg >= 1, "the kernel log names the crash");
    test.check(crash.journal >= 1, "the journal holds the report");
}

/// Runs `cargo xtask symbolize` on the report with the image's debug file
/// and checks that it names the panicking function and its line.
fn symbolize(test: &Test, report: &Path, debug: &Path) -> Result<()> {
    let args: Vec<String> = ["cargo", "xtask", "symbolize"]
        .iter()
        .map(|&arg| arg.to_owned())
        .chain([
            report.display().to_string(),
            "--debug-dir".to_owned(),
            debug.display().to_string(),
        ])
        .collect();
    let command = host_user_command(&args)?;
    let missing: Vec<&str> = ["cargo", "addr2line"]
        .into_iter()
        .filter(|tool| !is_on_path(tool, &command.path))
        .collect();
    let ran = if missing.is_empty() {
        run_host(&command.args, &test.settings().repo)?
    } else {
        Ran {
            code: 127,
            stdout: String::new(),
            stderr: format!(
                "not found on this host: {} (cargo: Rust; addr2line: binutils)\n",
                missing.join(", ")
            ),
        }
    };
    let output = format!("{}{}", ran.stdout, ran.stderr);
    let path = test.dir().join("symbolized.txt");
    fs::write(&path, &output).map_err(Error::io(format!("writing {}", path.display())))?;
    test.note(&output);
    check_symbolized(test, &ran, &missing)
}

/// Checks the output of `cargo xtask symbolize`.
fn check_symbolized(test: &Test, ran: &Ran, missing: &[&str]) -> Result<()> {
    let missing = if missing.is_empty() {
        String::new()
    } else {
        format!(" (missing: {})", missing.join(", "))
    };
    test.check(
        ran.code == 0,
        &format!("cargo xtask symbolize ran{missing}"),
    );
    let frame = regex(r"zbm::debug::panic_for_test at \S*crates/zbm/src/debug\.rs:\d+")?;
    test.check(
        frame.is_match(&ran.stdout),
        "the symbolized report names the function and its line",
    );
    Ok(())
}

/// Runs `args` in `dir`; returns how it ended and what it printed.
fn run_host(args: &[String], dir: &Path) -> Result<Ran> {
    let (program, rest) = args
        .split_first()
        .ok_or_else(|| Error::Setup("an empty command".into()))?;
    let output = Command::new(program)
        .args(rest)
        .current_dir(dir)
        .output()
        .map_err(Error::io(format!("running {program}")))?;
    Ok(Ran {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[cfg(test)]
mod tests;
