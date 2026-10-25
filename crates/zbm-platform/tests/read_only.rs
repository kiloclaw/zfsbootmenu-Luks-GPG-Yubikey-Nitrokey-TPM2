//! Tests of the runner of shadow mode on a fake machine.
//!
//! They are integration tests because the fakes of `zbm-testkit` implement
//! the traits of this crate as other crates see it.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    reason = "integration tests set up real files and fail loudly when they cannot"
)]

use std::time::Duration;

use pretty_assertions::assert_eq;
use zbm_platform::{
    ALLOWED, BASH_MATCH, Command, Input, ReadOnlyRunner, Runner, Secret, is_read_only,
};
use zbm_testkit::cases;
use zbm_testkit::fakes::FakeSystem;

#[test]
fn the_allowed_commands_are_pinned() {
    // Adding a command here is a decision: shadow mode must never change
    // the machine it runs on.
    assert_eq!(
        ALLOWED,
        &[
            ("zfs", &["list", "get"][..]),
            ("zpool", &["list", "get", "status"][..]),
            (
                "cryptsetup",
                &["luksDump", "status", "luksUUID", "isLuks"][..]
            ),
            ("blkid", &[][..]),
        ]
    );
    assert_eq!(BASH_MATCH, "[[ $1 =~ $2 ]]");
}

#[test]
fn only_reading_commands_are_read_only() {
    cases! {
        for (line, read_only) in {
            "zfs get" => ("zfs get -H -o value bootfs tank", true),
            "zfs list by path" => ("/usr/sbin/zfs list -H", true),
            "zpool status" => ("zpool status -P tank", true),
            "luksUUID" => ("cryptsetup luksUUID /dev/sda2", true),
            "blkid" => ("blkid -o device", true),
            "zpool import" => ("zpool import -N tank", false),
            "zpool export" => ("zpool export tank", false),
            "zfs set" => ("zfs set canmount=on tank", false),
            "zfs load-key" => ("zfs load-key tank", false),
            "cryptsetup open" => ("cryptsetup open /dev/sda2 v", false),
            "kexec" => ("kexec -e", false),
            "bare zpool" => ("zpool", false),
        } {
            let mut words = line.split(' ');
            let command = Command::new(words.next().unwrap()).args(words);
            assert_eq!(is_read_only(&command), read_only);
        }
    }
}

#[test]
fn bash_runs_only_the_regular_expression_match() {
    let matching = Command::new("bash").args(["-c", BASH_MATCH, "bash", "x", "y"]);
    let other = Command::new("bash").args(["-c", "rm -rf /"]);

    assert!(is_read_only(&matching));
    assert!(!is_read_only(&other));
    assert!(!is_read_only(&Command::new("bash")));
}

#[test]
fn secrets_and_a_spent_budget_are_refused_and_the_console_is_not_read() {
    let fake = FakeSystem::new();
    let runner = ReadOnlyRunner::new(&fake.runner, &fake.clock, Duration::from_secs(3));
    let status = Command::new("cryptsetup").args(["status", "v"]);

    let secret = runner.run(
        &status
            .clone()
            .stdin(Input::Secret(Secret::copy_from(b"pw"))),
    );
    let console = runner.run(&status.clone().stdin(Input::Inherit));
    fake.clock.advance(Duration::from_secs(3));
    let after = runner.run(&status.stdin(Input::Null));

    assert!(secret.is_err() && after.is_err());
    assert!(console.is_ok());
    let calls = fake.runner.calls();
    assert_eq!(fake.runner.lines(), ["cryptsetup status v"]);
    assert_eq!(calls[0].get_stdin(), &Input::Null);
    // A command still running when the budget ends is killed.
    assert_eq!(calls[0].get_timeout(), Some(Duration::from_secs(3)));
    assert!(
        after
            .unwrap_err()
            .to_string()
            .starts_with("the time budget is spent")
    );
}

#[test]
fn a_command_gets_what_is_left_of_the_budget() {
    let fake = FakeSystem::new();
    let runner = ReadOnlyRunner::new(&fake.runner, &fake.clock, Duration::from_secs(5));

    fake.clock.advance(Duration::from_secs(2));
    runner.run(&Command::new("blkid")).unwrap();
    runner
        .run(
            &Command::new("zpool")
                .arg("status")
                .timeout(Duration::from_secs(1)),
        )
        .unwrap();

    let calls = fake.runner.calls();
    assert_eq!(calls[0].get_timeout(), Some(Duration::from_secs(3)));
    assert_eq!(calls[1].get_timeout(), Some(Duration::from_secs(1)));
}
