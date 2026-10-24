//! `runtime-legacy-keysource`: the boot environment of `tkeys` (d2) is
//! encrypted with a key file that the image does not have; its key source
//! `tkeys/keystore` holds it. Two boots, each stopped before the boot
//! environment runs, as its initramfs has no key file either:
//!
//! - `countdown`: the countdown's boot skips key sources, so the legacy
//!   runtime asks for the boot environment's own passphrase (the content
//!   of the key file);
//! - `menu`: a boot chosen in the menu tries the key source: the legacy
//!   runtime asks for the key source's passphrase, copies the key file into
//!   `/zfsbootmenu/.keys` and unlocks the boot environment with it.
//!
//! Each time, shadow mode must compare where the key came from without a
//! difference. Only the legacy runtime: the Rust runtime does not load the
//! keys of encryption roots yet.

use crate::boot::{Follow, Kind, follow_boot};
use crate::error::Result;
use crate::expect::regex;
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::{ZFSKEY, ZbmRuntime};
use crate::vm::Vm;

use super::setup::BE_KEY;

/// The summary of a shadow mode run that compared the key source.
const COMPARED: &str = r"ZBM-SHADOW: \d+ differences in \d+ decisions \([^)\n]*key source";

/// A boot of the scenario.
struct Boot {
    /// The label of the machine, and of its serial log.
    label: &'static str,
    /// The kernel command line.
    append: &'static str,
    /// The passphrase answered, and the dataset that must ask for it.
    answer: &'static str,
    asker: &'static str,
}

/// The two boots.
const BOOTS: [Boot; 2] = [
    Boot {
        label: "countdown",
        append: "console=ttyS0 zbm.timeout=3 loglevel=7",
        answer: BE_KEY,
        asker: "tkeys/ROOT/be",
    },
    Boot {
        label: "menu",
        append: "console=ttyS0 zbm.show loglevel=7",
        answer: ZFSKEY,
        asker: "tkeys/keystore",
    },
];

/// Boots `tkeys`'s boot environment from the countdown, then from the menu,
/// with the legacy runtime.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let image = super::image(settings, "auto", ZbmRuntime::Legacy, None)?;
    for boot in &BOOTS {
        let machine =
            Machine::boot(&image, boot.append).with_disks(vec![super::disk(settings, 2)?]);
        let mut vm = Vm::start(test, boot.label, &machine)?;
        let follow = Follow {
            no_be: true,
            menu: boot.label == "menu",
            timeout: Some(1200),
            ..Follow::default()
        }
        .answer(Kind::ZfsKey, &[boot.answer]);
        let followed = follow_boot(&mut vm, follow)?;
        vm.kill();
        let log = super::serial_text(&vm)?;
        check(test, boot, &followed.prompts, &log);
    }
    Ok(())
}

/// Checks the prompts and the console of `boot`.
fn check(test: &Test, boot: &Boot, prompts: &[String], log: &str) {
    let label = boot.label;
    let keys: Vec<&String> = prompts.iter().filter(|prompt| *prompt != "menu").collect();
    test.check(
        keys == ["zfskey"],
        &format!("{label}: one passphrase prompt (prompts: {prompts:?})"),
    );
    test.check(
        log.contains(&format!("passphrase for '{}'", boot.asker)),
        &format!("{label}: the passphrase asked for is {}'s", boot.asker),
    );
    test.check(
        regex(COMPARED).is_ok_and(|compared| compared.is_match(log)),
        &format!("{label}: shadow mode compared the key source"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{checks, cleanup, settings};

    #[test]
    fn each_boot_asks_its_passphrase_and_shadow_mode_compares_the_key_source() {
        let settings = settings("runtime-keysource");
        let test = Test::new(&settings, "runtime-legacy-keysource").unwrap();
        let log = "Enter passphrase for 'tkeys/keystore':\n\
                   ZBM-SHADOW: 0 differences in 4 decisions (pools, default boot environment, \
                   kernel, key source)\n";
        let prompts = ["menu".to_owned(), "zfskey".to_owned()];
        check(&test, &BOOTS[1], &prompts, log);
        check(
            &test,
            &BOOTS[0],
            &[],
            "ZBM-SHADOW: 0 differences in 2 decisions (pools, kernel)\n",
        );

        let results: Vec<bool> = checks(&test)
            .iter()
            .map(|line| line.starts_with("ok"))
            .collect();
        assert_eq!(results, [true, true, true, false, false, false]);
        cleanup(&settings);
    }
}
