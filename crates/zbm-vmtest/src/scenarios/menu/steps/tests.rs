use pretty_assertions::assert_eq;

use super::*;
use crate::scenarios::menu::tests::{TABLES, all_ok, checked, expected, played, screen, serial};

/// Runs the table `steps` on a menu that shows what it waits for; returns
/// its checks.
fn ran(name: &str, steps: &'static [Step]) -> Vec<String> {
    checked(&format!("steps-{name}"), played(steps), |test, vm| {
        run(test, &mut serial(), vm, steps)
    })
}

/// Runs the table of `index` and checks that each of its checks passed.
fn plays(index: usize) {
    let (name, steps) = TABLES[index];

    let found = ran(name, steps);

    assert_eq!(found.len(), expected(steps), "{name}: {found:#?}");
    assert!(found.is_empty() || all_ok(&found), "{name}: {found:#?}");
}

#[test]
fn the_shell_and_power_off_play_on_the_menu() {
    plays(0);
    plays(1);
}

#[test]
fn the_snapshot_changes_play_on_the_menu() {
    plays(2);
}

#[test]
fn the_destroy_of_the_marked_snapshots_plays_on_the_menu() {
    plays(3);
}

#[test]
fn the_refused_destroys_and_the_promote_play_on_the_menu() {
    plays(4);
}

#[test]
fn the_pool_tasks_play_on_the_menu() {
    plays(5);
}

#[test]
fn the_palette_tour_and_the_boots_play_on_the_menu() {
    plays(6);
    plays(7);
    plays(8);
}

#[test]
fn the_display_s_way_back_to_the_boot_tab_and_its_reboot_play_on_the_menu() {
    plays(9);
    plays(10);
}

#[test]
fn a_menu_that_stops_ends_the_steps_with_an_error() {
    let steps: &'static [Step] = &SNAPSHOTS_CHANGES[..1];
    let mut script = played(steps);
    script.pop();

    let settings = crate::testing::settings("menu-steps-stopped");
    let test = Test::new(&settings, "menu-snapshots").unwrap();
    let (mut vm, guest) = crate::testing::scripted(&test, script);
    let ended = run(&test, &mut serial(), &mut vm, steps);
    drop(vm);

    assert!(ended.is_err());
    assert_eq!(guest.finish(), Vec::<String>::new());
    crate::testing::cleanup(&settings);
}

#[test]
fn a_screen_that_lacks_what_a_step_shows_fails_its_checks() {
    let step = Step::new("a step", &[], Await::Done)
        .showing(&["Type be to confirm:"])
        .hiding(&["Enter Restore"])
        .listing(&["two"]);
    let shown = screen(&["  √ Failed: no", "│   two-clone │", "Enter Restore"]);
    let shown = shown.replace("\x1b[H\x1b[2J", "").replace('\r', "");

    let found = checked("steps-lacking", Vec::new(), |test, _| {
        check(test, &step, &shown);
        Ok(())
    });

    assert_eq!(
        found,
        [
            "FAIL - a step: done",
            "FAIL - a step: the menu shows \"Type be to confirm:\"",
            "FAIL - a step: the menu does not show \"Enter Restore\"",
            "FAIL - a step: the menu lists two",
        ]
    );
}

#[test]
fn a_screen_is_matched_with_any_blank_between_its_words() {
    let wrapped = "│ Type be │\n│ to confirm: │";
    let pattern = Step::new("", &[], Await::Text("Type be to confirm:"))
        .pattern()
        .unwrap();
    assert!(
        regex::Regex::new(&pattern)
            .unwrap()
            .is_match("Type  be\nto confirm:")
    );
    assert!(!regex::Regex::new(&pattern).unwrap().is_match(wrapped));
    assert_eq!(Step::new("", &[], Await::Nothing).pattern(), None);
    assert_eq!(
        Step::new("", &[], Await::Done).pattern().as_deref(),
        Some(OUTCOME)
    );
}

#[test]
fn a_row_holds_the_pool_as_a_whole_word_then_its_state() {
    let pattern = Step::new("", &[], Await::Row("tother", "read-only"))
        .pattern()
        .unwrap();
    let row = regex::Regex::new(&pattern).unwrap();

    assert!(row.is_match("│ ▶ tother          read-only · ONLINE │"));
    assert!(row.is_match("│   tmenu   x │\n│   tother    read-only │"));
    assert!(!row.is_match("│ ▶ tother/after    read-only │"));
    assert!(!row.is_match("│ ▶ tother   read/write │\n│ tmenu   read-only │"));
    assert!(!row.is_match("┌ tother ───┐\n│ Health  ONLINE, read-only │"));
    // On a wide screen, the inspector beside the list, on the row's line.
    assert!(!row.is_match("│   tother   not imported ││ Health  ONLINE, read-only │"));
}

/// The name a step asks for: the `X` of "Type X to confirm:".
fn asked(step: &Step) -> Option<&'static str> {
    step.shows
        .iter()
        .find_map(|text| text.strip_prefix("Type ")?.strip_suffix(" to confirm:"))
}

#[test]
fn each_name_asked_for_is_typed_by_the_next_step() {
    for (name, steps) in TABLES {
        for (step, next) in steps.iter().zip(steps.iter().skip(1)) {
            let Some(asked) = asked(step) else {
                continue;
            };
            assert_eq!(
                next.acts.first(),
                Some(&Act::Type(asked)),
                "{name}: {}",
                step.what
            );
        }
    }
}

#[test]
fn the_forced_import_is_read_only_so_tforeign_keeps_its_last_host() {
    // The forced import writes nothing (`update/dialogs.rs`, `force_import`):
    // tforeign's last host stays deadbeef, whose host ID the last import of
    // the table takes. A forced import read/write would change both.
    let forced = POOLS
        .iter()
        .position(|step| step.shows.contains(&"Force the Import of tforeign"))
        .unwrap();
    let last_host = POOLS
        .iter()
        .position(|step| {
            step.acts
                .contains(&Act::Select("Import with Its Last Host ID"))
        })
        .unwrap();

    assert!(
        POOLS[forced]
            .shows
            .contains(&"Force the import of tforeign read-only")
    );
    assert!(POOLS[forced].shows.contains(&"deadbeef"));
    assert!(forced < last_host);
    assert_eq!(last_host, POOLS.len() - 2);
}

#[test]
fn the_clone_and_promote_comes_last_among_the_snapshot_changes() {
    let promote = SNAPSHOTS_REFUSED
        .iter()
        .position(|step| step.acts.contains(&key('m')))
        .unwrap();

    assert_eq!(promote, SNAPSHOTS_REFUSED.len() - 3);
}
