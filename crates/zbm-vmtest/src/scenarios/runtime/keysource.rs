//! `runtime-legacy-keysource`: the boot environment of `tkeys` (d2) is
//! encrypted with a key file that the image does not have; its key source
//! `tkeys/keystore` holds it. The legacy runtime asks once for the key
//! source's passphrase, copies the key file into `/zfsbootmenu/.keys`,
//! unlocks the boot environment with it and boots it; shadow mode must
//! compare where the key came from and find what the Rust code decides.
//!
//! Only the legacy runtime: the Rust runtime does not load the keys of
//! encryption roots yet. The boot environment itself is not followed, as
//! its initramfs has no key file either.

use crate::boot::{Follow, follow_boot};
use crate::error::Result;
use crate::expect::regex;
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

/// The command line of the boot.
const APPEND: &str = "console=ttyS0 zbm.timeout=3 loglevel=7";

/// The summary of a shadow mode run that compared the key source.
const COMPARED: &str = r"ZBM-SHADOW: \d+ differences in \d+ decisions \([^)\n]*key source";

/// Boots `tkeys`'s boot environment with the legacy runtime.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let image = super::image(settings, "auto", ZbmRuntime::Legacy, None)?;
    let machine = Machine::boot(&image, APPEND).with_disks(vec![super::disk(settings, 2)?]);
    let mut vm = Vm::start(test, "boot", &machine)?;
    let follow = Follow {
        no_be: true,
        timeout: Some(1200),
        ..Follow::default()
    };
    let boot = follow_boot(&mut vm, follow)?;
    vm.kill();
    let log = super::serial_text(&vm)?;
    check(test, &boot.prompts, &log);
    Ok(())
}

/// Checks the prompts and the console of the boot.
fn check(test: &Test, prompts: &[String], log: &str) {
    test.check(
        prompts == ["zfskey"],
        &format!("one passphrase prompt (prompts: {prompts:?})"),
    );
    test.check(
        log.contains("passphrase for 'tkeys/keystore'"),
        "the passphrase asked for is the key source's",
    );
    test.check(
        log.contains("Booting") && log.contains("for tkeys/ROOT/be"),
        "the boot environment, unlocked with the cached key, is booted",
    );
    test.check(
        regex(COMPARED).is_ok_and(|compared| compared.is_match(log)),
        "shadow mode compared the key source",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{checks, cleanup, settings};

    #[test]
    fn the_boot_asks_for_the_key_source_and_shadow_mode_compares_it() {
        let settings = settings("runtime-keysource");
        let test = Test::new(&settings, "runtime-legacy-keysource").unwrap();
        let log = "Enter passphrase for 'tkeys/keystore':\n\
                   Booting /boot/vmlinuz-7.3 for tkeys/ROOT/be in 3 seconds\n\
                   ZBM-SHADOW: 0 differences in 4 decisions (pools, default boot environment, \
                   kernel, key source)\n";
        check(&test, &["zfskey".to_owned()], log);
        check(
            &test,
            &[],
            "ZBM-SHADOW: 0 differences in 2 decisions (pools, kernel)\n",
        );

        let results: Vec<bool> = checks(&test)
            .iter()
            .map(|line| line.starts_with("ok"))
            .collect();
        assert_eq!(
            results,
            [true, true, true, true, false, false, false, false]
        );
        cleanup(&settings);
    }
}
