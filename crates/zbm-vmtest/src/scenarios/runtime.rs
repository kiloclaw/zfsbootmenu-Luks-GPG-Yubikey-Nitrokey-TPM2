//! Start-up and pool import with each runtime, without LUKS (Phase 5 and
//! package A of Phase 6 in `PORT-ROADMAP.md`).
//!
//! - `setup`: creates two pools, each holding the ZFS 2.4.4 test boot
//!   environment as its bootfs: `tplain` on disk d0, exported by this host,
//!   and `tforeign` on disk d1, left imported by another host (host ID
//!   `deadbeef`).
//! - `RUNTIME[-SCENARIO]`: boots with RUNTIME, `legacy` or `rust`
//!   (`zfsbootmenu_runtime="rust"`); the scenarios (`boot`):
//!   - (none): `tplain`: the countdown boots its boot environment; process
//!     1, the runtime and `BOOTFS` as hooks see them;
//!   - `hostid`: `tforeign` alone: host ID matching imports it with the
//!     other host's ID, which the boot environment gets as `spl_hostid`;
//!   - `strict`: `tforeign` alone with `zbm.import_policy=strict`: no
//!     import; the retry prompt appears;
//!   - `force`: `tforeign` alone with `zbm.import_policy=force`: imported
//!     at once;
//!   - `prefer`: both pools with `zbm.prefer=tplain!!`: only `tplain` is
//!     imported, and its boot environment boots;
//!   - `late`: d0 is plugged in while the import retry loop waits;
//!   - `waitfor`: `zbm.wait_for` names d0, which is plugged in while
//!     ZFSBootMenu waits for it;
//!   - `hooks`: `zbm.hookroot` adds a hook from a FAT disk,
//!     `zbm.skip_hooks` disables the runtime report hook;
//!   - `signals` (`signals`): the signal dispositions of a hook, fzf
//!     (legacy only: the menu) and the recovery shell; the Rust run
//!     compares with the legacy run's.
//! - `rust-pilot` (`pilot`): the pilots of `REFACTOR-ROADMAP.md` (R0).
//! - `rust-autosize` (`autosize`): the console font follows the display's
//!   EDID (`zbm.autosize`, HiDPI), on a virtio GPU.
//! - `rust-colours` (`colours`): the image's console fonts leave the VGA
//!   text console and the framebuffer console their 16 colours.
//! - `rust-drm` (`drm`): the menu prototype on the display without a
//!   terminal, on `virtio_gpu` and on `simpledrm` (ROADMAP.md #19).
//! - `rust-crash` (`crash`): a hook runs `zbm debug panic` in an image
//!   built with the crash-test feature and prints the crash report;
//!   `cargo xtask symbolize` must name the panicking function and line from
//!   the image's debug file.
//! - `record-zfs` (`record_zfs`): not a test: records what `zfs` and
//!   `zpool` print as the fixtures of `crates/zbm-zfs` (`cargo xtask
//!   record-zfs`, which defines the commands).
//! - `record-fixtures` (`record_fixtures`): not a test: records what
//!   `zfs`, `zpool` and `/proc/self/mounts` print as the fixtures of the
//!   legacy-compatible commands of `crates/zbm-zfs`.
//!
//! Every scenario must pass with the legacy image first (rule 2 of the
//! roadmap).

use std::fs;
use std::thread;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::images::{Image, ImageSpec, zbm_image};
use crate::qemu::Disk;
use crate::record::Test;
use crate::settings::{Settings, ZbmRuntime};
use crate::text::clean;
use crate::vm::Vm;

mod autosize;
mod boot;
mod colours;
mod crash;
mod drm;
mod host;
mod pilot;
mod record_fixtures;
mod record_zfs;
mod setup;
mod signals;

/// The command line of the machines that open the recovery shell from the
/// import retry loop.
const SHELL_APPEND: &str = "console=ttyS0 zbm.luks=off zbm.retry_delay=3";

/// The command line of the machines that escape the countdown: long
/// enough for the test to press Escape.
const ESCAPE_APPEND: &str = "console=ttyS0 zbm.timeout=30 loglevel=7";

/// The countdown of `tplain`'s boot environment.
const COUNTDOWN: &str = r"Booting \S*tplain/ROOT/be\S* in";

/// Runs `setup`, `record-zfs`, `rust-pilot`, `rust-crash`, `rust-autosize`,
/// `rust-colours`, `rust-drm`, `RUNTIME-signals` or `RUNTIME[-SCENARIO]`.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub fn run(test: &Test, scenario: &str) -> Result<()> {
    match scenario {
        "setup" => setup::run(test),
        "record-zfs" => record_zfs::run(test),
        "record-fixtures" => record_fixtures::run(test),
        "rust-pilot" => pilot::run(test),
        "rust-crash" => crash::run(test),
        "rust-autosize" => autosize::run(test),
        "rust-colours" => colours::run(test),
        "rust-drm" => drm::run(test),
        _ => {
            let (name, rest) = scenario.split_once('-').unwrap_or((scenario, ""));
            let runtime = parse_runtime(name)?;
            if rest == "signals" {
                signals::run(test, runtime)
            } else {
                boot::run(test, runtime, rest)
            }
        }
    }
}

/// The runtime named `name`.
fn parse_runtime(name: &str) -> Result<ZbmRuntime> {
    match name {
        "legacy" => Ok(ZbmRuntime::Legacy),
        "rust" => Ok(ZbmRuntime::Rust),
        _ => Err(Error::Setup(format!("unknown runtime {name}"))),
    }
}

/// The test image `variant` with `runtime`, and `hook` (path in the build
/// directory, script) when given.
fn image(
    settings: &Settings,
    variant: &str,
    runtime: ZbmRuntime,
    hook: Option<(&str, &str)>,
) -> Result<Image> {
    let mut spec = ImageSpec::new(variant);
    spec.runtime = Some(runtime);
    spec.files = hook
        .into_iter()
        .map(|(path, script)| (path.to_owned(), script.as_bytes().to_vec()))
        .collect();
    zbm_image(settings, &spec)
}

/// The disk `d<index>` of the group.
fn disk(settings: &Settings, index: usize) -> Result<Disk> {
    let path = crate::disks::disk(settings, "runtime", index, "384M", false)?;
    Ok(Disk::new(path, format!("d{index}")))
}

/// Waits for the countdown of `tplain`'s boot environment and escapes it.
fn escape_countdown(vm: &mut Vm<'_>) -> Result<()> {
    vm.expect1(COUNTDOWN, 1200)?;
    thread::sleep(Duration::from_secs(2));
    vm.send("\x1b")
}

/// What the serial console of `vm` showed so far, without escape sequences
/// and carriage returns.
fn serial_text(vm: &Vm<'_>) -> Result<String> {
    let path = vm.serial_log();
    let bytes = fs::read(&path).map_err(Error::io(format!("reading {}", path.display())))?;
    Ok(clean(&String::from_utf8_lossy(&bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtimes_are_named_as_in_the_test_names() {
        assert_eq!(parse_runtime("legacy").unwrap(), ZbmRuntime::Legacy);
        assert_eq!(parse_runtime("rust").unwrap(), ZbmRuntime::Rust);
        assert!(parse_runtime("record").is_err());
    }

    #[test]
    fn the_countdown_matches_with_or_without_a_pool_prefix() {
        let countdown = crate::expect::regex(COUNTDOWN).unwrap();

        assert!(countdown.is_match("Booting tplain/ROOT/be in 30 seconds"));
        assert!(countdown.is_match("Booting [tplain/ROOT/be] in 3"));
        assert!(!countdown.is_match("Booting tforeign/ROOT/be in 3"));
    }
}
