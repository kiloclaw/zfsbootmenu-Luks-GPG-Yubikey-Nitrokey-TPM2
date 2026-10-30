use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{checks, cleanup, settings};

#[test]
fn the_other_host_id_is_written_little_endian() {
    assert_eq!(
        foreign_import(),
        r"printf '\xef\xbe\xad\xde' > /etc/hostid && zpool import -N tforeign"
    );
}

/// What the boot of `name` showed: ZFSBootMenu's pool status and prompts.
fn boot(status: &str, prompts: &[&str]) -> Boot {
    Boot {
        zbm_status: status.into(),
        prompts: prompts.iter().map(|&prompt| prompt.to_owned()).collect(),
        ..Boot::default()
    }
}

#[test]
fn a_volume_that_unlocked_itself_holds_start_up_to_its_pool() {
    let settings = settings("integrity-unattended-checks");
    let test = Test::new(&settings, "integrity-unattended").unwrap();
    let status = "  pool: tauto\n state: ONLINE\n  pool: tplain\n state: ONLINE";
    let log = format!("ZFSBootMenu: the volumes unlocked themselves: {HELD}\n");

    check_unlocked_itself(&test, &boot(status, &[]), &log);
    // Asked, not held, and the other host's pool adopted: every check fails.
    let adopted = format!("{status}\n  pool: tforeign\n state: ONLINE");
    check_unlocked_itself(&test, &boot(&adopted, &["tpm2pin"]), "");
    check_waits(
        &test,
        &format!("ZFSBootMenu: tplain/ROOT/be {WAITS}: it boots only when chosen\n"),
    );
    check_waits(&test, "Booting vmlinuz for tplain/ROOT/be\nBE-KERNEL 7.3\n");
    check_typed(&test, &boot(status, &["luks"]), "");
    check_typed(&test, &boot(status, &[]), &log);

    let results: Vec<bool> = checks(&test)
        .iter()
        .map(|line| line.starts_with("ok"))
        .collect();
    assert_eq!(&results[..3], [true, true, true]);
    assert_eq!(&results[3..6], [false, false, false]);
    assert_eq!(&results[6..8], [true, true]);
    assert_eq!(&results[8..10], [false, false]);
    assert_eq!(&results[10..12], [true, true]);
    assert_eq!(&results[12..], [false, false]);
    cleanup(&settings);
}

#[test]
fn under_lockdown_the_kernel_must_not_load() {
    let settings = settings("integrity-lockdown-checks");
    let test = Test::new(&settings, "integrity-lockdown").unwrap();
    let log = format!("ZFSBootMenu: {REFUSED} /mnt/boot/vmlinuz\n{NOT_LOADED}\n");

    check_lockdown(&test, true, &log);
    check_lockdown(&test, false, "BE-KERNEL 7.3\n");

    let results: Vec<bool> = checks(&test)
        .iter()
        .map(|line| line.starts_with("ok"))
        .collect();
    assert_eq!(results, [true, true, false, false]);
    cleanup(&settings);
}
