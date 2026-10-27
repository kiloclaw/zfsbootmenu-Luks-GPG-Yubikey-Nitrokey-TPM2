//! `runtime-rust-pilot`: the first pilot of `ROADMAP.md` (R0) in
//! the Rust image, on the serial console of the recovery shell: `zluks
//! --pilot open` on a LUKS volume formatted in the VM. The second, the
//! boot environment list, gave way to the menu itself (R5 step 3), which
//! the other scenarios drive.

use crate::error::Result;
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

/// The key of the pilot's LUKS volume.
const PILOT_KEY: &str = "zbm-pilot-key-1234";

/// Runs the pilot.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let image = super::image(settings, "auto", ZbmRuntime::Rust, None)?;
    let machine =
        Machine::boot(&image, super::ESCAPE_APPEND).with_disks([super::disk(settings, 0)?]);
    let mut vm = Vm::start(test, "boot", &machine)?;
    super::escape_to_shell(&mut vm)?;
    unlock(&mut vm)?;
    vm.kill();
    Ok(())
}

/// Formats a LUKS volume on a loop device and writes the key files.
fn format_command() -> String {
    format!(
        "if command -v truncate >/dev/null; then truncate -s 32M /tmp/pilot.img; \
         elif command -v fallocate >/dev/null; then fallocate -l 32M /tmp/pilot.img; \
         else head -c 33554432 /dev/zero > /tmp/pilot.img; fi && \
         dev=$(losetup -f --show /tmp/pilot.img) && \
         printf %s {PILOT_KEY} | cryptsetup luksFormat --batch-mode --type luks2 --pbkdf pbkdf2 \
         --pbkdf-force-iterations 1000 --key-file=- \"$dev\" && \
         printf %s {PILOT_KEY} > /tmp/pilot.key && printf wrong > /tmp/wrong.key && echo DEV=$dev"
    )
}

/// `zluks --pilot open` with the key file `key`.
fn open_command(key: &str) -> String {
    format!(
        "zluks --pilot open \"$dev\" zbm-pilot --key-file /tmp/{key}.key \
         --journal /tmp/pilot.jsonl"
    )
}

/// `zluks --pilot open` on a LUKS volume formatted in the VM.
fn unlock(vm: &mut Vm<'_>) -> Result<()> {
    let test = vm.test();
    let (rc, out) = vm.sh_show("command -v losetup && command -v cryptsetup")?;
    if rc != 0 {
        test.skip(&format!(
            "pilot 1: the image lacks losetup or cryptsetup ({out})"
        ));
        return Ok(());
    }
    // The image has no dd; make the 32 MiB file with whichever tool it has.
    let (rc, _) = vm.sh_show(&format_command())?;
    test.check(rc == 0, "a LUKS volume was formatted on a loop device");
    if rc != 0 {
        return Ok(());
    }
    let (rc, out) = vm.sh_show(&open_command("wrong"))?;
    test.check(
        rc == 1 && out.contains("no key slot of"),
        "a wrong key file is refused, with the reason",
    );
    let (rc, out) = vm.sh_show(&open_command("pilot"))?;
    test.check(
        rc == 0 && out.contains("as /dev/mapper/zbm-pilot (key file)"),
        "zluks --pilot open opens the volume with the key file",
    );
    let (rc, _) = vm.sh("test -b /dev/mapper/zbm-pilot")?;
    test.check(rc == 0, "/dev/mapper/zbm-pilot exists");
    let (_, out) = vm.sh_show("cat /tmp/pilot.jsonl")?;
    check_journal(test, &out);
    vm.sh("cryptsetup close zbm-pilot; losetup -d \"$dev\"")
        .map(drop)
}

/// Checks the journal of the two attempts.
fn check_journal(test: &Test, journal: &str) {
    test.check(
        journal.contains(r#""stdin":"secret""#),
        "the journal records that the key went to standard input",
    );
    test.check(
        !journal.contains(PILOT_KEY),
        "the journal does not contain the key",
    );
    test.check(
        journal.contains(r#""message":"refused""#) && journal.contains(r#""message":"opened""#),
        "the journal records the refused and the successful attempt",
    );
}

#[cfg(test)]
mod tests;
