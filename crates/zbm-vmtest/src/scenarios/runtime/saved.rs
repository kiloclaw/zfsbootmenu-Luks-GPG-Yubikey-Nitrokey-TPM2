//! `runtime-rust-saved`: saved boot logs and bug reports (ROADMAP.md #9).
//!
//! The first boot, with `zbm.save_logs` naming a FAT disk, boots `tplain`'s
//! boot environment; ZFSBootMenu saves its logs on the disk before `kexec`.
//! After a restart, `zbm debug saved` in the recovery shell must list the
//! first boot's logs, its record `boot.json` among them, and `zbm report`
//! must write a report on the same disk. The host then reads both from the
//! disk, and checks what the record says of the first boot. The reports of
//! `zbm-debug(8)` must read this boot's start-up there too.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::boot::{Follow, check_be, follow_boot};
use crate::disks::vfat_disk;
use crate::error::{Error, Result};
use crate::expect::regex;
use crate::qemu::{Disk, Machine};
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

/// The label of the FAT disk that holds the logs.
const LABEL: &str = "ZBMLOGS";

/// The option that saves the logs on it.
const SAVE: &str = " zbm.save_logs=LABEL=ZBMLOGS";

/// Where the logs and the reports are on the disk.
const LOGS: &str = "EFI/zfsbootmenu/logs";

/// The boot environment the first boot boots.
const BE: &str = "tplain/ROOT/be";

/// A boot that `zbm debug saved` lists: its directory and its files.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SavedBoot {
    name: String,
    files: String,
}

/// Boots once into the boot environment, restarts, and reads the logs.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let image = super::image(settings, "auto", ZbmRuntime::Rust, None)?;
    let dir = settings.disks("runtime");
    std::fs::create_dir_all(&dir).map_err(Error::io(format!("creating {}", dir.display())))?;
    let logs = vfat_disk(&dir.join("logs.img"), LABEL, &[], "64M", 512)?;
    let disks = || -> Result<Vec<Disk>> {
        Ok(vec![
            super::disk(settings, 0)?,
            Disk::new(logs.clone(), "logs"),
        ])
    };

    let first = Machine::boot(
        &image,
        &format!("console=ttyS0 zbm.timeout=3 loglevel=7{SAVE}"),
    )
    .with_disks(disks()?);
    let mut vm = Vm::start(test, "boot", &first)?;
    let follow = Follow {
        timeout: Some(1200),
        ..Follow::default()
    };
    let boot = follow_boot(&mut vm, follow)?;
    check_be(test, &boot, "tplain", &["tplain"], &[]);
    vm.kill();

    test.step("restarting the machine");
    let second =
        Machine::boot(&image, &format!("{}{SAVE}", super::ESCAPE_APPEND)).with_disks(disks()?);
    let mut vm = Vm::start(test, "restart", &second)?;
    super::escape_to_shell(&mut vm)?;
    let (code, listed) = vm.sh_show("/libexec/zbm debug saved")?;
    test.check(code == 0, "zbm debug saved lists the saved logs");
    let boots = saved_boots(&listed);
    check_saved(test, &boots);
    let (code, written) = vm.sh_show("/libexec/zbm report")?;
    test.check(code == 0, "zbm report writes a report");
    check_reports(test, &mut vm)?;
    vm.kill();

    check_on_disk(test, &logs, &boots, &written)
}

/// The reports of `zbm-debug(8)` the restart runs, and what each must
/// print about its start-up.
const REPORTS: &[(&str, &[&str])] = &[
    ("config", &["zbm.save_logs", "LABEL=ZBMLOGS"]),
    ("state", &["== "]),
    ("journal --level info", &["running init stage"]),
    ("commands", &["zpool import", " ms  "]),
    ("pools", &["tplain", "ONLINE", "command line:"]),
    ("luks", &["no LUKS volume"]),
];

/// Runs the reports of `zbm-debug(8)` in the recovery shell.
fn check_reports(test: &Test, vm: &mut Vm) -> Result<()> {
    for (report, wanted) in REPORTS {
        let (code, output) = vm.sh_show(&format!("/libexec/zbm debug {report}"))?;
        check_report(test, report, code, &output, wanted);
    }
    Ok(())
}

/// Checks that `zbm debug REPORT` ended with 0 and printed each of
/// `wanted`, and what `state` and `commands` must or must not print.
fn check_report(test: &Test, report: &str, code: i32, output: &str, wanted: &[&str]) {
    test.check(code == 0, &format!("zbm debug {report} ends with 0"));
    for text in wanted {
        test.check(
            output.contains(text),
            &format!("zbm debug {report} prints {text:?}"),
        );
    }
    match report {
        "state" => test.check(
            !output.contains("journal.jsonl"),
            "zbm debug state leaves out the journal",
        ),
        "commands" => test.check(
            regex(r"\d+ commands, \d+ failed or did not start, \d+ ms in all")
                .is_ok_and(|summary| summary.is_match(output)),
            "zbm debug commands ends with a summary",
        ),
        _ => {}
    }
}

/// The boots `zbm debug saved` listed.
fn saved_boots(output: &str) -> Vec<SavedBoot> {
    let Ok(line) = regex(r"^(\d{4}-\d\d-\d\dT\d\d-\d\d-\d\dZ(?:\.\d+)?): (.+)$") else {
        return Vec::new();
    };
    output
        .lines()
        .filter_map(|text| {
            let captures = line.captures(text.trim())?;
            Some(SavedBoot {
                name: captures[1].to_owned(),
                files: captures[2].to_owned(),
            })
        })
        .collect()
}

/// Checks that the first boot's logs were listed, and nothing else.
fn check_saved(test: &Test, boots: &[SavedBoot]) {
    test.note(&format!("saved boots: {boots:?}"));
    test.check(boots.len() == 1, "the first boot's logs are saved, once");
    let files = boots.first().map_or("", |boot| boot.files.as_str());
    test.check(
        files.contains("journal.jsonl (") && !files.contains("journal.jsonl (0 bytes)"),
        &format!("the saved logs hold the journal ({files})"),
    );
    test.check(
        files.contains("kernel.log ("),
        "the saved logs hold the kernel log",
    );
    test.check(
        files.contains("boot.json (") && !files.contains("boot.json.tmp"),
        "the saved logs hold the boot's record, renamed whole",
    );
}

/// Checks the first boot's record, `boot.json`: one line of schema 1 that
/// names `tplain`'s boot environment, its kernel and command line, loaded
/// by `kexec_file_load`, not measured, as the machine has no TPM, and with
/// the environment's own command line.
fn check_record(test: &Test, text: &str) {
    test.note(&format!("boot.json: {text}"));
    let record: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let field = |name: &str| record.get(name).cloned().unwrap_or_default();
    let text_of = |name: &str| field(name).as_str().map(str::to_owned).unwrap_or_default();
    test.check(
        text.ends_with("}\n") && text.trim_end().lines().count() == 1,
        "the record is one line of JSON",
    );
    test.check(
        field("schema") == 1 && field("time").as_u64().is_some_and(|time| time > 0),
        "the record has schema 1 and the time of the save",
    );
    test.check(
        field("environment") == BE,
        "the record names the boot environment booted",
    );
    test.check(
        text_of("kernel").starts_with("/boot/") && text_of("initramfs").starts_with("/boot/"),
        "the record names the kernel and the initramfs",
    );
    test.check(
        text_of("command_line").starts_with(&format!("root=zfs:{BE}"))
            && field("command_line_edited") == false,
        "the record holds the environment's own command line",
    );
    test.check(
        field("loaded_by") == "kexec_file_load" && field("measured") == false,
        "the record says kexec_file_load loaded the kernel, unmeasured",
    );
}

/// The report's directory on the disk, from what `zbm report` printed.
fn report_dir(output: &str) -> Option<String> {
    let found = regex(r"report written in (EFI/zfsbootmenu/reports/\S+) on /dev/\S+").ok()?;
    let captures = found.captures(output)?;
    Some(captures[1].to_owned())
}

/// Reads the first boot's journal and the report from the disk image.
fn check_on_disk(test: &Test, logs: &Path, boots: &[SavedBoot], written: &str) -> Result<()> {
    if let Some(boot) = boots.first() {
        let journal = mtype(logs, &format!("{LOGS}/{}/journal.jsonl", boot.name))?;
        test.check(
            journal.contains("running init stage"),
            "the saved journal is the first boot's",
        );
        // Saved once the kernel was loaded, and before the teardown hooks,
        // which may take the partition away.
        test.check(
            journal.contains("kexec") && !names_teardown_d(&journal),
            "the journal was saved after loading the kernel, before the teardown hooks",
        );
        // Loaded by the kernel's own call, without `kexec` (ROADMAP.md,
        // SB4).
        test.check(
            journal.contains("kexec_file_load loaded"),
            "kexec_file_load loaded the kernel",
        );
        let record = mtype(logs, &format!("{LOGS}/{}/boot.json", boot.name))?;
        check_record(test, &record);
    }
    let Some(dir) = report_dir(written) else {
        test.check(
            false,
            &format!("zbm report named the report ({})", written.trim()),
        );
        return Ok(());
    };
    let report = mtype(logs, &format!("{dir}/report.txt"))?;
    test.note(&report);
    for (part, what) in [
        ("== versions\n", "the versions"),
        ("pool: tplain", "the pools"),
        ("== LUKS volumes\nnone\n", "the LUKS volumes"),
    ] {
        test.check(report.contains(part), &format!("the report shows {what}"));
    }
    let journal = mtype(logs, &format!("{dir}/journal.jsonl"))?;
    test.check(!journal.is_empty(), "the report holds this boot's journal");
    Ok(())
}

/// Whether a line of `journal` names `teardown.d`, the `teardown.d` hooks
/// having run or been looked for, but for the event of the system report
/// that start-up logs, whose `report` field and message list the enabled
/// hooks by their paths. A line that is not JSON is searched whole.
fn names_teardown_d(journal: &str) -> bool {
    journal.lines().any(|line| {
        let is_the_report = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|event| event.get("fields")?.get("report").cloned())
            .is_some();
        !is_the_report && line.contains("teardown.d")
    })
}

/// The kernel log saved on the FAT disk `image`, of its first saved boot;
/// empty when it holds none.
pub(crate) fn saved_kernel_log(image: &Path) -> Result<String> {
    saved_file(image, "kernel.log")
}

/// The record `boot.json` saved on the FAT disk `image`, of its first
/// saved boot; empty when it holds none.
pub(crate) fn saved_record(image: &Path) -> Result<String> {
    saved_file(image, "boot.json")
}

/// The file `name` of the first boot saved on the FAT disk `image`; empty
/// when it holds none.
fn saved_file(image: &Path, name: &str) -> Result<String> {
    let listed = crate::disks::run(
        Command::new("mdir")
            .env("MTOOLS_SKIP_CHECK", "1")
            .args(["-b", "-i"])
            .arg(image)
            .arg(format!("::/{LOGS}")),
    )?;
    let Some(boot) = first_boot(&listed) else {
        return Ok(String::new());
    };
    mtype(image, &format!("{LOGS}/{boot}/{name}"))
}

/// The first saved boot that `mdir -b` lists in `listed`: a directory
/// named after its time, which starts with a digit.
fn first_boot(listed: &str) -> Option<&str> {
    listed
        .lines()
        .filter_map(|line| line.trim_end_matches('/').rsplit('/').next())
        .find(|name| name.starts_with(|c: char| c.is_ascii_digit()))
}

/// The file `inner` of the FAT disk image `image`.
pub(super) fn mtype(image: &Path, inner: &str) -> Result<String> {
    let path: PathBuf = image.to_owned();
    crate::disks::run(
        Command::new("mtype")
            .env("MTOOLS_SKIP_CHECK", "1")
            .arg("-i")
            .arg(path)
            .arg(format!("::/{inner}")),
    )
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::testing::{checks, cleanup, settings};

    #[test]
    fn the_system_report_s_list_of_teardown_hooks_is_no_teardown() {
        let journal = concat!(
            r#"{"message":"System Report\n* /libexec/hooks/teardown.d/10-zstatus.sh","#,
            r#""fields":{"report":"System Report\n\n>> Enabled hooks\n"#,
            r#"* /libexec/hooks/teardown.d/10-zstatus.sh\n","template":"{report}"}}"#,
            "\n",
            r#"{"fields":{"message":"kexec_file_load loaded the kernel"}}"#,
            "\n",
        );

        assert!(!names_teardown_d(journal));
    }

    #[test]
    fn a_teardown_hook_in_the_journal_is_named() {
        for journal in [
            r#"{"fields":{"message":"processing /libexec/hooks/teardown.d/10-zstatus.sh"}}"#,
            r#"{"message":"processing /libexec/hooks/teardown.d/10-zstatus.sh","fields":{}}"#,
            r#"{"fields":{"line":"-"},"spans":[{"line":"/libexec/hooks/teardown.d/x"}]}"#,
            "not JSON: /libexec/hooks/teardown.d/10-zstatus.sh",
        ] {
            assert!(names_teardown_d(journal), "{journal}");
        }
    }

    #[test]
    fn the_first_saved_boot_is_the_first_directory_named_by_its_time() {
        let listed = "::/EFI/zfsbootmenu/logs/reports/\n\
                      ::/EFI/zfsbootmenu/logs/2026-10-24T10-00-00Z/\n\
                      ::/EFI/zfsbootmenu/logs/2026-10-25T10-00-00Z/\n";

        assert_eq!(first_boot(listed), Some("2026-10-24T10-00-00Z"));
        assert_eq!(first_boot("::/EFI/zfsbootmenu/logs/reports/\n"), None);
        assert_eq!(first_boot(""), None);
        let missing = Path::new("/nonexistent/zbm-vmtest-logs.img");
        assert!(saved_record(missing).is_err());
        assert!(saved_kernel_log(missing).is_err());
    }

    #[test]
    fn the_saved_boots_and_the_report_are_read_from_the_output() {
        let output = "saved logs on /dev/vdb:\r\n\
                      2026-10-24T10-00-00Z: boot.json (300 bytes), journal.jsonl (812 bytes), \
                      kernel.log (9 bytes)\r\n\
                      2026-10-24T10-00-00Z.2: kernel.log (0 bytes)\n";

        let boots = saved_boots(output);

        assert_eq!(boots.len(), 2);
        assert_eq!(boots[0].name, "2026-10-24T10-00-00Z");
        assert_eq!(boots[1].files, "kernel.log (0 bytes)");
        let written = "report written in EFI/zfsbootmenu/reports/zbm-report-X on /dev/vdb\n";
        assert_eq!(
            report_dir(written).as_deref(),
            Some("EFI/zfsbootmenu/reports/zbm-report-X")
        );
        assert_eq!(report_dir("zbm report: unable to mount /dev/vdb"), None);
    }

    #[test]
    fn the_saved_logs_must_hold_the_journal_once() {
        let settings = settings("runtime-saved-checks");
        let test = Test::new(&settings, "runtime-rust-saved").unwrap();
        let good = SavedBoot {
            name: "2026-10-24T10-00-00Z".to_owned(),
            files: "boot.json (300 bytes), journal.jsonl (812 bytes), kernel.log (9 bytes)"
                .to_owned(),
        };
        check_saved(&test, std::slice::from_ref(&good));
        let empty = SavedBoot {
            files: "journal.jsonl (0 bytes)".to_owned(),
            ..good.clone()
        };
        check_saved(&test, &[good, empty]);
        check_saved(&test, &[]);

        let results: Vec<bool> = checks(&test)
            .iter()
            .map(|line| line.starts_with("ok"))
            .collect();
        assert_eq!(&results[..4], [true, true, true, true]);
        assert_eq!(&results[4..8], [false, true, true, true]);
        assert_eq!(&results[8..], [false, false, false, false]);
        cleanup(&settings);
    }

    #[test]
    fn the_record_must_name_the_first_boot() {
        let settings = settings("runtime-saved-record");
        let test = Test::new(&settings, "runtime-rust-saved").unwrap();
        let good = concat!(
            r#"{"schema":1,"time":1793246955,"environment":"tplain/ROOT/be","#,
            r#""kernel":"/boot/vmlinuz-6.12","initramfs":"/boot/initramfs-6.12.img","#,
            r#""command_line":"root=zfs:tplain/ROOT/be          quiet","devicetree":null,"#,
            r#""loaded_by":"kexec_file_load","measured":false,"command_line_edited":false}"#,
            "\n"
        );
        check_record(&test, good);
        let other = good
            .replace("schema\":1", "schema\":2")
            .replace("tplain/ROOT/be", "tplain/ROOT/other")
            .replace("/boot/vmlinuz", "vmlinuz")
            .replace("\"kexec_file_load\"", "\"kexec\"");
        check_record(&test, &other);
        check_record(&test, "");

        let results: Vec<bool> = checks(&test)
            .iter()
            .map(|line| line.starts_with("ok"))
            .collect();
        assert_eq!(&results[..6], [true; 6]);
        assert_eq!(&results[6..12], [true, false, false, false, false, false]);
        assert_eq!(&results[12..], [false; 6]);
        cleanup(&settings);
    }

    #[test]
    fn the_reports_must_end_well_and_show_the_start_up() {
        let settings = settings("runtime-saved-reports");
        let test = Test::new(&settings, "runtime-rust-saved").unwrap();
        let commands = "   0      12 ms  zpool import -N tplain\r\n\
                        1 commands, 0 failed or did not start, 12 ms in all\r\n";
        check_report(&test, "commands", 0, commands, &["zpool import"]);
        check_report(&test, "commands", 1, "", &["zpool import"]);
        check_report(&test, "state", 0, "== bootfs (5 bytes)\n", &["== "]);
        check_report(&test, "state", 0, "== journal.jsonl (5 bytes)\n", &[]);
        check_report(&test, "luks", 0, "no LUKS volume\n", &["no LUKS volume"]);

        let results: Vec<bool> = checks(&test)
            .iter()
            .map(|line| line.starts_with("ok"))
            .collect();
        assert_eq!(&results[..3], [true, true, true]);
        assert_eq!(&results[3..6], [false, false, false]);
        assert_eq!(&results[6..9], [true, true, true]);
        assert_eq!(&results[9..11], [true, false]);
        assert_eq!(&results[11..], [true, true]);
        assert_eq!(REPORTS.len(), 6);
        cleanup(&settings);
    }
}
