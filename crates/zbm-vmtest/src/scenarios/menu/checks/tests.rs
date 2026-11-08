use pretty_assertions::assert_eq;

use super::*;
use crate::scenarios::menu::tests::{
    LEFT, all_ok, checked, pools_left_said, said, snapshots_left_said, space_after_said,
    space_before_said,
};

#[test]
fn the_space_freed_is_what_the_dry_run_said() {
    for (after, passed) in [(4096, true), (8192, false)] {
        let mut script = space_before_said(8192, 12288);
        script.extend(space_after_said(after));

        let found = checked(&format!("space-{after}"), script, |test, vm| {
            let before = space_before(test, vm)?;
            assert_eq!(
                before,
                Space {
                    snapshots: 12288,
                    reclaim: 8192
                }
            );
            space_after(test, vm, before)
        });

        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].starts_with("ok"), "{found:#?}");
        assert_eq!(found[1].starts_with("ok"), passed, "{found:#?}");
    }
}

#[test]
fn a_dry_run_without_its_reclaim_fails() {
    let mut script = said("zpool sync tmenu", 0, "");
    script.extend(said("zpool get -Hp -o value freeing tmenu", 0, "0"));
    script.extend(said(
        "zfs destroy -nvp tmenu/ROOT/be@one,three",
        1,
        "could not find any snapshots to destroy",
    ));
    script.extend(said(
        "zfs get -Hp -o value usedbysnapshots tmenu/ROOT/be",
        0,
        "-",
    ));

    let settings = crate::testing::settings("menu-checks-no-reclaim");
    let test = Test::new(&settings, "menu-snapshots").unwrap();
    let (mut vm, guest) = crate::testing::scripted(&test, script);
    let before = space_before(&test, &mut vm);
    drop(vm);

    assert!(before.is_err());
    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(
        crate::testing::checks(&test),
        ["FAIL - zfs destroy -nvp says what destroying one and three frees"]
    );
    crate::testing::cleanup(&settings);
}

#[test]
fn the_snapshot_tasks_leave_the_clones_the_copy_and_the_bookmark() {
    let found = checked("snapshots-left", snapshots_left_said(LEFT), |test, vm| {
        snapshots_left(test, vm)?;
        files_restored(test, vm)
    });

    assert_eq!(found.len(), 9, "{found:#?}");
    assert!(all_ok(&found), "{found:#?}");
}

#[test]
fn a_snapshot_left_behind_or_a_bookmark_gone_fails() {
    let listed = LEFT.replace("tmenu/ROOT/be#three\t-\n", "tmenu/ROOT/be@one\t-\n");

    let found = checked(
        "snapshots-wrong",
        snapshots_left_said(&listed),
        |test, vm| {
            snapshots_left(test, vm)?;
            files_restored(test, vm)
        },
    );

    let failed: Vec<&String> = found
        .iter()
        .filter(|line| line.starts_with("FAIL"))
        .collect();
    assert_eq!(
        failed,
        [
            "FAIL - one, three, five and menu-new are gone",
            "FAIL - the bookmark of three is kept"
        ]
    );
}

#[test]
fn the_pool_tasks_leave_the_pools_imported_rewound_and_as_deadbeef() {
    let found = checked("pools-left", pools_left_said(), pools_left);

    assert_eq!(found.len(), 7, "{found:#?}");
    assert!(all_ok(&found), "{found:#?}");
}

#[test]
fn the_shell_s_output_is_read_as_zfs_and_zpool_print_it() {
    assert_eq!(reclaim("destroy\ta@b\nreclaim\t4096\n"), Some(4096));
    assert_eq!(reclaim("destroy\ta@b\n"), None);
    assert_eq!(bytes(" 12\n"), Some(12));
    assert_eq!(bytes("-"), None);
    assert_eq!(
        read_only("tmenu\treadonly\ton\t-\ntother\treadonly\toff\tdefault\nnoise"),
        [("tmenu", true), ("tother", false)]
    );
    assert_eq!(
        datasets("a\t-\na@b\t-\nc\ta@b\n"),
        [("a", "-"), ("a@b", "-"), ("c", "a@b")]
    );
}
