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

#[test]
fn the_pool_is_imported_again_by_its_guid_so_that_a_pool_of_its_name_is_left_alone() {
    let (fake, _log) = machine("off");
    fake.runner
        .respond("zpool get -H -o value guid tank", 0, "1234\n");
    fake.runner
        .respond("zpool import -N -o readonly=off 1234", 1, "");

    assert!(set_rw(&fake, "tank", ImportPolicy::default()).is_err());

    // Named by its name, `zpool import` refuses a pool that shares it with
    // one that is not imported; by its guid, it imports it, and imports it
    // read-only again when the read/write import fails.
    let lines = fake.runner.lines();
    assert_eq!(
        lines[lines.len() - 4..],
        [
            "zpool get -H -o value guid tank",
            "zpool export tank",
            "zpool import -N -o readonly=off 1234",
            "zpool import -N -o readonly=on 1234"
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

/// [`reimport`] of `tank` on `fake`, where no one answers.
fn again(fake: &FakeSystem, access: Access, rewind: Rewind) -> Result<(), WritableError> {
    let state = State::new(&fake.files, "/zfsbootmenu");
    let context = Context::new(fake.system());
    let tank = name("tank");
    let import = Import::pool(&tank)
        .policy(ImportPolicy::Strict)
        .access(access)
        .rewind(rewind);
    reimport(&context, &NoInteraction, &state, &tank, import)
}

#[test]
fn a_writable_pool_is_imported_read_only_again_without_a_question() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu").file(BCLONE, "1\n");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");

    again(&fake, Access::ReadOnly, Rewind::No).unwrap();

    assert_eq!(
        fake.runner.lines(),
        [
            "zpool get -H -o value readonly tank",
            "zpool get -H -o value guid tank",
            "zpool export tank",
            "zpool import -N -o readonly=on tank"
        ]
    );
    // Only a read/write import turns block cloning off.
    assert_eq!(fake.files.contents(BCLONE).as_deref(), Some(&b"1\n"[..]));

    let (read_only, _log) = machine("-");
    again(&read_only, Access::ReadOnly, Rewind::No).unwrap();
    assert_eq!(
        read_only.runner.lines(),
        ["zpool get -H -o value readonly tank"]
    );
}

#[test]
fn a_rewind_exports_a_writable_pool_and_asks_what_read_write_asks() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");
    fake.runner
        .respond(&format!("zpool get -H -o value {READONLY} tank"), 0, "-\n");
    fake.runner.respond_program("blkid", 2, "");

    again(&fake, Access::ReadWrite, Rewind::ToCheckpoint).unwrap();

    let lines = fake.runner.lines();
    assert!(lines.contains(&format!("zpool get -H -o value {READONLY} tank")));
    assert_eq!(
        lines[lines.len() - 2..],
        [
            "zpool export tank",
            "zpool import -N -o readonly=off --rewind-to-checkpoint tank"
        ]
    );

    let (forbidden, _log) = machine("on");
    assert!(matches!(
        again(&forbidden, Access::ReadWrite, Rewind::ToCheckpoint),
        Err(WritableError::ReadWriteProhibited { .. })
    ));
    assert!(
        !forbidden
            .runner
            .lines()
            .iter()
            .any(|line| line.contains("export"))
    );
}

#[test]
fn a_failed_rewind_imports_the_pool_read_only_without_rewinding() {
    let (fake, _log) = machine("-");
    fake.runner.respond(
        "zpool import -N -o readonly=off --rewind-to-checkpoint tank",
        1,
        "",
    );

    let failed = again(&fake, Access::ReadWrite, Rewind::ToCheckpoint);

    assert!(matches!(
        failed,
        Err(WritableError::Pool(PoolError::Import { .. }))
    ));
    let lines = fake.runner.lines();
    assert_eq!(
        lines[lines.len() - 2..],
        [
            "zpool import -N -o readonly=off --rewind-to-checkpoint tank",
            "zpool import -N -o readonly=on tank"
        ]
    );

    // A pool that does not import again either is said to have failed.
    let (fake, _log) = machine("-");
    fake.runner.respond("zpool export tank", 0, "");
    fake.runner.respond_program("zpool", 1, "");
    assert!(again(&fake, Access::ReadWrite, Rewind::ToCheckpoint).is_err());
    assert!(fake.kmsg().contains("tank could not be imported again"));
}
