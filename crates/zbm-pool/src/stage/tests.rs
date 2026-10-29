//! The stage against fake `zpool` answers and a fake operator. The parity
//! tests of `zbm-boot` run it against the legacy stage too.

use zbm_testkit::Shared;

use pretty_assertions::assert_eq;
use zbm_platform::exec::{Output, Status};
use zbm_testkit::fakes::FakeSystem;
use zbm_testkit::legacy::stub_lines;
use zbm_types::ImportPolicy;

use super::*;
use crate::import::tests::log;

/// An operator that records what the stage asked, and answers from lists.
#[derive(Default)]
struct FakeOperator {
    /// The calls of `unlock_late` that open a volume, from 1.
    opens: Vec<usize>,
    unlocks: Shared<usize>,
    /// The answers to the retry prompts, in order; then RETURN.
    answers: Shared<Vec<Answer>>,
    prompts: Shared<Vec<String>>,
    warnings: Shared<Vec<String>>,
    shells: Shared<Vec<String>>,
}

impl Operator for FakeOperator {
    fn unlock_late(&self) -> Late {
        *self.unlocks.borrow_mut() += 1;
        if self.opens.contains(&self.unlocks.get()) {
            Late::Unlocked
        } else {
            Late::Nothing
        }
    }

    fn retry(&self, target: Option<&PoolName>, delay: RetryDelay) -> Answer {
        let target = target.map_or("pool", PoolName::as_str);
        self.prompts
            .borrow_mut()
            .push(format!("{target} {}", delay.seconds()));
        let mut answers = self.answers.borrow_mut();
        if answers.is_empty() {
            Answer::Continue
        } else {
            answers.remove(0)
        }
    }

    fn unsupported_features(&self, advice: &str) {
        self.warnings.borrow_mut().push(advice.to_owned());
    }

    fn emergency_shell(&self, message: &str) {
        self.shells.borrow_mut().push(message.to_owned());
    }
}

/// `zpool list -H -o name` listing `pool` from its `after`th call on.
fn pool_after(fake: &FakeSystem, pool: &'static str, after: usize) {
    let lists = Shared::new(0);
    fake.runner.on(move |command| {
        if command.display() != "zpool list -H -o name" {
            return None;
        }
        lists.set(lists.get() + 1);
        let pools = if lists.get() >= after { pool } else { "" };
        Some(Ok(Output::new(Status::exited(0), pools)))
    });
}

fn info_lines(fake: &FakeSystem) -> Vec<String> {
    stub_lines(&fake.kmsg())
        .lines()
        .filter(|line| line.starts_with("INFO"))
        .map(str::to_owned)
        .collect()
}

fn strict() -> ImportSettings {
    ImportSettings {
        policy: ImportPolicy::Strict,
        ..ImportSettings::default()
    }
}

#[test]
fn late_luks_volumes_are_unlocked_before_retrying() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    pool_after(&fake, "tank\n", 3);
    let state = State::new(&fake.files, "/zfsbootmenu");
    // The first failure finds a volume at once (call 1); the retry finds none
    // before importing (2), nor does the second failure (3), so the prompt
    // comes; the retry after it finds one before importing (4).
    let operator = FakeOperator {
        opens: vec![1, 4],
        ..FakeOperator::default()
    };
    stage(&fake.context(), &state, &strict(), &operator);
    assert_eq!(operator.unlocks.get(), 4);
    assert_eq!(operator.prompts.get(), ["pool 5"]);
    assert_eq!(
        info_lines(&fake),
        [
            "INFO: unable to import a pool on attempt 1",
            "INFO: unlocked LUKS volumes that appeared late, retrying import",
            "INFO: unable to import a pool on attempt 2",
            "INFO: unlocked LUKS volumes that appeared late",
        ]
    );
}

#[test]
fn escape_opens_the_recovery_shell_after_logging_unavailable_pools() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    pool_after(&fake, "tank\n", 2);
    let state = State::new(&fake.files, "/zfsbootmenu");
    let operator = FakeOperator {
        answers: Shared::new(vec![Answer::Escape]),
        ..FakeOperator::default()
    };
    let settings = ImportSettings {
        prefer_pool: "tank".parse::<PoolName>().ok(),
        requirement: PoolRequirement::Only,
        retry_delay: RetryDelay::or_default("2"),
        ..strict()
    };
    stage(&fake.context(), &state, &settings, &operator);
    assert_eq!(operator.prompts.get(), ["tank 2"]);
    assert_eq!(
        operator.shells.get(),
        ["unable to successfully import a pool"]
    );
    assert!(fake.runner.lines().contains(&"zpool import".to_owned()));
}

#[test]
fn host_id_matching_records_the_host_id() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.files.dir("/etc");
    let imported = Shared::new(false);
    let seen = imported.clone();
    fake.runner.on(move |command| {
        let line = command.display();
        let (status, output) = match line.as_str() {
            "zpool list -H -o name" if seen.get() => (0, "tank\n"),
            "zpool list -H -o name" => (0, ""),
            "zpool import" => (0, "   pool: tank\n  state: ONLINE\n"),
            "zpool import -o readonly=on -N tank" => (1, "(hostid=deadbeef)\n"),
            "zpool import -N -o readonly=on tank" => {
                seen.set(true);
                (0, "")
            }
            _ => return None,
        };
        Some(Ok(Output::new(Status::exited(status), output)))
    });
    let state = State::new(&fake.files, "/zfsbootmenu");
    let settings = ImportSettings {
        policy: ImportPolicy::Hostid,
        ..ImportSettings::default()
    };
    let imported = stage(&fake.context(), &state, &settings, &FakeOperator::default());
    assert_eq!(
        imported.spl_hostid.as_ref().map(HostId::as_str),
        Some("deadbeef")
    );
    assert_eq!(state.spl_hostid().as_deref(), Some("deadbeef"));
    assert!(stub_lines(&fake.kmsg()).contains("DEBUG: match_hostid returned: tank;deadbeef"));
}

#[test]
fn unhealthy_and_unsupported_pools_are_restricted() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.runner
        .respond("zpool list -H -o name", 0, "data\ntank\n");
    fake.runner.respond(
        "zpool list -H -o name,health",
        0,
        "data\tDEGRADED\ntank\tONLINE\n",
    );
    fake.runner.respond(
        "zpool get all -H -o name,property",
        0,
        "data\tsize\ntank\tunsupported@org.x:y\ndata\tunsupported@org.x:z\n",
    );
    fake.runner
        .respond("zpool list -H -o bootfs --", 0, "-\ntank/ROOT/a\n");
    let state = State::new(&fake.files, "/zfsbootmenu");
    for build in [Build::Local, Build::Release] {
        let settings = ImportSettings { build, ..strict() };
        let operator = FakeOperator::default();
        let imported = stage(&fake.context(), &state, &settings, &operator);
        assert_eq!(imported.bootfs.unwrap().as_str(), "tank/ROOT/a");
        assert_eq!(operator.warnings.borrow().len(), 1);
    }
    assert!(state.is_degraded("data"));
    assert!(state.is_degraded("tank"));
    assert_eq!(state.bootfs().as_deref(), Some("tank/ROOT/a"));
    let log = stub_lines(&fake.kmsg());
    assert!(log.contains("ERROR: prohibiting read/write operations on data"));
    assert!(log.contains("Upgrade ZFS modules in ZFSBootMenu with generate-zbm"));
    assert!(log.contains("Check for a new binary release of ZFSBootMenu"));
}

#[test]
fn the_boot_environment_is_preferred_then_taken_from_the_pools() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.runner
        .respond("zfs list -H -o name -t filesystem tank/ROOT/b", 0, "");
    fake.runner.respond_program("zfs", 1, "");
    fake.runner
        .respond("zpool list -H -o bootfs tank", 0, "-\n");
    fake.runner.respond("zpool list -H -o bootfs --", 0, "");
    fake.runner
        .respond("zpool list -H -o bootfs other", 0, "tank/ROOT@snap\n");
    let runner = fake.system().runner;
    let choose = |prefer_pool: &str, prefer_bootfs: &str| {
        let settings = ImportSettings {
            prefer_pool: prefer_pool.parse::<PoolName>().ok(),
            prefer_bootfs: BootfsPreference::from_conf(prefer_bootfs),
            ..ImportSettings::default()
        };
        choose_bootfs(runner, &settings).map(|dataset| dataset.to_string())
    };
    assert_eq!(choose("", "tank/ROOT/b").as_deref(), Some("tank/ROOT/b"));
    assert_eq!(choose("", "tank/ROOT/none"), None);
    assert_eq!(choose("", "tank@snap"), None);
    assert_eq!(choose("tank", ""), None);
    assert_eq!(choose("", ""), None);
    // A `bootfs` that names no dataset is refused.
    assert_eq!(choose("other", ""), None);
    let log = stub_lines(&fake.kmsg());
    assert!(log.contains("ERROR: bootfs value 'tank/ROOT/none' not a ZFS dataset"));
    assert!(log.contains("ERROR: bootfs value 'tank@snap' not a ZFS dataset"));
}
