use pretty_assertions::assert_eq;

use super::*;
use crate::scenarios::menu::tests::{all_ok, checked, said};
use crate::testing::Step as Play;

/// The shell runs every command of the setup, and lists `pools`.
fn made(pools: &str) -> Vec<Play> {
    let mut script: Vec<Play> = commands()
        .iter()
        .flat_map(|command| said(command, 0, ""))
        .collect();
    script.extend(said("finish", 0, "control"));
    script.extend(said(&foreign_import(), 0, ""));
    script.extend(said(
        "od -An -tx4 /etc/hostid; zpool list -H -o name",
        0,
        &format!(" deadbeef\n{pools}"),
    ));
    script
}

#[test]
fn the_setup_makes_the_pools_and_leaves_tforeign_to_the_other_host() {
    let found = checked("setup", made("tforeign"), make_pools);

    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(all_ok(&found), "{found:#?}");
}

#[test]
fn another_pool_left_imported_fails_the_setup() {
    let found = checked("setup-tmenu", made("tforeign\ntmenu"), make_pools);

    assert_eq!(
        found,
        ["FAIL - tforeign imported by host deadbeef, and no other pool"]
    );
}

#[test]
fn a_command_that_fails_stops_the_setup() {
    let script = said(&commands()[0], 1, "no such device");
    let settings = crate::testing::settings("menu-setup-fails");
    let test = Test::new(&settings, "menu-setup").unwrap();
    let (mut vm, guest) = crate::testing::scripted(&test, script);

    assert!(make_pools(&test, &mut vm).is_err());
    drop(vm);
    assert_eq!(guest.finish(), Vec::<String>::new());
    crate::testing::cleanup(&settings);
}

/// Where in the commands the first that holds `text` is.
fn at(text: &str) -> usize {
    commands()
        .iter()
        .position(|command| command.contains(text))
        .unwrap()
}

#[test]
fn the_files_change_between_three_and_four_so_that_the_rollback_keeps_it() {
    assert!(at("@three &&") < at("rm /be/zbmtest/removed"));
    assert!(at("rm /be/zbmtest/removed") < at("@four"));
    assert!(at("@four") < at("@five"));
    assert!(at("vmlinuz-0.0.1-zbmtest") < at("@one"));
    assert!(at("chown 1234:5678") < at("@one"));
    assert!(at("@one") < at("@two &&") && at("@two &&") < at("@held &&"));
}

#[test]
fn the_volume_is_made_fast_to_open_with_its_fixed_uuid() {
    let luks = &commands()[at("mkluks")];

    assert!(luks.contains("--pbkdf pbkdf2 --pbkdf-force-iterations 1000"));
    assert!(luks.contains(&format!("--uuid {UUID}")));
    assert!(luks.starts_with("mkluks pw "));
    assert!(at("mkluks") < at("tlocked /dev/mapper/luks-"));
}

#[test]
fn tmenu_has_room_for_a_duplicate_of_its_boot_environment() {
    let settings = crate::testing::settings("menu-setup-disks");
    let stale = settings.disks(GROUP).join("disk0.img");
    std::fs::create_dir_all(settings.disks(GROUP)).unwrap();
    std::fs::write(&stale, "stale").unwrap();

    let disks = empty_disks(&settings).unwrap();

    let serials: Vec<&str> = disks.iter().map(|disk| disk.serial.as_str()).collect();
    assert_eq!(serials, ["d0", "d1", "d2", "d3"]);
    let sizes: Vec<u64> = disks
        .iter()
        .map(|disk| std::fs::metadata(&disk.path).unwrap().len())
        .collect();
    assert_eq!(sizes, [1 << 30, 384 << 20, 384 << 20, 384 << 20]);
    crate::testing::cleanup(&settings);
}
