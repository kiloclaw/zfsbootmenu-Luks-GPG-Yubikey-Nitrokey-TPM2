//! `integrity-locked` (ROADMAP.md SE9, ADR 0010): the locked menu. The
//! integrity image built with a lock (`zfsbootmenu_lock_hash`, the hash of
//! [`PIN`]) boots twice with the TPM:
//!
//! - with `zbm.prefer=tauto`: the default boot needs nothing, the lock
//!   notwithstanding, and `tauto`'s boot environment boots by itself;
//! - with `zbm.prefer=tplain`, which waits in the menu: the toolbar marks
//!   what the lock keeps with the PIN (ADR 0010); the recovery shell
//!   asks for the PIN, refuses a wrong one, opens with the right one after
//!   the delay a wrong one adds; once it ended, the menu is locked again:
//!   booting `tauto`'s environment, which is not the default, asks for the
//!   PIN on the console, and boots once it is typed.

use std::time::{Duration, Instant};

use super::{BASE, image_with, start_with};
use crate::boot::{Boot, Follow, check_be, follow_boot, prompt};
use crate::error::Result;
use crate::record::Test;
use crate::vm::Vm;

/// The PIN of the image.
pub(super) const PIN: &str = "zbmtest-lock-PIN";

/// The configuration that locks the image with [`PIN`]: its hash, which
/// `zbm lock hash` printed for it.
pub(super) const LOCK: &str = "zfsbootmenu_lock_hash='$argon2id$v=19$m=65536,t=3,p=1$\
    r6QDED1MEmUmc9/Tvs9nNg$Nj5kvd7Af5S9LCcd1vFRrLt+Kad6YiItccO7ZVD51EU'";

/// What `zbm lock shell` says first.
const LOCKED_SHELL: &str = "This ZFSBootMenu is locked. Type its PIN";

/// What the session asks before an action the lock keeps.
const LOCKED_ACTION: &str = "This ZFSBootMenu is locked. PIN";

/// The prompt of the PIN, after which it is typed.
const PIN_PROMPT: &str = "PIN: ";

/// The toolbar's edit of the command line, which the lock keeps, marked.
const LOCK_MARK: &str = r"screen:Edit PIN";

/// The scenario.
///
/// # Errors
///
/// When a step cannot be done.
pub fn run(test: &Test) -> Result<()> {
    let image = image_with(test.settings(), "integrity-locked", LOCK)?;
    let append = format!("{BASE} zbm.prefer=tauto");
    let mut vm = start_with(
        test,
        "locked-default",
        (&append, &[0, 1, 2]),
        (true, None),
        &image,
    )?;
    let boot = follow_boot(&mut vm, Follow::default())?;
    check_default(test, &boot, &super::serial_text(&vm)?);
    check_be(test, &boot, "tauto", &["tauto"], &[]);
    vm.kill();

    let append = format!("{BASE} zbm.prefer=tplain");
    let mut vm = start_with(
        test,
        "locked-menu",
        (&append, &[0, 1, 2]),
        (true, None),
        &image,
    )?;
    vm.expect1(prompt::NEW_MENU, 1200)?;
    marked(test, &mut vm);
    shell_behind_the_pin(test, &mut vm)?;
    let boot = boot_behind_the_pin(test, &mut vm)?;
    check_be(test, &boot, "tauto", &["tauto"], &[]);
    Ok(())
}

/// The checks of the default boot, on its `boot` and serial `log`.
pub(super) fn check_default(test: &Test, boot: &Boot, log: &str) {
    test.check(
        !log.contains(LOCKED_ACTION) && !boot.has_prompt("luks"),
        "the locked image boots its default by itself, nothing asked",
    );
}

/// The locked menu marks on its toolbar what the PIN keeps, before any
/// PIN is typed: the edit of the command line.
pub(super) fn marked(test: &Test, vm: &mut Vm<'_>) {
    let marked = vm.expect1(LOCK_MARK, 60).is_ok();
    test.check(
        marked,
        "the locked menu's toolbar marks the edit with the PIN",
    );
}

/// The recovery shell, opened from the menu, asks for the PIN: a wrong one
/// is refused, the right one opens it after the delay, and the shell takes
/// commands; it ends, and the menu shows again.
pub(super) fn shell_behind_the_pin(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    crate::boot::ask_menu_for_shell(vm)?;
    let asked = vm.expect1(LOCKED_SHELL, 120).is_ok();
    test.check(asked, "the recovery shell asks for the PIN");
    vm.expect1(PIN_PROMPT, 30)?;
    vm.sendline("not the PIN")?;
    let refused = vm.expect1("Wrong PIN.", 120).is_ok();
    test.check(refused, "a wrong PIN is refused");
    vm.expect1(PIN_PROMPT, 30)?;
    let sent = Instant::now();
    vm.sendline(PIN)?;
    vm.shell_ready()?;
    test.check(
        sent.elapsed() >= Duration::from_secs(1),
        &format!(
            "the right PIN, after a wrong one, is answered after a delay ({:?})",
            sent.elapsed()
        ),
    );
    let (_, output) = vm.sh("echo ZBMTEST-LOCKED-$((6*7))")?;
    test.check(
        output.contains("ZBMTEST-LOCKED-42"),
        "the right PIN opens the recovery shell",
    );
    vm.sendline("exit")?;
    vm.expect1(prompt::NEW_MENU, 600)?;
    Ok(())
}

/// Booting the environment above the default, `tauto`'s, asks for the PIN
/// on the console, and boots once it is typed: the boot.
pub(super) fn boot_behind_the_pin(test: &Test, vm: &mut Vm<'_>) -> Result<Boot> {
    std::thread::sleep(Duration::from_secs(2));
    vm.send("\x1b[A")?;
    std::thread::sleep(Duration::from_secs(1));
    vm.send("\r")?;
    let asked = vm.expect1(LOCKED_ACTION, 120).is_ok();
    test.check(
        asked,
        "booting an environment other than the default asks for the PIN again, on the console",
    );
    vm.sendline(PIN)?;
    follow_boot(vm, Follow::default())
}

#[cfg(test)]
mod tests;
