//! `runtime-RUNTIME[-SCENARIO]`: a boot with the legacy or the Rust
//! runtime, without LUKS, from the countdown into the boot environment.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use crate::boot::{Boot, Follow, Kind, check_be, check_teardown, dialogs, follow_boot};
use crate::disks::vfat_disk;
use crate::error::{Error, Result};
use crate::expect::regex;
use crate::qemu::{Disk, Machine};
use crate::record::Test;
use crate::settings::{Settings, ZbmRuntime};
use crate::vm::Vm;

/// A teardown hook on the FAT disk of `hooks`.
const HOOK: &str = r#"#!/bin/bash
echo "ZBMTEST-HOOKROOT ran from $0"
"#;

/// The command line of every boot scenario, before the scenario's options.
const APPEND: &str = "console=ttyS0 zbm.timeout=3 loglevel=7";

/// The report of the runtime report hook (`hooks/teardown.d/20-runtime.sh`).
const REPORT: &str = r"ZBMTEST-RUNTIME pid1=\[(.*?)\] conf=\[(.*?)\] bootfs=\[(.*?)\]";

/// How a scenario's machine is set up.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Plan {
    /// The disks `d<index>` attached at boot.
    disks: &'static [usize],
    /// Whether the FAT disk `hooks` is attached too.
    hooks: bool,
    /// Options added to [`APPEND`].
    extra: &'static str,
}

impl Plan {
    /// The kernel command line.
    fn append(&self) -> String {
        format!("{APPEND}{}", self.extra)
    }
}

/// The plan of `scenario` (empty: the plain boot).
fn plan(scenario: &str) -> Result<Plan> {
    let (disks, extra): (&'static [usize], &'static str) = match scenario {
        "" => (&[0], ""),
        "hostid" => (&[1], ""),
        "strict" => (&[1], " zbm.import_policy=strict zbm.retry_delay=5"),
        "force" => (&[1], " zbm.import_policy=force"),
        "prefer" => (&[0, 1], " zbm.prefer=tplain!!"),
        "late" => (&[], " zbm.retry_delay=10"),
        "waitfor" => (
            &[],
            " zbm.wait_for=/dev/disk/by-id/virtio-d0 zbm.retry_delay=10",
        ),
        "hooks" => (
            &[0],
            " zbm.hookroot=/dev/disk/by-id/virtio-hooks//zbm zbm.skip_hooks=20-runtime.sh",
        ),
        _ => return Err(Error::Setup(format!("unknown scenario {scenario}"))),
    };
    Ok(Plan {
        disks,
        hooks: scenario == "hooks",
        extra,
    })
}

/// Boots with `runtime` as `scenario` says and checks the boot.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test, runtime: ZbmRuntime, scenario: &str) -> Result<()> {
    let settings = test.settings();
    let plan = plan(scenario)?;
    let image = super::image(settings, "auto", runtime, None)?;
    let mut disks = plan
        .disks
        .iter()
        .map(|&index| super::disk(settings, index))
        .collect::<Result<Vec<Disk>>>()?;
    if plan.hooks {
        disks.push(hooks_disk(settings)?);
    }
    let machine = Machine::boot(&image, &plan.append()).with_disks(disks);
    let mut vm = Vm::start(test, "boot", &machine)?;
    drive(&mut vm, runtime, scenario)
}

/// The FAT disk `hooks` with a teardown hook below `zbm`.
fn hooks_disk(settings: &Settings) -> Result<Disk> {
    let dir = settings.disks("runtime");
    std::fs::create_dir_all(&dir).map_err(Error::io(format!("creating {}", dir.display())))?;
    let files: [(&str, &[u8]); 1] = [("zbm/teardown.d/50-hookroot", HOOK.as_bytes())];
    let path = vfat_disk(&dir.join("hooks.img"), "HOOKS", &files, "64M", 512)?;
    Ok(Disk::new(path, "hooks"))
}

/// The disk d0, plugged in while ZFSBootMenu runs.
fn d0(settings: &Settings) -> Result<PathBuf> {
    crate::disks::disk(settings, "runtime", 0, "384M", false)
}

/// Follows the boot of a started machine and checks it.
fn drive(vm: &mut Vm<'_>, runtime: ZbmRuntime, scenario: &str) -> Result<()> {
    let test = vm.test();
    if scenario == "strict" {
        return strict(vm);
    }
    let mut follow = Follow {
        no_be: scenario == "force",
        timeout: Some(1200),
        ..Follow::default()
    };
    if scenario == "late" {
        let path = d0(test.settings())?;
        follow = follow.on(Kind::Retry, move |vm| vm.hotplug_disk(&path, "d0"));
    }
    if scenario == "waitfor" {
        vm.expect(&["required devices are missing", dialogs::DEVICES], 1200)?;
        test.step("ZFSBootMenu waits for d0; plugging it in");
        thread::sleep(Duration::from_secs(2));
        vm.hotplug_disk(&d0(test.settings())?, "d0")?;
    }
    let boot = follow_boot(vm, follow)?;
    let log = super::serial_text(vm)?;
    test.check(
        boot.prompts.is_empty(),
        &format!("no prompts (prompts: {})", boot.prompts_text()),
    );
    if matches!(scenario, "hostid" | "force") {
        return foreign(vm, &boot, runtime, scenario, &log);
    }
    plain(test, &boot, runtime, scenario, &log)
}

/// `strict`: the pool of another host is not imported.
fn strict(vm: &mut Vm<'_>) -> Result<()> {
    let test = vm.test();
    let patterns = ["Unable to import", dialogs::RETRY, "ZBMTEST-STATUS-BEGIN"];
    let index = vm.expect(&patterns, 1200)?;
    test.check(
        index < 2,
        "the strict policy does not import a pool of another host",
    );
    vm.kill();
    let log = super::serial_text(vm)?;
    test.check(
        !log.contains("Booting tforeign") && !log.contains(" for tforeign/"),
        "nothing is booted",
    );
    Ok(())
}

/// `hostid` and `force`: `tforeign`, the pool of another host, is imported.
fn foreign(
    vm: &mut Vm<'_>,
    boot: &Boot,
    runtime: ZbmRuntime,
    scenario: &str,
    log: &str,
) -> Result<()> {
    let test = vm.test();
    let pools = boot.pools();
    test.check(
        pools.get("tforeign") == Some("ONLINE"),
        &format!("ZFSBootMenu imported tforeign: {}", pools.show("tforeign")),
    );
    if scenario == "hostid" {
        check_runtime(test, runtime, log, "tforeign/ROOT/be")?;
        check_teardown(test, boot, &["tforeign"]);
        let spl = boot.be_line("BE-CMDLINE");
        test.check(
            spl.contains("spl_hostid=0xdeadbeef") || spl.contains("spl_hostid=deadbeef"),
            &format!("the boot environment gets the matched host ID ({spl})"),
        );
        check_be(test, boot, "tforeign", &["tforeign"], &[]);
    } else {
        check_teardown(test, boot, &["tforeign"]);
        vm.kill();
    }
    Ok(())
}

/// The other scenarios: `tplain` is imported and its boot environment
/// boots.
fn plain(test: &Test, boot: &Boot, runtime: ZbmRuntime, scenario: &str, log: &str) -> Result<()> {
    let pools = boot.pools();
    test.check(
        pools.get("tplain") == Some("ONLINE"),
        &format!("ZFSBootMenu imported tplain: {}", pools.show("tplain")),
    );
    if scenario == "prefer" {
        test.check(
            !pools.contains("tforeign"),
            &format!(
                "zbm.prefer=tplain!! imports only tplain ({})",
                pools.sorted_names()
            ),
        );
    }
    if scenario == "hooks" {
        test.check(
            log.contains("ZBMTEST-HOOKROOT ran from /libexec/hooks/teardown.d/50-hookroot"),
            "zbm.hookroot added a teardown hook",
        );
        test.check(
            report(log)?.is_none(),
            "zbm.skip_hooks disabled the runtime report hook",
        );
    } else {
        check_runtime(test, runtime, log, "tplain/ROOT/be")?;
    }
    test.check(
        regex(r"Booting \S+ for tplain/ROOT/be")?.is_match(log),
        "the countdown boots the default boot environment",
    );
    // ZFSBootMenu exports only pools it imported read-write
    check_teardown(test, boot, &["tplain"]);
    check_be(test, boot, "tplain", &["tplain"], &[]);
    Ok(())
}

/// Process 1, the runtime and `BOOTFS` from the runtime report hook.
fn report(log: &str) -> Result<Option<(String, String, String)>> {
    let captures = regex(REPORT)?.captures(log);
    Ok(captures.map(|captures| {
        let group = |n: usize| captures[n].trim().to_owned();
        (group(1), group(2), group(3))
    }))
}

/// Checks the runtime report: the runtime the image records, process 1 and
/// the `BOOTFS` hooks see.
fn check_runtime(test: &Test, runtime: ZbmRuntime, log: &str, bootfs: &str) -> Result<()> {
    let report = report(log)?;
    test.check(report.is_some(), "the runtime report hook ran");
    let Some((pid1, conf, seen)) = report else {
        return Ok(());
    };
    test.note(&format!("process 1: {pid1}; {conf}"));
    let name = runtime.as_str();
    test.check(
        conf == format!("ZBM_RUNTIME=\"{name}\""),
        &format!("the image records the {name} runtime"),
    );
    if runtime == ZbmRuntime::Rust {
        test.check(
            pid1 == "/libexec/zbm zfsbootmenu-init --supervise",
            "process 1 is the Rust supervisor",
        );
    } else {
        test.check(
            pid1.contains("zfsbootmenu-init") && !pid1.contains("zbm"),
            "process 1 is the legacy start-up program",
        );
    }
    test.check(seen == bootfs, &format!("hooks see BOOTFS ({seen})"));
    Ok(())
}

#[cfg(test)]
mod tests;
