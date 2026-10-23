use std::fs;

use pretty_assertions::assert_eq;

use super::devices::{links, table};
use super::session::{Run, count, first_line, substitute};
use super::upgrade::{has_all_new, new_states, parse_features, state};
use super::verify::check_boot;
use crate::boot::Boot;
use crate::record::Test;
use crate::testing::{Step, checks, cleanup, scripted, settings};

/// The lines of `steps.log` after the test's first, without the time.
fn steps(test: &Test) -> Vec<String> {
    let steps = fs::read_to_string(test.dir().join("steps.log")).unwrap();
    steps
        .lines()
        .skip(1)
        .map(|line| line.get(11..).unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn the_disks_are_those_of_the_documentation_in_order() {
    let table = table();

    let serials: Vec<&str> = table.iter().map(|device| device.serial.as_str()).collect();
    assert_eq!(
        serials,
        [
            "w1", "w2", "w3", "w4", "w5", "w6", "w7", "x1", "x2", "x3", "x4", "x5", "x6", "x7",
            "f1", "f2", "f3", "n1", "s1", "s2"
        ]
    );
    let big: Vec<&str> = table
        .iter()
        .filter(|device| device.size() == "768M")
        .map(|device| device.name.as_str())
        .collect();
    assert_eq!(big, ["nvme-FAST1", "nvme-FAST2", "nvme-FAST3"]);
    assert_eq!(table[7].parts, None);
    assert_eq!(
        table[19].parts,
        Some(&[("esp", Some("64M")), ("zfs", Some("300M"))][..])
    );
}

#[test]
fn placeholder_names_link_to_the_virtual_disks_and_their_partitions() {
    let links = links(&table());

    assert_eq!(links.len(), 7 * 2 + 7 + 3 * 5 + 2 + 2 * 2);
    assert_eq!(
        links[0],
        "ln -sf /dev/disk/by-id/virtio-w1-part1 /dev/disk/by-id/ata-DISK1-part1"
    );
    assert_eq!(
        links[14],
        "ln -sf /dev/disk/by-id/virtio-x1 /dev/disk/by-id/ata-DISK1"
    );
    assert_eq!(
        links[25],
        "ln -sf /dev/disk/by-id/virtio-f1-part5 /dev/disk/by-id/nvme-FAST1-part5"
    );
    assert_eq!(
        links[37],
        "ln -sf /dev/disk/by-id/virtio-n1-part2 /dev/disk/by-id/ata-NEWDISK-part2"
    );
    assert_eq!(
        links[41],
        "ln -sf /dev/disk/by-id/virtio-s2-part2 /dev/disk/by-id/nvme-SSD2-part2"
    );
}

#[test]
fn messages_show_the_first_line_of_a_command_shortened() {
    let command = format!("{}\nsecond", "x".repeat(120));

    let first = first_line(&command, 100);

    assert_eq!(first, "x".repeat(100));
    assert_eq!(first_line("", 90), "");
    assert_eq!(first_line("zpool status\n-v", 90), "zpool status");
}

#[test]
fn substitutions_apply_in_order() {
    let subst = [
        ("/dev/mapper/luks-9a72f853-...", "/dev/mapper/luks-1"),
        ("luks-9a72f853-...", "luks-1"),
    ];

    let command = substitute(
        "zpool remove tank /dev/mapper/luks-9a72f853-... && cryptsetup close luks-9a72f853-...",
        &subst,
    );

    assert_eq!(
        command,
        "zpool remove tank /dev/mapper/luks-1 && cryptsetup close luks-1"
    );
}

#[test]
fn matches_are_counted_line_by_line() {
    let text = "/dev/mapper/luks-a ONLINE\nx /dev/mapper/luks-b ONLINE\n/dev/mapper/luks-c ONLINE";

    let anchored = count(r"(?m)^/dev/mapper/luks-\S+ ONLINE$", text).unwrap();
    let anywhere = count(r"/dev/mapper/luks-\S+\s+ONLINE", text).unwrap();

    assert_eq!((anchored, anywhere), (2, 3));
    assert!(count("(", text).is_err());
}

#[test]
fn feature_states_are_read_from_zpool_get() {
    let out = "feature@async_destroy\tenabled\nfeature@physical_rewrite\tdisabled\n\
               feature@block_cloning_endian disabled\nfeature@dynamic_gang_header\n\
               x feature@fast_dedup enabled";

    let features = parse_features(out);

    assert_eq!(state(&features, "async_destroy"), Some("enabled"));
    assert_eq!(state(&features, "fast_dedup"), None);
    assert_eq!(new_states(&features), "['disabled', 'disabled', None]");
    assert!(!has_all_new(&features, "disabled"));
}

#[test]
fn every_new_feature_in_a_state_is_reported() {
    let features = parse_features(
        "feature@block_cloning_endian enabled\nfeature@physical_rewrite enabled\n\
         feature@dynamic_gang_header enabled",
    );

    assert!(has_all_new(&features, "enabled"));
    assert_eq!(new_states(&features), "['enabled', 'enabled', 'enabled']");
}

#[test]
fn a_section_runs_until_a_command_fails() {
    let settings = settings("procedures-doc");
    let docs = settings.work.join("docs");
    fs::create_dir_all(&docs).unwrap();
    fs::write(
        docs.join("p.rst"),
        "Sect\n====\n.. code-block:: bash\n\n  # echo A\n  # skip me\n  # false A\n  # never\n",
    )
    .unwrap();
    let test = Test::new(&settings, "procedures").unwrap();
    let script = vec![
        Step::Hear("echo B; echo RC=$?"),
        Step::Say("B\r\nRC=0\r\n"),
        Step::Hear("false B; echo RC=$?"),
        Step::Say("oops\r\nRC=1\r\n"),
    ];
    let (vm, guest) = scripted(&test, script);
    let mut run = Run::new(vm, &docs);

    let out = run
        .doc_with("p.rst", "Sect", &["^skip"], &[("A", "B")])
        .unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(out, "B\noops");
    assert_eq!(
        steps(&test),
        [
            "--- p.rst: Sect (4 commands)",
            "$ echo B -> 0",
            "  (not run here: skip me)",
            "$ false B -> 1",
            "      oops",
            "FAIL - Sect: 'false B' exited 1",
        ]
    );
    cleanup(&settings);
}

#[test]
fn a_section_whose_commands_all_succeed_passes_and_the_pool_is_reset() {
    let settings = settings("procedures-reset");
    let docs = settings.work.join("docs");
    fs::create_dir_all(&docs).unwrap();
    fs::write(
        docs.join("p.rst"),
        "Sect\n====\n.. code-block:: bash\n\n  X=1\n",
    )
    .unwrap();
    let test = Test::new(&settings, "procedures").unwrap();
    let script = vec![
        Step::Hear("X=1; echo RC=$?"),
        Step::Say("RC=0\r\n"),
        Step::Hear("zpool status -P tank; echo RC=$?"),
        Step::Say("  pool: tank\r\nRC=0\r\n"),
        Step::Hear("zpool destroy tank; echo RC=$?"),
        Step::Say("RC=0\r\n"),
        Step::Hear(
            "for m in /dev/mapper/luks-*; do [ -e \"$m\" ] && cryptsetup close \"${m##*/}\"; \
             done; true; echo RC=$?",
        ),
        Step::Say("RC=0\r\n"),
    ];
    let (vm, guest) = scripted(&test, script);
    let mut run = Run::new(vm, &docs);

    run.doc("p.rst", "Sect").unwrap();
    let status = run.status("tank").unwrap();
    run.reset(Some("tank")).unwrap();
    let missing = run.doc("p.rst", "Other").unwrap_err().to_string();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(status, "  pool: tank");
    assert_eq!(checks(&test), ["ok   - Sect: all commands succeeded"]);
    assert!(missing.starts_with("no section 'Other' in "), "{missing}");
    cleanup(&settings);
}

#[test]
fn step_4_checks_what_zfsbootmenu_shows() {
    let settings = settings("procedures-verify");
    let test = Test::new(&settings, "procedures").unwrap();
    let healthy = Boot {
        prompts: vec!["luks".into()],
        zbm_status: "  pool: zroot\n state: ONLINE\n\tcache\n\t  /dev/mapper/luks-a  ONLINE\n\
                     \tspares\n\t  /dev/mapper/luks-b  AVAIL"
            .into(),
        zluks: "luks-x open zroot\n".repeat(11),
        teardown: Some(Default::default()),
        ..Boot::default()
    };
    let broken = Boot {
        prompts: vec!["luks".into(), "luks".into()],
        zbm_status: "  pool: zroot\n state: DEGRADED\n  /dev/mapper/luks-c  UNAVAIL".into(),
        ..Boot::default()
    };

    check_boot(&test, &healthy).unwrap();
    check_boot(&test, &broken).unwrap();

    assert_eq!(
        checks(&test),
        [
            "ok   - one passphrase prompt for the 11 volumes (['luks'])",
            "ok   - Step 4: zroot ONLINE",
            "ok   - Step 4: cache ONLINE",
            "ok   - Step 4: spare AVAIL",
            "ok   - Step 4: nothing UNAVAIL",
            "ok   - Step 4: zluks status maps the 11 volumes to zroot",
            "ok   - teardown exported pools and closed volumes (dm=[] pools=[] state=[])",
            "FAIL - one passphrase prompt for the 11 volumes (['luks', 'luks'])",
            "FAIL - Step 4: zroot ONLINE",
            "FAIL - Step 4: cache ONLINE",
            "FAIL - Step 4: spare AVAIL",
            "FAIL - Step 4: nothing UNAVAIL",
            "FAIL - Step 4: zluks status maps the 11 volumes to zroot",
            "FAIL - teardown report missing",
        ]
    );
    cleanup(&settings);
}
