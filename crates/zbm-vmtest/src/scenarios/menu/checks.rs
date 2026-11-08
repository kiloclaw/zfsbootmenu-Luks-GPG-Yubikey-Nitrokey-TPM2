//! What the menu tests check in the recovery shell, after the menu did its
//! tasks: the space a destroy freed, the datasets, the files restored, and
//! the pools and the volume.

use std::thread;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::record::Test;
use crate::vm::Vm;

use super::UUID;

/// The boot environment the snapshots belong to.
const BE: &str = "tmenu/ROOT/be";

/// How many times the space being freed is read before it is taken as
/// stuck, a second apart.
const FREEING_READS: usize = 60;

/// The content `restore-me` had in `one`, which the restore brings back.
pub(super) const RESTORED: &str = "restore-me at one";

/// What `ls -ln` says first of `removed`, brought back: its mode 0640, one
/// link, its owner 1234 and its group 5678.
const REMOVED: &str = "-rw-r----- 1 1234 5678 ";

/// The space the snapshots of the boot environment hold, and what
/// destroying `one` and `three` would free, as `zfs destroy -nvp` says.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Space {
    /// `usedbysnapshots`, in bytes.
    pub(super) snapshots: u64,
    /// The `reclaim` of the dry run, in bytes.
    pub(super) reclaim: u64,
}

/// A number in bytes the shell printed alone.
fn bytes(output: &str) -> Option<u64> {
    output.trim().parse().ok()
}

/// The `reclaim` line of `zfs destroy -nvp`.
fn reclaim(output: &str) -> Option<u64> {
    output.lines().find_map(
        |line| match line.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["reclaim", space] => space.parse().ok(),
            _ => None,
        },
    )
}

/// Syncs `tmenu`, and waits until it frees nothing more: what a destroy
/// frees is counted once it is synced, and freed in the background.
fn settle(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    vm.must("zpool sync tmenu")?;
    for _ in 0..FREEING_READS {
        let (_, freeing) = vm.sh("zpool get -Hp -o value freeing tmenu")?;
        if bytes(&freeing) == Some(0) {
            return Ok(());
        }
        thread::sleep(Duration::from_secs(1));
    }
    test.check(false, "tmenu frees what it destroyed in a minute");
    Ok(())
}

/// The space the snapshots of the boot environment hold.
fn used_by_snapshots(vm: &mut Vm<'_>) -> Result<u64> {
    let (_, output) = vm.sh_show(&format!("zfs get -Hp -o value usedbysnapshots {BE}"))?;
    bytes(&output).ok_or_else(|| Error::Setup(format!("no usedbysnapshots in {output:?}")))
}

/// Before the destroy of `one` and `three`: what their snapshots hold, and
/// what the destroy would free.
///
/// # Errors
///
/// When the shell does not answer, or says no number.
pub(super) fn space_before(test: &Test, vm: &mut Vm<'_>) -> Result<Space> {
    settle(test, vm)?;
    let (code, output) = vm.sh_show(&format!("zfs destroy -nvp {BE}@one,three"))?;
    let reclaim = reclaim(&output).filter(|_| code == 0);
    test.check(
        reclaim.is_some(),
        "zfs destroy -nvp says what destroying one and three frees",
    );
    Ok(Space {
        snapshots: used_by_snapshots(vm)?,
        reclaim: reclaim.unwrap_or_default(),
    })
}

/// After the destroy of `one` and `three`: the space the snapshots hold
/// fell by what the dry run said it would free (ROADMAP.md #15).
///
/// # Errors
///
/// When the shell does not answer, or says no number.
pub(super) fn space_after(test: &Test, vm: &mut Vm<'_>, before: Space) -> Result<()> {
    settle(test, vm)?;
    let after = used_by_snapshots(vm)?;
    let fell = before.snapshots.saturating_sub(after);
    test.check(
        fell == before.reclaim,
        &format!(
            "destroying one and three freed what the dry run said, {} B \
             (usedbysnapshots {} -> {after})",
            before.reclaim, before.snapshots
        ),
    );
    Ok(())
}

/// The datasets, snapshots and bookmarks of `tmenu`, with their origins,
/// as `zfs list -H -o name,origin` prints them.
fn datasets(output: &str) -> Vec<(&str, &str)> {
    output
        .lines()
        .filter_map(|line| line.trim_end().split_once('\t'))
        .collect()
}

/// What the snapshot tasks left: the clone and promote, the clone and the
/// copy, the snapshots destroyed and those kept, the bookmark of a
/// destroyed snapshot, and the restore's safety snapshot.
///
/// # Errors
///
/// When the shell does not answer.
pub(super) fn snapshots_left(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let (_, output) = vm.sh_show("zfs list -H -t all -o name,origin -r tmenu")?;
    let listed = datasets(&output);
    let origin = |name: &str| listed.iter().find(|(n, _)| *n == name).map(|(_, o)| *o);
    let has = |wanted: &dyn Fn(&str) -> bool| listed.iter().any(|(name, _)| wanted(name));
    let gone = ["@one", "@three", "@five", "@menu-new"];
    let checks = [
        (
            origin("tmenu/ROOT/promoted") == Some("-")
                && origin(BE) == Some("tmenu/ROOT/promoted@four"),
            "Clone and Promote made tmenu/ROOT/promoted, of which be is now a clone",
        ),
        (
            ["two", "held", "four"]
                .iter()
                .all(|name| origin(&format!("tmenu/ROOT/promoted@{name}")).is_some()),
            "the promoted clone holds two, held and four",
        ),
        (
            origin("tmenu/ROOT/cloned") == Some("tmenu/ROOT/promoted@two")
                && origin("tmenu/two-clone") == Some("tmenu/ROOT/promoted@two"),
            "the clones of two, cloned among them, follow two to the promoted clone",
        ),
        (
            origin("tmenu/ROOT/copied") == Some("-"),
            "the copy of four is independent",
        ),
        (
            !has(&|name| gone.iter().any(|end| name.ends_with(end))),
            "one, three, five and menu-new are gone",
        ),
        (
            has(&|name| name.ends_with("#three")),
            "the bookmark of three is kept",
        ),
        (
            has(&|name| name.starts_with(&format!("{BE}@before-restore-"))),
            "the restore took a safety snapshot",
        ),
    ];
    for (passed, what) in checks {
        test.check(passed, what);
    }
    Ok(())
}

/// The command that prints `restore-me` and lists `removed` in the boot
/// environment, mounted read-only where the shell can read it.
const FILES: &str = "m=/mnt/be; grep -q ' /mnt/be ' /proc/mounts || \
    { mkdir -p $m && mount -o zfsutil,ro -t zfs tmenu/ROOT/be $m; }; \
    cat $m/zbmtest/restore-me && ls -ln $m/zbmtest/removed";

/// What the restore brought back: the content of `restore-me`, and
/// `removed` with its owner and mode (ROADMAP.md #21).
///
/// # Errors
///
/// When the shell does not answer.
pub(super) fn files_restored(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let (code, output) = vm.sh_show(FILES)?;
    test.check(
        code == 0 && output.lines().any(|line| line.trim_end() == RESTORED),
        "restore-me has its content of one again",
    );
    test.check(
        output.lines().any(|line| line.starts_with(REMOVED)),
        "removed is back, owned by 1234:5678 with the mode 0640",
    );
    Ok(())
}

/// Whether each pool of `zpool get -H readonly`'s output is read-only.
fn read_only(output: &str) -> Vec<(&str, bool)> {
    output
        .lines()
        .filter_map(
            |line| match line.split_whitespace().collect::<Vec<_>>().as_slice() {
                [pool, "readonly", value, ..] => Some((*pool, *value == "on")),
                _ => None,
            },
        )
        .collect()
}

/// What the pool tasks left: the access of each pool, the rewind, the host
/// ID taken from `tforeign`'s last host, and the volume open.
///
/// # Errors
///
/// When the shell does not answer.
pub(super) fn pools_left(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let (_, output) = vm.sh_show("zpool get -H readonly")?;
    let access = read_only(&output);
    let is = |pool: &str, read_only: bool| access.contains(&(pool, read_only));
    test.check(is("tother", false), "the rewind imported tother read/write");
    test.check(is("tforeign", true), "tforeign is imported read-only");
    test.check(is("tlocked", true), "tlocked is imported read-only");
    let (code, _) = vm.sh("zfs list tother/after")?;
    test.check(
        code != 0,
        "the rewind lost tother/after, made after the checkpoint",
    );
    let (code, status) = vm.sh_show("zpool status tother")?;
    test.check(
        code == 0 && !status.contains("checkpoint:"),
        "the rewind discarded tother's checkpoint",
    );
    let (_, hostid) = vm.sh_show("od -An -tx4 /etc/hostid")?;
    test.check(
        hostid.contains("deadbeef"),
        "the import with tforeign's last host ID took deadbeef",
    );
    let (code, _) = vm.sh(&format!("[ -e /dev/mapper/luks-{UUID} ]"))?;
    test.check(code == 0, "the volume the menu unlocked is open");
    Ok(())
}

#[cfg(test)]
mod tests;
