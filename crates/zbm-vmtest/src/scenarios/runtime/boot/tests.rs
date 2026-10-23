use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{Step, checks, cleanup, scripted, settings};

/// ZFSBootMenu's reports of a boot of `tplain` with the legacy runtime.
const PLAIN: &str = "Booting /boot/vmlinuz-7.3 for tplain/ROOT/be in 3 seconds\r\n\
    ZBMTEST-STATUS-BEGIN\r\nZS|   pool: tplain\r\nZS|  state: ONLINE\r\nZBMTEST-STATUS-END\r\n\
    ZBMTEST-RUNTIME pid1=[/bin/bash /libexec/zfsbootmenu-init ] \
    conf=[ZBM_RUNTIME=\"legacy\"] bootfs=[tplain/ROOT/be]\r\n\
    ZBMTEST-TEARDOWN dm=[] pools=[ tplain ] state=[]\r\n";

/// The boot environment's report.
const BE: &str = "BE-INIT-START\r\nBE-KERNEL: 7.3.1-zbm\r\n\
    BE-ZFS-VERSION: zfs-2.4.4-1 zfs-kmod-2.4.4-1\r\nBE-ROOT: tplain/ROOT/be / zfs rw\r\n\
    BE|   pool: tplain\r\nBE|  state: ONLINE\r\nBE-WRITE: ok\r\n\
    BE-CMDLINE: root=zfs:tplain/ROOT/be\r\nBE-INIT-DONE\r\n";

/// The checks of a boot environment that booted from `tplain`.
const BE_CHECKS: [&str; 6] = [
    "ok   - kexec into the ZFS 2.4.4 boot environment",
    "ok   - boot environment kernel 7.3.1-zbm",
    "ok   - boot environment runs ZFS 2.4.4 (zfs-2.4.4-1 zfs-kmod-2.4.4-1)",
    "ok   - root file system tplain/ROOT/be / zfs rw",
    "ok   - boot environment imported tplain: ONLINE (expected ONLINE)",
    "ok   - root dataset is writable in the boot environment",
];

/// Drives `scenario` with `runtime` against `script`; returns the checks.
fn drive_script(name: &str, runtime: ZbmRuntime, scenario: &str, script: Vec<Step>) -> Vec<String> {
    let settings = settings(&format!("runtime-boot-{name}"));
    let test = Test::new(&settings, name).unwrap();
    let (mut vm, guest) = scripted(&test, script);
    drive(&mut vm, runtime, scenario).unwrap();
    assert_eq!(guest.finish(), Vec::<String>::new());
    let checks = checks(&test);
    cleanup(&settings);
    checks
}

#[test]
fn each_scenario_has_its_disks_and_command_line() {
    let table: Vec<(&str, &[usize], bool, String)> = [
        "", "hostid", "strict", "force", "prefer", "late", "waitfor", "hooks",
    ]
    .into_iter()
    .map(|scenario| {
        let plan = plan(scenario).unwrap();
        (scenario, plan.disks, plan.hooks, plan.append())
    })
    .collect();

    let base = "console=ttyS0 zbm.timeout=3 loglevel=7";
    let expected: Vec<(&str, &[usize], bool, String)> = vec![
        ("", &[0], false, base.to_owned()),
        ("hostid", &[1], false, base.to_owned()),
        (
            "strict",
            &[1],
            false,
            format!("{base} zbm.import_policy=strict zbm.retry_delay=5"),
        ),
        (
            "force",
            &[1],
            false,
            format!("{base} zbm.import_policy=force"),
        ),
        (
            "prefer",
            &[0, 1],
            false,
            format!("{base} zbm.prefer=tplain!!"),
        ),
        ("late", &[], false, format!("{base} zbm.retry_delay=10")),
        (
            "waitfor",
            &[],
            false,
            format!("{base} zbm.wait_for=/dev/disk/by-id/virtio-d0 zbm.retry_delay=10"),
        ),
        (
            "hooks",
            &[0],
            true,
            format!(
                "{base} zbm.hookroot=/dev/disk/by-id/virtio-hooks//zbm \
                 zbm.skip_hooks=20-runtime.sh"
            ),
        ),
    ];
    assert_eq!(table, expected);
    assert!(plan("other").is_err());
}

#[test]
fn the_plain_boot_reports_the_runtime_and_boots_tplain() {
    let script = vec![Step::Say(PLAIN), Step::Say(BE)];

    let checks = drive_script("runtime-legacy", ZbmRuntime::Legacy, "", script);

    let mut expected = vec![
        "ok   - no prompts (prompts: [])",
        "ok   - ZFSBootMenu imported tplain: ONLINE",
        "ok   - the runtime report hook ran",
        "ok   - the image records the legacy runtime",
        "ok   - process 1 is the legacy start-up program",
        "ok   - hooks see BOOTFS (tplain/ROOT/be)",
        "ok   - the countdown boots the default boot environment",
        "ok   - teardown exported pools and closed volumes (dm=[] pools=[tplain] state=[])",
    ];
    expected.extend(BE_CHECKS);
    assert_eq!(checks, expected);
}

#[test]
fn the_rust_runtime_must_run_the_supervisor() {
    let script = vec![Step::Say(PLAIN), Step::Say(BE)];

    let checks = drive_script("runtime-rust", ZbmRuntime::Rust, "", script);

    assert_eq!(
        checks[3..5],
        [
            "FAIL - the image records the rust runtime",
            "FAIL - process 1 is the Rust supervisor",
        ]
    );
}

#[test]
fn the_strict_policy_stops_at_the_retry_prompt() {
    let script = vec![Step::Say(
        "Unable to import tforeign, retrying in 5 seconds\r\n",
    )];

    let checks = drive_script(
        "runtime-legacy-strict",
        ZbmRuntime::Legacy,
        "strict",
        script,
    );

    assert_eq!(
        checks,
        [
            "ok   - the strict policy does not import a pool of another host",
            "ok   - nothing is booted",
        ]
    );
}

#[test]
fn a_strict_boot_that_imports_fails_both_checks() {
    let script = vec![Step::Say(
        "Booting /boot/vmlinuz for tforeign/ROOT/be\r\nZBMTEST-STATUS-BEGIN\r\n",
    )];

    let checks = drive_script("runtime-rust-strict", ZbmRuntime::Rust, "strict", script);

    assert_eq!(
        checks,
        [
            "FAIL - the strict policy does not import a pool of another host",
            "FAIL - nothing is booted",
        ]
    );
}

#[test]
fn host_id_matching_boots_the_pool_of_another_host() {
    let reports = "ZBMTEST-STATUS-BEGIN\r\nZS|   pool: tforeign\r\nZS|  state: ONLINE\r\n\
        ZBMTEST-STATUS-END\r\nZBMTEST-RUNTIME pid1=[/libexec/zbm zfsbootmenu-init --supervise ] \
        conf=[ZBM_RUNTIME=\"rust\"] bootfs=[tforeign/ROOT/be]\r\n\
        ZBMTEST-TEARDOWN dm=[] pools=[tforeign] state=[]\r\n";
    let be = "BE-INIT-START\r\nBE-KERNEL: 7.3.1-zbm\r\n\
        BE-ZFS-VERSION: zfs-2.4.4-1 zfs-kmod-2.4.4-1\r\nBE-ROOT: tforeign/ROOT/be / zfs rw\r\n\
        BE|   pool: tforeign\r\nBE|  state: ONLINE\r\nBE-WRITE: ok\r\n\
        BE-CMDLINE: root=zfs:tforeign/ROOT/be spl_hostid=0xdeadbeef\r\nBE-INIT-DONE\r\n";
    let script = vec![Step::Say(reports), Step::Say(be)];

    let checks = drive_script("runtime-rust-hostid", ZbmRuntime::Rust, "hostid", script);

    assert_eq!(
        checks[..8],
        [
            "ok   - no prompts (prompts: [])",
            "ok   - ZFSBootMenu imported tforeign: ONLINE",
            "ok   - the runtime report hook ran",
            "ok   - the image records the rust runtime",
            "ok   - process 1 is the Rust supervisor",
            "ok   - hooks see BOOTFS (tforeign/ROOT/be)",
            "ok   - teardown exported pools and closed volumes \
             (dm=[] pools=[tforeign] state=[])",
            "ok   - the boot environment gets the matched host ID \
             (root=zfs:tforeign/ROOT/be spl_hostid=0xdeadbeef)",
        ]
    );
    assert!(checks[8..].iter().all(|check| check.starts_with("ok")));
}

#[test]
fn a_forced_import_stops_after_the_teardown() {
    let reports = "ZBMTEST-STATUS-BEGIN\r\nZS|   pool: tforeign\r\nZS|  state: ONLINE\r\n\
        ZBMTEST-STATUS-END\r\nZBMTEST-TEARDOWN dm=[] pools=[tforeign] state=[]\r\n";

    let checks = drive_script(
        "runtime-legacy-force",
        ZbmRuntime::Legacy,
        "force",
        vec![Step::Say(reports)],
    );

    assert_eq!(
        checks,
        [
            "ok   - no prompts (prompts: [])",
            "ok   - ZFSBootMenu imported tforeign: ONLINE",
            "ok   - teardown exported pools and closed volumes (dm=[] pools=[tforeign] state=[])",
        ]
    );
}

#[test]
fn prefer_fails_when_the_other_pool_is_imported_too() {
    let reports = PLAIN.replace(
        "ZS|  state: ONLINE\r\n",
        "ZS|  state: ONLINE\r\nZS|   pool: tforeign\r\nZS|  state: ONLINE\r\n",
    );
    let script = vec![Step::Say(reports.leak()), Step::Say(BE)];

    let checks = drive_script(
        "runtime-legacy-prefer",
        ZbmRuntime::Legacy,
        "prefer",
        script,
    );

    assert_eq!(
        checks[2],
        "FAIL - zbm.prefer=tplain!! imports only tplain (['tforeign', 'tplain'])"
    );
}

#[test]
fn hooks_from_the_hook_root_run_and_skipped_hooks_do_not() {
    let reports = "Booting /boot/vmlinuz-7.3 for tplain/ROOT/be in 3 seconds\r\n\
        ZBMTEST-STATUS-BEGIN\r\nZS|   pool: tplain\r\nZS|  state: ONLINE\r\nZBMTEST-STATUS-END\r\n\
        ZBMTEST-HOOKROOT ran from /libexec/hooks/teardown.d/50-hookroot\r\n\
        ZBMTEST-TEARDOWN dm=[] pools=[tplain] state=[]\r\n";
    let script = vec![Step::Say(reports), Step::Say(BE)];

    let checks = drive_script("runtime-legacy-hooks", ZbmRuntime::Legacy, "hooks", script);

    assert_eq!(
        checks[2..5],
        [
            "ok   - zbm.hookroot added a teardown hook",
            "ok   - zbm.skip_hooks disabled the runtime report hook",
            "ok   - the countdown boots the default boot environment",
        ]
    );
}

#[test]
fn the_runtime_report_is_read_from_the_log() {
    let log = "x ZBMTEST-RUNTIME pid1=[ /init ] conf=[] bootfs=[a/b ]\n";

    let report = report(log).unwrap();

    assert_eq!(
        report,
        Some(("/init".to_owned(), String::new(), "a/b".to_owned()))
    );
    assert_eq!(super::report("nothing").unwrap(), None);
}
