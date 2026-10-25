//! Waiting for devices: the start-up stage `init.d/30-device-wait`.

use std::path::Path;

use zbm_platform::System;

use zbm_boot::prompt::{Answer, TimedPrompt, colorize, retry_delay};

/// The fields `IFS=',' read -r -a fields <<< "${text}"` reads: a trailing
/// empty field is dropped, other empty fields are kept.
fn comma_fields(text: &str) -> Vec<&str> {
    let mut fields: Vec<&str> = text.split(',').collect();
    if fields.last() == Some(&"") {
        fields.pop();
    }
    fields
}

/// The path a `zbm.wait_for` entry names: a `/dev` path itself, or
/// `<type>=<name>` as `/dev/disk/by-<type>/<name>` with the type in lower
/// case. Other entries are malformed.
pub fn device_path(device: &str) -> Option<String> {
    if device.starts_with("/dev/") {
        return Some(device.to_owned());
    }
    // `${device%=*}` and `${device##*=}` split at the last `=`.
    let (kind, name) = device.rsplit_once('=')?;
    let prefix = format!("/dev/disk/by-{kind}").to_ascii_lowercase();
    Some(format!("{prefix}/{name}"))
}

/// Waits for the devices of `zbm.wait_for` (`wait_for`, separated by commas),
/// asking on the console every `retry_delay` seconds, until all exist or the
/// user cancels.
pub fn stage(system: &System<'_>, wait_for: &str, delay: &str) {
    stage_with(system, wait_for, delay, &mut |seconds| {
        missing_prompt(system, seconds)
    });
}

/// The prompt that says devices are missing, on the console: RETURN or the
/// time running out looks again, ESCAPE stops waiting.
pub fn missing_prompt(system: &System<'_>, delay: i64) -> Answer {
    TimedPrompt::new()
        .delay(delay)
        .on_escape("to cancel")
        .message("")
        .message(colorize("red", "One or more required devices are missing"))
        .prompt(|seconds| {
            let seconds = colorize("yellow", &format!("{seconds:02}"));
            format!("retrying in {seconds} seconds")
        })
        .run(system)
}

/// [`stage`] with `prompt` saying that devices are missing, given the delay
/// in seconds.
pub fn stage_with(
    system: &System<'_>,
    wait_for: &str,
    delay: &str,
    prompt: &mut dyn FnMut(i64) -> Answer,
) {
    if wait_for.is_empty() {
        return;
    }
    let devices = comma_fields(wait_for);
    loop {
        let mut missing = Vec::new();
        for device in &devices {
            match device_path(device) {
                Some(path) if system.files.exists(Path::new(&path)) => {}
                Some(_) => missing.push(*device),
                None => {
                    zbm_trace::error!("malformed device: '{device}'");
                }
            }
        }
        if missing.is_empty() {
            return;
        }
        if prompt(retry_delay(delay)) == Answer::Escape {
            for device in missing {
                zbm_trace::error!("required device '{device}' not found");
            }
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_platform::KernelLog;
    use zbm_testkit::fakes::FakeSystem;
    use zbm_testkit::legacy::{Bash, log_lines, path};
    use zbm_testkit::parity;

    use zbm_testkit::legacy::{LINE_LOG_STUBS, stub_lines};

    #[test]
    fn device_names() {
        assert_eq!(device_path("/dev/sda").as_deref(), Some("/dev/sda"));
        assert_eq!(
            device_path("UUID=1234-ABCD").as_deref(),
            Some("/dev/disk/by-uuid/1234-ABCD")
        );
        assert_eq!(
            device_path("PartLabel=a=b").as_deref(),
            Some("/dev/disk/by-partlabel=a/b")
        );
        assert_eq!(device_path("sda"), None);
        assert_eq!(device_path("/devx"), None);
        assert_eq!(comma_fields("a,,b,"), ["a", "", "b"]);
        assert_eq!(comma_fields(",a"), ["", "a"]);
    }

    fn setup() -> FakeSystem {
        let fake = FakeSystem::new();
        fake.files.dir("/zfsbootmenu");
        fake.runner.respond("tput lines", 0, "25\n");
        fake.runner.respond("tput cols", 0, "80\n");
        fake
    }

    #[test]
    fn present_devices_need_no_prompt() {
        let fake = setup();
        fake.files
            .device("/dev/sda")
            .device("/dev/disk/by-uuid/abc");
        let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
        stage(&fake.system(), "/dev/sda,UUID=abc", "");
        assert!(fake.runner.lines().is_empty());
    }

    #[test]
    fn missing_devices_are_retried_until_cancelled() {
        let fake = setup();
        fake.files.device("/dev/sda");
        fake.console.keys(b"\n\x1b");
        let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
        stage(&fake.system(), "/dev/sda,LABEL=boot", "3");
        let prompts = fake
            .runner
            .lines()
            .iter()
            .filter(|line| *line == "tput civis")
            .count();
        assert_eq!(prompts, 2);
        assert!(
            fake.console
                .output_text()
                .contains("retrying in \x1b[1;33m03\x1b[0m seconds")
        );
        assert_eq!(
            stub_lines(&fake.kmsg()),
            "ERROR: required device 'LABEL=boot' not found\n"
        );
    }

    #[test]
    fn matches_the_legacy_stage() {
        let mut parity = parity!("matches_the_legacy_stage");
        for wait_for in [
            "/dev/null",
            "/dev/null,/dev/zbm-missing,UUID=zbm-missing",
            "bad,,/dev/zero,",
            "Label=x=y,/dev/zbm-missing",
        ] {
            let (legacy_prompts, logged): (usize, String) = parity.legacy(wait_for, || {
                let script =
                    std::fs::read_to_string(path("zfsbootmenu/init.d/30-device-wait")).unwrap();
                let legacy = Bash::new()
                    .code(LINE_LOG_STUBS)
                    .code(&format!(
                        "timed_prompt() {{ echo PROMPT >&2; return 1; }}\n\
                         colorize() {{ printf '%s' \"$2\"; }}\n\
                         run() {{\nZFSBOOTMENU_INITIALIZATION=yes\n{script}\n}}"
                    ))
                    .env("zbm_wait_for_devices", wait_for)
                    .call("run", &[], b"");
                assert!(legacy.status.is_success(), "{}", legacy.stderr_text());
                (
                    legacy.stderr_text().matches("PROMPT").count(),
                    log_lines(&legacy.stderr_text()),
                )
            });

            let fake = setup();
            fake.files.device("/dev/null").device("/dev/zero");
            fake.console.keys(b"\x1b");
            let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
            stage(&fake.system(), wait_for, "");
            let prompts = fake
                .runner
                .lines()
                .iter()
                .filter(|line| *line == "tput civis")
                .count();
            assert_eq!(prompts, legacy_prompts, "{wait_for}");
            assert_eq!(stub_lines(&fake.kmsg()), logged, "{wait_for}");
        }
    }
}
