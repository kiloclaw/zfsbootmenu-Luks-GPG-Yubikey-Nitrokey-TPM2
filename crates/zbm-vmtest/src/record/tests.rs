use pretty_assertions::assert_eq;

use super::*;

/// Settings with a fresh work directory.
fn settings(name: &str) -> Settings {
    let work = std::env::temp_dir().join(format!("zbm-vmtest-{name}-{}", std::process::id()));
    fs::remove_dir_all(&work).ok();
    let mut settings = Settings::from_vars(|_| None, false, false);
    settings.work = work;
    settings
}

#[test]
fn a_test_records_its_checks_and_writes_its_result() {
    let settings = settings("record");
    let test = Test::new(&settings, "layouts-main").unwrap();

    test.check(true, "zpool create tsingle");
    test.check(false, "ZFSBootMenu imported tank: None");
    test.note("a note");
    test.show("one\n\n  two\n");
    let outcome = test.finish();

    assert_eq!(outcome.passed, 1);
    assert_eq!(
        outcome.failures,
        vec!["ZFSBootMenu imported tank: None".to_owned()]
    );
    let dir = settings.work.join("tests/layouts-main");
    let written = fs::read_to_string(dir.join("result.json")).unwrap();
    assert_eq!(Outcome::from_json(&written), Some(outcome));
    let steps = fs::read_to_string(dir.join("steps.log")).unwrap();
    let steps: Vec<&str> = steps.lines().map(|line| &line[11..]).collect();
    assert_eq!(
        steps[1..],
        [
            "ok   - zpool create tsingle",
            "FAIL - ZFSBootMenu imported tank: None",
            "note - a note",
            "      one",
            "        two",
            "FAIL (1): ['ZFSBootMenu imported tank: None']",
        ]
    );
    fs::remove_dir_all(settings.work).ok();
}

#[test]
fn tests_of_rust_images_are_named_apart_unless_they_choose_their_runtime() {
    let mut settings = settings("names");
    settings.runtime = ZbmRuntime::Rust;

    assert_eq!(
        Test::new(&settings, "keys-plugin").unwrap().name(),
        "keys-plugin-rust"
    );
    assert_eq!(
        Test::new(&settings, "runtime-legacy").unwrap().name(),
        "runtime-legacy"
    );
    fs::remove_dir_all(settings.work).ok();
}

#[test]
fn shadow_mode_reports_are_checked_at_the_end() {
    let settings = settings("shadow");
    let test = Test::new(&settings, "complex-healthy").unwrap();
    let console = "ZBM-SHADOW: 0 differences in 12 decisions (import, unlock)\r\n\
                   ZBM-SHADOW: unlock: legacy opened 2, Rust 1\r\n\
                   ZBM-SHADOW: 1 differences in 3 decisions (unlock, kcl)\r\n";
    fs::write(test.dir().join("serial-boot.log"), console).unwrap();

    let outcome = test.finish();

    assert_eq!(
        outcome.failures,
        vec!["shadow mode: 1 differences in 2 boots (compared: import, unlock, kcl)".to_owned()]
    );
    fs::remove_dir_all(settings.work).ok();
}

#[test]
fn a_test_without_shadow_mode_reports_has_no_shadow_check() {
    let settings = settings("noshadow");
    let test = Test::new(&settings, "runtime-rust").unwrap();
    fs::write(
        test.dir().join("serial-boot.log"),
        "Booting tplain/ROOT/be\n",
    )
    .unwrap();

    assert_eq!(test.finish().passed, 0);
    fs::remove_dir_all(settings.work).ok();
}

#[test]
fn a_shadow_boot_that_reached_the_boot_environment_needs_a_summary() {
    let settings = settings("shadow-missing");
    let test = Test::new(&settings, "complex-healthy").unwrap();
    test.expect_shadow("boot");
    test.expect_shadow("setup");
    fs::write(test.dir().join("serial-boot.log"), "BE-INIT-START\r\n").unwrap();
    // A setup that never left the recovery shell prints no summary.
    fs::write(test.dir().join("serial-setup.log"), "zbm# \r\n").unwrap();

    let outcome = test.finish();

    assert_eq!(
        outcome.failures,
        ["shadow mode: the boot boot reached the boot environment without a shadow summary"]
    );
    fs::remove_dir_all(settings.work).ok();
}

#[test]
fn a_shadow_summary_that_compared_nothing_fails() {
    let settings = settings("shadow-nothing");
    let test = Test::new(&settings, "complex-healthy").unwrap();
    let console = "ZBM-SHADOW: 0 differences in 0 decisions\r\nBE-INIT-START\r\n";
    fs::write(test.dir().join("serial-boot.log"), console).unwrap();

    let outcome = test.finish();

    assert_eq!(
        outcome.failures,
        ["shadow mode: 0 differences in 1 boots (compared: nothing)"]
    );
    fs::remove_dir_all(settings.work).ok();
}

#[test]
fn a_test_that_did_not_run_is_skipped_and_one_that_checked_nothing_fails() {
    let settings = settings("skip");
    let test = Test::new(&settings, "layouts-parity-oldzfs").unwrap();
    test.skip("no old image");
    let outcome = test.finish();
    assert_eq!((outcome.verdict(), outcome.passed), ("SKIP", 0));
    let written = fs::read_to_string(test.dir().join("result.json")).unwrap();
    assert_eq!(Outcome::from_json(&written), Some(outcome));
    let steps = fs::read_to_string(test.dir().join("steps.log")).unwrap();
    assert!(steps.ends_with("SKIP: no old image\n"), "{steps}");

    let test = Test::new(&settings, "layouts-main").unwrap();
    test.note("nothing to see");
    let outcome = test.finish();
    assert_eq!(outcome.verdict(), "FAIL");
    assert_eq!(outcome.failures, ["the test made a check"]);
    fs::remove_dir_all(settings.work).ok();
}

#[test]
fn shadow_lines_other_than_summaries_are_shown_once() {
    let text = "ZBM-SHADOW: a differs\nZBM-SHADOW: a differs\n\
                ZBM-SHADOW: 0 differences in 1 decisions\n"
        .to_owned();

    let summary = summarize_shadow(&[text]);

    assert_eq!(summary.lines, vec!["ZBM-SHADOW: a differs".to_owned()]);
    assert_eq!((summary.boots, summary.decisions.len()), (1, 0));
}

#[test]
fn utc_offsets_are_read_from_date() {
    assert_eq!(parse_offset("+0200"), Some(7200));
    assert_eq!(parse_offset("-0530"), Some(-19800));
    assert_eq!(parse_offset("Z"), None);
    assert_eq!(clock().len(), 8);
}

#[test]
fn a_result_without_its_fields_is_not_read() {
    assert_eq!(Outcome::from_json("{}"), None);
    assert_eq!(Outcome::from_json("not json"), None);
}
