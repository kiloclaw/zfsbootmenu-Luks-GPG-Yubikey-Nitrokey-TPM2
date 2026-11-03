//! Tests of the read/write import.

use std::path::Path;

use zbm_platform::exec::{Output, Status};
use zbm_platform::interaction::{NoInteraction, Resume, ResumeRequest};
use zbm_platform::{Installed, KernelLog};
use zbm_testkit::fakes::FakeSystem;

use super::*;

fn name(text: &str) -> PoolName {
    text.parse().unwrap()
}

/// [`set_rw_pool`] on `fake`, where no one answers.
fn set_rw(fake: &FakeSystem, pool: &str, policy: ImportPolicy) -> Result<(), WritableError> {
    let state = State::new(&fake.files, "/zfsbootmenu");
    let context = Context::new(fake.system());
    set_rw_pool(&context, &NoInteraction, &state, &name(pool), policy)
}

const BCLONE: &str = "/sys/module/zfs/parameters/zfs_bclone_enabled";

/// A machine where `tank` is imported read-only with `flag` as its
/// [`READONLY`] property and no suspend partition.
fn machine(flag: &str) -> (FakeSystem, Installed) {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu").file(BCLONE, "1\n");
    let log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "on\n");
    fake.runner.respond(
        &format!("zpool get -H -o value {READONLY} tank"),
        0,
        &format!("{flag}\n"),
    );
    fake.runner.respond_program("blkid", 2, "");
    (fake, log)
}

#[test]
fn a_read_only_pool_is_exported_and_imported_read_write() {
    let (fake, _log) = machine("-");

    set_rw(&fake, "tank", ImportPolicy::Force).unwrap();

    let lines = fake.runner.lines();
    assert_eq!(
        lines[lines.len() - 2..],
        [
            "zpool export tank",
            "zpool import -N -f -o readonly=off tank"
        ]
    );
    assert_eq!(fake.files.contents(BCLONE).as_deref(), Some(&b"0"[..]));
}

#[test]
fn a_writable_pool_is_left_as_it_is() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");

    set_rw(&fake, "tank", ImportPolicy::default()).unwrap();

    assert_eq!(fake.runner.lines(), ["zpool get -H -o value readonly tank"]);
}

#[test]
fn the_readonly_property_and_a_degraded_pool_forbid_it() {
    let (fake, _log) = machine("on");
    let state = State::new(&fake.files, "/zfsbootmenu");
    state.add_degraded("tank").unwrap();

    let error = set_rw(&fake, "tank", ImportPolicy::default()).unwrap_err();

    assert_eq!(
        error.to_string(),
        "tank may not be imported read/write: org.zfsbootmenu:readonly is on; it is degraded"
    );
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.contains("export"))
    );

    let unreadable = FakeSystem::new();
    unreadable.runner.respond_program("zpool", 1, "");
    assert!(matches!(
        set_rw(&unreadable, "tank", ImportPolicy::default()),
        Err(WritableError::PoolProperty { .. })
    ));
}

#[test]
fn a_failed_import_or_export_keeps_it_read_only() {
    let (fake, _log) = machine("off");
    fake.runner.respond("zpool export tank", 1, "");
    assert!(matches!(
        set_rw(&fake, "tank", ImportPolicy::default()),
        Err(WritableError::Pool(PoolError::Export { .. }))
    ));

    let (fake, _log) = machine("off");
    fake.runner
        .respond("zpool import -N -o readonly=off tank", 1, "");
    assert!(matches!(
        set_rw(&fake, "tank", ImportPolicy::default()),
        Err(WritableError::Pool(PoolError::Import { .. }))
    ));
    // The export went through: the pool is imported read-only again, not
    // left exported.
    let lines = fake.runner.lines();
    assert_eq!(
        lines[lines.len() - 2..],
        [
            "zpool import -N -o readonly=off tank",
            "zpool import -N -o readonly=on tank"
        ]
    );
}

/// The answer the tests give to the resume question, and the pools it was
/// asked about.
struct Answers {
    answer: Resume,
    asked: std::sync::Mutex<Vec<String>>,
}

impl Interaction for Answers {
    fn ask(&self, _: &zbm_platform::interaction::PromptRequest) -> zbm_types::Answer {
        zbm_types::Answer::Escape
    }

    fn secret(
        &self,
        _: &zbm_platform::interaction::SecretRequest<'_>,
    ) -> zbm_platform::interaction::Asked {
        zbm_platform::interaction::Asked::Ended
    }

    fn wait(
        &self,
        _: &zbm_platform::interaction::WaitRequest<'_>,
    ) -> zbm_platform::interaction::Wait {
        zbm_platform::interaction::Wait::TimedOut
    }

    fn notify(&self, _: zbm_platform::interaction::Tell<'_>) {}

    fn resume(&self, request: &ResumeRequest<'_>) -> Resume {
        self.asked.lock().unwrap().push(request.pool.to_owned());
        self.answer
    }
}

#[test]
fn with_a_suspend_partition_the_resume_question_decides() {
    for (answer, imported) in [
        (Resume::Noresume, true),
        (Resume::Dangerous, true),
        (Resume::Refuse, false),
    ] {
        let (fake, _log) = machine("-");
        // A suspend partition, found before `machine`'s blkid finds none.
        fake.runner.first(|command| {
            (command.display() == "blkid -t TYPE=suspend")
                .then(|| Ok(Output::new(Status::exited(0), "")))
        });
        let state = State::new(&fake.files, "/zfsbootmenu");
        let answers = Answers {
            answer,
            asked: std::sync::Mutex::default(),
        };

        let done = set_rw_pool(
            &Context::new(fake.system()),
            &answers,
            &state,
            &name("tank"),
            ImportPolicy::default(),
        );

        assert_eq!(*answers.asked.lock().unwrap(), ["tank"], "{answer:?}");
        let exported = fake
            .runner
            .lines()
            .contains(&"zpool export tank".to_owned());
        assert_eq!((done.is_ok(), exported), (imported, imported), "{answer:?}");
        assert_eq!(state.noresume().is_some(), answer == Resume::Noresume);
        if answer == Resume::Refuse {
            assert!(matches!(done, Err(WritableError::ResumeRefused { .. })));
        }
    }
}
