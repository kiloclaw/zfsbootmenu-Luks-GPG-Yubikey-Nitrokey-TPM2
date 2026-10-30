//! The keyholder as a process (ROADMAP.md, SB10): `zbm unlock serve`,
//! started by `ProcessKeyholders` on a socket pair, answers a run and ends.
//! It is started without programs to run (`PATH` leads nowhere), so it
//! opens nothing on the machine that runs the tests: it says why not.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "the test starts the program it tests, and fails loudly"
)]

use std::os::unix::fs::PermissionsExt;

use zbm::unlock::ProcessKeyholders;
use zbm_app::keyholder::Keyholders;
use zbm_app::keyholder::{self, UnlockRequest};
use zbm_app::model::{Action, Event, Outcome};
use zbm_platform::interaction::NoInteraction;
use zbm_testkit::TempDir;

#[test]
fn a_keyholder_process_answers_a_run_and_ends() {
    let dir = TempDir::new("keyholder");
    let program = dir.path().join("zbm");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\nexport PATH=/nowhere\nexec {} \"$@\"\n",
            env!("CARGO_BIN_EXE_zbm")
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let keyholders = ProcessKeyholders::new().of(&program).quiet();
    let request = UnlockRequest {
        action: Action::UnlockAll,
        retry: false,
    };

    let events = keyholder::unlock(&keyholders, &NoInteraction, &request).unwrap();

    let Some(Event::Finished(Outcome::Failed { why, .. })) = events.last() else {
        panic!("{events:?}");
    };
    let expected = [
        zbm_app::NO_CRYPTSETUP.to_owned(),
        format!("unable to source {}", zbm_config::conf::PATH),
    ];
    assert!(expected.contains(&why[0]), "{why:?}");
    // A program that is not there starts no keyholder.
    let none = ProcessKeyholders::new().of(&dir.path().join("none"));
    assert!(none.start().is_err());
}

#[test]
fn the_keyholder_needs_its_connection_and_the_serve_command() {
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_zbm"))
            .arg("unlock")
            .args(args)
            .env_remove(keyholder::KEYHOLDER_FD)
            .output()
            .unwrap()
    };

    let alone = run(&["serve"]);
    assert_eq!(alone.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&alone.stderr);
    assert!(
        stderr.contains("no connection in ZBM_KEYHOLDER_FD"),
        "{stderr}"
    );
    for args in [
        &[][..],
        &["listen"],
        &["serve", "--fast"],
        &["serve", "--journal"],
    ] {
        assert_eq!(run(args).status.code(), Some(2), "{args:?}");
    }
}
