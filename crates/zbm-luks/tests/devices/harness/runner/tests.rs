use std::time::Duration;

use zbm_platform::exec::{Command, Input, Stream};

use super::{DeviceRunner, lab_devices, with_args};

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

/// A command as `Devices` builds one, with `args`.
fn probe(args: &[&str]) -> Command {
    Command::new("blkid")
        .stdin(Input::Null)
        .timeout(Duration::from_secs(60))
        .args(args.iter().copied())
        .stdout(Stream::Capture)
        .stderr(Stream::Null)
}

#[test]
fn a_search_lists_devices_with_no_device_named() {
    let luks = ["-c", "/dev/null", "-o", "device", "-t", "TYPE=crypto_LUKS"];
    assert!(DeviceRunner::is_search(&strings(&luks)));
    let find = ["-c", "/dev/null", "-l", "-o", "device", "-t", "UUID=1234"];
    assert!(DeviceRunner::is_search(&strings(&find)));
}

#[test]
fn a_probe_of_a_named_device_or_another_format_is_no_search() {
    let tag = [
        "-c",
        "/dev/null",
        "-s",
        "LABEL",
        "-o",
        "value",
        "/dev/loop0",
    ];
    assert!(!DeviceRunner::is_search(&strings(&tag)));
    let named = ["-o", "device", "/dev/loop0"];
    assert!(!DeviceRunner::is_search(&strings(&named)));
    let full = ["-t", "TYPE=swsuspend"];
    assert!(!DeviceRunner::is_search(&strings(&full)));
}

#[test]
fn a_search_finds_only_the_labs_loop_devices_and_their_partitions() {
    let loops = strings(&["/dev/loop1", "/dev/loop3"]);
    let text = "/dev/sda2\n/dev/loop1\n/dev/loop10\n/dev/loop3p1\n/dev/nvme0n1p3\n";
    assert_eq!(
        lab_devices(text, &loops, false),
        ["/dev/loop1", "/dev/loop3p1"]
    );
    assert_eq!(lab_devices(text, &loops, true), ["/dev/loop1"]);
    assert!(lab_devices("/dev/sda2\n", &loops, false).is_empty());
}

#[test]
fn a_search_without_its_first_only_option_keeps_the_rest_of_the_command() {
    let find = ["-c", "/dev/null", "-l", "-o", "device", "-t", "UUID=1234"];
    let all = strings(&find);
    let without = with_args(&probe(&find), all.iter().filter(|arg| *arg != "-l"));
    let expected = ["-c", "/dev/null", "-o", "device", "-t", "UUID=1234"];
    assert_eq!(without, probe(&expected));
}

#[test]
fn every_setting_of_a_command_is_kept_with_new_arguments() {
    let token = zbm_platform::event::CancelToken::new().unwrap();
    let command = Command::new("blkid")
        .merge_stderr()
        .env("A", "1")
        .env_remove("B")
        .default_signal(zbm_platform::exec::Signal::Pipe)
        .cancel(&token)
        .root("/new-root")
        .output_limit(4096)
        .arg("-l");
    let args = strings(&["-o", "device"]);
    let rebuilt = with_args(&command, &args);
    let expected = Command::new("blkid")
        .merge_stderr()
        .env("A", "1")
        .env_remove("B")
        .default_signal(zbm_platform::exec::Signal::Pipe)
        .cancel(&token)
        .root("/new-root")
        .output_limit(4096)
        .args(["-o", "device"]);
    assert_eq!(rebuilt, expected);
}
