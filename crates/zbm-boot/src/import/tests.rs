//! The import stage of the start-up program against the legacy stage: both
//! run on the same fake machine (see [`machine`]), and must leave the same
//! log, commands, files and prompts behind.

use zbm_pool::hostid::scrape_hostid;
use zbm_testkit::legacy::Bash;
use zbm_testkit::parity;
use zbm_types::ImportPolicy;

mod machine;

/// Recovery whose late unlocking opens a volume once.
struct OpensOnce(std::sync::atomic::AtomicBool);

impl crate::recovery::Recovery for OpensOnce {
    fn unlock_late(&self) -> Result<(), crate::error::BootError> {
        if self.0.swap(false, std::sync::atomic::Ordering::Relaxed) {
            Ok(())
        } else {
            Err(crate::error::BootError::NoCryptsetup)
        }
    }

    fn emergency_shell(&self, _message: &str) {}
}

#[test]
fn the_operator_reports_late_volumes() {
    use zbm_pool::{Late, Operator};
    let fake = zbm_testkit::fakes::FakeSystem::new();
    let system = fake.system();
    let recovery = OpensOnce(std::sync::atomic::AtomicBool::new(true));
    let prompts = super::Prompts::new(&system, &recovery);
    assert_eq!(prompts.unlock_late(), Late::Unlocked);
    assert_eq!(prompts.unlock_late(), Late::Nothing);
}

use machine::{Scenario, Settings};

#[test]
fn host_ids_are_scraped_as_the_legacy_pattern_matches() {
    let text = "Last accessed by host (hostid=deadbeef1) at ...";
    assert_eq!(scrape_hostid(text), Some("deadbeef"));
    assert_eq!(scrape_hostid("hostid=x hostid=A1"), Some("A1"));
    assert_eq!(scrape_hostid("hostid="), None);
    assert_eq!(scrape_hostid("myhostid=0b"), Some("0b"));
    // Found by the fuzz target program_output: text that is not ASCII
    // before the match.
    assert_eq!(scrape_hostid("tan\u{fffd}\tDEGRADED"), None);
    assert_eq!(scrape_hostid("pool «tank» hostid=0c"), Some("0c"));
    let mut parity = parity!("host_ids_are_scraped_as_the_legacy_pattern_matches");
    for text in [
        "hostid=12345678",
        "(hostid=a) x",
        "hostid=hostid=7",
        "hostid=g1",
    ] {
        let legacy: String = parity.legacy(text, || {
            Bash::new()
                .code(concat!(
                    r#"m() { [[ $1 =~ hostid=([A-Fa-f0-9]{1,8}) ]] "#,
                    r#"&& echo "${BASH_REMATCH[1]}"; }"#
                ))
                .call("m", &[text], b"")
                .stdout_text()
        });
        let expected = legacy.trim_end_matches('\n').to_owned();
        assert_eq!(scrape_hostid(text).unwrap_or_default(), expected, "{text}");
    }
}

fn hostid_settings() -> Settings {
    Settings {
        import_policy: ImportPolicy::Hostid,
        ..Settings::default()
    }
}

#[test]
fn imports_every_pool_and_finds_bootfs() {
    let mut parity = parity!("imports_every_pool_and_finds_bootfs");
    let outcome = Scenario {
        pools: vec![
            ("boot", "id=1\nbootfs=-"),
            ("tank", "id=2\nbootfs=tank/ROOT/void"),
        ],
        settings: hostid_settings(),
        ..Scenario::default()
    }
    .check(&mut parity);
    assert_eq!(outcome.imported, "boot\ntank\n");
    assert_eq!(outcome.bootfs, "tank/ROOT/void");
    assert_eq!(outcome.bootfs_file.as_deref(), Some("tank/ROOT/void\n"));
    assert_eq!(outcome.degraded, None);
}

#[test]
fn matches_the_host_id_of_a_foreign_pool() {
    let mut parity = parity!("matches_the_host_id_of_a_foreign_pool");
    let outcome = Scenario {
        pools: vec![("tank", "id=7\nhostid=deadbeef\nbootfs=tank/ROOT/a")],
        hostid: Some([0x0c, 0xb1, 0xba, 0x00]),
        settings: hostid_settings(),
        ..Scenario::default()
    }
    .check(&mut parity);
    assert_eq!(outcome.imported, "tank\n");
    assert_eq!(outcome.spl_hostid_file.as_deref(), Some("deadbeef"));
    assert_eq!(outcome.etc_hostid, Some(vec![0xef, 0xbe, 0xad, 0xde]));
    assert!(
        outcome
            .log
            .contains("WARN: imported tank with assumed hostid deadbeef")
    );

    // A release build does not warn; a preferred pool is matched alone.
    let mut settings = hostid_settings();
    settings.release_build = true;
    settings.prefer_pool = "tank".to_owned();
    let outcome = Scenario {
        pools: vec![
            ("tank", "id=7\nhostid=deadbeef\nbootfs=tank/ROOT/a"),
            ("other", "id=8\nhostid=deadbeef"),
        ],
        hostid: Some([0x0c, 0xb1, 0xba, 0x00]),
        settings,
        ..Scenario::default()
    }
    .check(&mut parity);
    assert_eq!(outcome.imported, "tank\nother\n");
    assert!(!outcome.log.contains("WARN:"));

    // A required pool alone: the other pool stays where it is.
    let mut settings = hostid_settings();
    settings.prefer_pool = "tank".to_owned();
    settings.require_pool = "only".to_owned();
    let outcome = Scenario {
        pools: vec![
            ("tank", "id=7\nhostid=deadbeef\nbootfs=tank/ROOT/a"),
            ("other", "id=8"),
        ],
        hostid: Some([0x0c, 0xb1, 0xba, 0x00]),
        settings,
        ..Scenario::default()
    }
    .check(&mut parity);
    assert_eq!(outcome.imported, "tank\n");
}

#[test]
fn host_id_cases() {
    let mut parity = parity!("host_id_cases");
    // Two pools of different foreign hosts: the first match decides the host
    // ID, and only pools of that host follow.
    let outcome = Scenario {
        pools: vec![
            ("alpha", "id=1\nhostid=0000abcd\nbootfs=alpha/ROOT/a"),
            ("beta", "id=2\nhostid=deadbeef"),
            ("gamma", "id=3\nhostid=0000abcd"),
        ],
        hostid: Some([0x0c, 0xb1, 0xba, 0x00]),
        settings: hostid_settings(),
        ..Scenario::default()
    }
    .check(&mut parity);
    assert_eq!(outcome.imported, "alpha\ngamma\n");
    assert_eq!(outcome.spl_hostid_file.as_deref(), Some("0000abcd"));

    // The pool's host ID is this machine's: no matching.
    let outcome = Scenario {
        pools: vec![("tank", "id=7\nhostid=00bab10c")],
        hostid: Some([0x0c, 0xb1, 0xba, 0x00]),
        settings: hostid_settings(),
        ..Scenario::default()
    }
    .check(&mut parity);
    assert_eq!(outcome.spl_hostid_file, None);

    // Without /etc/hostid, a pool with a host ID is matched too.
    let outcome = Scenario {
        pools: vec![("tank", "id=7\nhostid=00bab10c")],
        settings: hostid_settings(),
        ..Scenario::default()
    }
    .check(&mut parity);
    assert_eq!(outcome.etc_hostid, Some(vec![0x0c, 0xb1, 0xba, 0x00]));
}

#[test]
fn a_required_pool_that_never_appears() {
    let mut parity = parity!("a_required_pool_that_never_appears");
    for policy in [ImportPolicy::Strict, ImportPolicy::Hostid] {
        let mut settings = hostid_settings();
        settings.import_policy = policy;
        settings.prefer_pool = "missing".to_owned();
        settings.require_pool = "only".to_owned();
        let outcome = Scenario {
            pools: vec![("other", "id=1\nbootfs=other/ROOT/a")],
            settings,
            escapes: vec![false, true],
            on_shell: "printf 'id=9\\n' > pools/missing\n",
            ..Scenario::default()
        }
        .check(&mut parity);
        assert_eq!(outcome.prompts, 2, "{policy}");
        assert_eq!(outcome.imported, "missing\n", "{policy}");
    }
}

#[test]
fn strict_and_forced_policies() {
    let mut parity = parity!("strict_and_forced_policies");
    let mut settings = hostid_settings();
    settings.import_policy = ImportPolicy::Force;
    let outcome = Scenario {
        pools: vec![("tank", "id=7\nhostid=deadbeef")],
        settings,
        ..Scenario::default()
    }
    .check(&mut parity);
    assert_eq!(outcome.imported, "tank\n");
    assert_eq!(outcome.spl_hostid_file, None);

    // Strict: no host ID matching; ESCAPE opens the recovery shell, which
    // fixes the pool.
    let mut settings = hostid_settings();
    settings.import_policy = ImportPolicy::Strict;
    let outcome = Scenario {
        pools: vec![("tank", "id=7\nhostid=deadbeef")],
        hostid: Some([0x0c, 0xb1, 0xba, 0x00]),
        settings,
        escapes: vec![false, true],
        on_shell: "sed -i '/^hostid=/d' pools/tank\n",
        ..Scenario::default()
    }
    .check(&mut parity);
    assert_eq!(outcome.prompts, 2);
    assert_eq!(outcome.shells, "unable to successfully import a pool\n");
    assert_eq!(outcome.imported, "tank\n");
}

#[test]
fn retries_until_late_pools_appear() {
    let mut parity = parity!("retries_until_late_pools_appear");
    let outcome = Scenario {
        pools: vec![("late", "id=9\nappear=3\nbootfs=late/ROOT/x")],
        settings: hostid_settings(),
        ..Scenario::default()
    }
    .check(&mut parity);
    assert_eq!(outcome.imported, "late\n");
    assert!(outcome.prompts >= 1);
    assert_eq!(outcome.bootfs, "late/ROOT/x");
}

#[test]
fn preferred_and_required_pools() {
    let mut parity = parity!("preferred_and_required_pools");
    for require in ["", "yes", "only"] {
        let mut settings = hostid_settings();
        settings.prefer_pool = "tank".to_owned();
        settings.require_pool = require.to_owned();
        settings.retry_delay = "2".to_owned();
        Scenario {
            pools: vec![
                ("boot", "id=1\nbootfs=boot/ROOT/b"),
                ("tank", "id=2\nappear=2\nbootfs=-"),
            ],
            settings,
            ..Scenario::default()
        }
        .check(&mut parity);
    }
    // A missing preferred pool that is not required.
    let mut settings = hostid_settings();
    settings.prefer_pool = "missing".to_owned();
    Scenario {
        pools: vec![("boot", "id=1\nbootfs=boot/ROOT/b")],
        settings,
        ..Scenario::default()
    }
    .check(&mut parity);
}

#[test]
fn restricts_unhealthy_and_unsupported_pools() {
    let mut parity = parity!("restricts_unhealthy_and_unsupported_pools");
    for release_build in [false, true] {
        let mut settings = hostid_settings();
        settings.release_build = release_build;
        let outcome = Scenario {
            pools: vec![
                ("data", "id=1\nhealth=DEGRADED"),
                ("data2", "id=2\nunsupported=com.example:new"),
                ("tank", "id=3\nhealth=DEGRADED\nunsupported=org.x:y"),
            ],
            settings,
            ..Scenario::default()
        }
        .check(&mut parity);
        assert_eq!(outcome.degraded.as_deref(), Some("data\ntank\ndata2\n"));
        assert_eq!(outcome.prompts, 1);
    }
}

#[test]
fn unavailable_pools_are_logged_before_the_recovery_shell() {
    let mut parity = parity!("unavailable_pools_are_logged_before_the_recovery_shell");
    let outcome = Scenario {
        pools: vec![("broken", "id=42\nstate=UNAVAIL")],
        settings: hostid_settings(),
        escapes: vec![true],
        on_shell: "printf 'id=43\\nbootfs=-\\n' > pools/fixed\n",
        ..Scenario::default()
    }
    .check(&mut parity);
    assert!(
        outcome.log.contains(
            "ERROR: cannot import 'broken': one or more devices is currently unavailable"
        )
    );
    assert_eq!(outcome.imported, "fixed\n");
    assert_eq!(outcome.bootfs, "");
}

#[test]
fn a_preferred_boot_environment_must_be_a_filesystem() {
    let mut parity = parity!("a_preferred_boot_environment_must_be_a_filesystem");
    for (datasets, prefer) in [("tank/ROOT/a", "tank/ROOT/a"), ("", "tank/ROOT/none")] {
        let mut settings = hostid_settings();
        settings.prefer_pool = "tank".to_owned();
        settings.prefer_bootfs = prefer.to_owned();
        Scenario {
            pools: vec![("tank", "id=1\nbootfs=tank/ROOT/b")],
            datasets,
            settings,
            ..Scenario::default()
        }
        .check(&mut parity);
    }
}
