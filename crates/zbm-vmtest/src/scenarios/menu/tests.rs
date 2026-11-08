//! The menu tests' own tests, and what they share: a guest that plays the
//! menu of a step table, and the shell's answers to the checks.

use std::fs;

use pretty_assertions::assert_eq;

use super::steps::{Act, Await, Step};
use super::*;
use crate::testing::{Step as Play, checks, cleanup, scripted, settings};

/// No pauses, and a short patience.
pub(super) const NOW: Pace = Pace {
    pause: Duration::ZERO,
    patience: 30,
};

/// How long a screen stays before the menu changes it by itself, in
/// milliseconds: longer than the serial console waits for it to settle.
const SETTLED: u64 = 600;

/// A serial console at once.
pub(super) fn serial() -> Serial {
    Serial { pace: NOW }
}

/// A whole screen of `lines`, as the menu draws it on the serial console.
pub(super) fn screen(lines: &[&str]) -> &'static str {
    let mut text = String::from("\x1b[H\x1b[2J");
    for line in lines {
        text.push_str(line);
        text.push_str("\r\n");
    }
    text.leak()
}

/// What a guest plays for `steps`: it reads each key's bytes, draws a
/// screen whose selected row holds each name selected, and the screen each
/// step waits for, with what it must show.
pub(super) fn played(steps: &[Step]) -> Vec<Play> {
    let mut script = Vec::new();
    for step in steps {
        // A step that presses nothing waits for the menu to change by
        // itself: the screen before stays long enough to be taken.
        if step.acts.is_empty() {
            script.push(Play::Pause(SETTLED));
        }
        for act in step.acts {
            match *act {
                Act::Press(key) => script.push(Play::Keys(key.serial().leak())),
                Act::Type(text) => script.push(Play::Keys(text)),
                Act::Select(name) => {
                    script.push(Play::Keys("\x1b[H"));
                    script.push(Play::Say(screen(&[&format!("│ ▶ {name} │")])));
                }
            }
        }
        let first = match step.awaits {
            Await::Nothing => continue,
            Await::Text(text) => text.to_owned(),
            Await::Done => " √ Done: it is done".to_owned(),
            Await::Row(name, text) => format!("│ ▶ {name}   {text} │"),
        };
        let rows = step.rows.iter().map(|name| format!("│   {name}   │"));
        let lines: Vec<String> = [first]
            .into_iter()
            .chain(step.shows.iter().map(|text| (*text).to_owned()))
            .chain(rows)
            .collect();
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        script.push(Play::Say(screen(&lines)));
    }
    script
}

/// Start-up, with its dialog of the LUKS volume when `luks`, then the
/// countdown, which Escape stops, and the workspace.
pub(super) fn started(luks: bool) -> Vec<Play> {
    let mut script = Vec::new();
    if luks {
        script.push(Play::Say(screen(&[
            "│ Unlock luks-0e8a5e1d │",
            "│ Passphrase:       │",
        ])));
        script.push(Play::Hear(""));
    }
    script.extend([
        Play::Say(screen(&[
            " Booting tmenu/ROOT/be in 30 s. Press any key to choose another.",
        ])),
        Play::Keys("\x1b"),
        Play::Say(screen(&[" [1 Boot] 2 Snapshots  3 Pools  4 Log"])),
    ]);
    script
}

/// The palette's Recovery Shell, which the harness prepares.
pub(super) fn shell_opened() -> Vec<Play> {
    let mut script = played(steps::SHELL);
    script.extend([
        Play::Say("zfsbootmenu ~ > "),
        Play::Hear("stty -echo cols 250; export PS1='zbm# ' PS2=''"),
        Play::Hear("true; echo RC=$?"),
        Play::Say("RC=0\r\n"),
    ]);
    script
}

/// The shell ends, and the menu shows again.
pub(super) fn shell_left() -> Vec<Play> {
    vec![
        Play::Hear("exit"),
        Play::Say(screen(&["┌ BOOT ENVIRONMENTS ───┐"])),
        Play::Keys("\x1b"),
    ]
}

/// The palette's Power Off, and the kernel powering off.
pub(super) fn powered_off() -> Vec<Play> {
    let mut script = played(steps::POWER_OFF);
    script.push(Play::Say("reboot: Power down\r\n"));
    script
}

/// The shell runs `command`, which prints `output` and ends with `code`.
pub(super) fn said(command: &str, code: i32, output: &str) -> Vec<Play> {
    vec![
        Play::Hear(format!("{command}; echo RC=$?").leak()),
        Play::Say(format!("{output}\r\nRC={code}\r\n").leak()),
    ]
}

/// The pool syncs and frees nothing more.
fn settled() -> Vec<Play> {
    let mut script = said("zpool sync tmenu", 0, "");
    script.extend(said("zpool get -Hp -o value freeing tmenu", 0, "0"));
    script
}

/// Before the destroy: the dry run frees `reclaim`, and the snapshots hold
/// `held`.
pub(super) fn space_before_said(reclaim: u64, held: u64) -> Vec<Play> {
    let mut script = settled();
    let dry_run =
        format!("destroy\ttmenu/ROOT/be@one\ndestroy\ttmenu/ROOT/be@three\nreclaim\t{reclaim}");
    script.extend(said(
        "zfs destroy -nvp tmenu/ROOT/be@one,three",
        0,
        &dry_run,
    ));
    script.extend(said(
        "zfs get -Hp -o value usedbysnapshots tmenu/ROOT/be",
        0,
        &held.to_string(),
    ));
    script
}

/// After the destroy: the snapshots hold `held`.
pub(super) fn space_after_said(held: u64) -> Vec<Play> {
    let mut script = settled();
    script.extend(said(
        "zfs get -Hp -o value usedbysnapshots tmenu/ROOT/be",
        0,
        &held.to_string(),
    ));
    script
}

/// What `zfs list` says once the snapshot tasks are done.
pub(super) const LEFT: &str = "tmenu\t-\n\
    tmenu/ROOT\t-\n\
    tmenu/ROOT/be\ttmenu/ROOT/promoted@four\n\
    tmenu/ROOT/be@before-restore-20261108T101010Z\t-\n\
    tmenu/ROOT/be#three\t-\n\
    tmenu/ROOT/cloned\ttmenu/ROOT/promoted@two\n\
    tmenu/ROOT/copied\t-\n\
    tmenu/ROOT/copied@four\t-\n\
    tmenu/ROOT/promoted\t-\n\
    tmenu/ROOT/promoted@two\t-\n\
    tmenu/ROOT/promoted@held\t-\n\
    tmenu/ROOT/promoted@four\t-\n\
    tmenu/two-clone\ttmenu/ROOT/promoted@two";

/// The shell lists `listed`, then shows the files restored.
pub(super) fn snapshots_left_said(listed: &str) -> Vec<Play> {
    let mut script = said("zfs list -H -t all -o name,origin -r tmenu", 0, listed);
    script.extend(said(
        "m=/mnt/be; grep -q ' /mnt/be ' /proc/mounts || { mkdir -p $m && mount -o zfsutil,ro \
         -t zfs tmenu/ROOT/be $m; }; cat $m/zbmtest/restore-me && ls -ln $m/zbmtest/removed",
        0,
        "restore-me at one\n-rw-r----- 1 1234 5678 8 Nov  8 10:00 /mnt/be/zbmtest/removed",
    ));
    script
}

/// The shell shows the pools as the pool tasks leave them.
pub(super) fn pools_left_said() -> Vec<Play> {
    let mut script = said(
        "zpool get -H readonly",
        0,
        "tforeign\treadonly\ton\t-\ntlocked\treadonly\ton\t-\n\
         tmenu\treadonly\ton\t-\ntother\treadonly\toff\t-",
    );
    script.extend(said(
        "zfs list tother/after",
        1,
        "cannot open 'tother/after': dataset does not exist",
    ));
    script.extend(said(
        "zpool status tother",
        0,
        "  pool: tother\n state: ONLINE",
    ));
    script.extend(said("od -An -tx4 /etc/hostid", 0, " deadbeef"));
    script.extend(said(
        "[ -e /dev/mapper/luks-0e8a5e1d-7c3b-4f2a-9d6e-1b2c3d4e5f60 ]",
        0,
        "",
    ));
    script
}

/// Plays `script` as the guest of the test `name`, runs `drive` on it, and
/// returns its checks, once the guest heard all it expected.
pub(super) fn checked(
    name: &str,
    script: Vec<Play>,
    drive: impl FnOnce(&Test, &mut Vm<'_>) -> Result<()>,
) -> Vec<String> {
    let settings = settings(&format!("menu-{name}"));
    let test = Test::new(&settings, "menu-snapshots").unwrap();
    let (mut vm, guest) = scripted(&test, script);
    drive(&test, &mut vm).unwrap();
    drop(vm);
    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

/// Whether every check of `found` passed.
pub(super) fn all_ok(found: &[String]) -> bool {
    !found.is_empty() && found.iter().all(|line| line.starts_with("ok"))
}

#[test]
fn a_menu_test_runs_only_after_its_setup_kept_the_disks() {
    let settings = settings("menu-pristine");
    let now = SystemTime::now();
    assert_eq!(missing_setup(&settings, now), Some("run menu-setup first"));
    let dir = settings.disks(GROUP);
    fs::create_dir_all(&dir).unwrap();
    for index in 0..DISKS {
        fs::write(pristine(&settings, index), format!("disk {index}")).unwrap();
    }
    assert_eq!(missing_setup(&settings, now), None);

    let disks = fresh_disks(&settings).unwrap();

    assert_eq!(disks.len(), DISKS);
    assert_eq!(disks[3].serial, "d3");
    assert_eq!(fs::read_to_string(&disks[2].path).unwrap(), "disk 2");
    let test = Test::new(&settings, "menu-nothing").unwrap();
    run(&test, "nothing").unwrap();
    assert_eq!(checks(&test), ["FAIL - unknown scenario nothing"]);
    cleanup(&settings);
}

#[test]
fn a_setup_of_an_earlier_month_must_run_again_as_its_snapshots_fold() {
    const AGAIN: &str = "run menu-setup again: its snapshots are of an earlier month";
    let settings = settings("menu-pristine-month");
    fs::create_dir_all(settings.disks(GROUP)).unwrap();
    let at = |seconds: u64| UNIX_EPOCH + Duration::from_secs(seconds);
    // 2026-10-31 23:00 UTC; an hour and a half later, 1 November.
    let october = at(1_793_487_600);
    for index in 0..DISKS {
        let path = pristine(&settings, index);
        fs::write(&path, "disk").unwrap();
        let file = fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(october).unwrap();
    }

    assert_eq!(missing_setup(&settings, at(1_793_487_700)), None);
    assert_eq!(missing_setup(&settings, at(1_793_493_000)), Some(AGAIN));
    let test = Test::new(&settings, "menu-snapshots").unwrap();
    run(&test, "snapshots").unwrap();
    assert_eq!(checks(&test), [format!("FAIL - {AGAIN}")]);
    cleanup(&settings);
}

#[test]
fn a_time_reads_as_its_year_and_month_in_utc() {
    let at = |seconds: u64| UNIX_EPOCH + Duration::from_secs(seconds);

    assert_eq!(month(UNIX_EPOCH), (1970, 1));
    assert_eq!(month(at(951_782_400)), (2000, 2), "2000-02-29");
    assert_eq!(month(at(951_868_800)), (2000, 3), "2000-03-01");
    assert_eq!(month(at(1_709_164_800)), (2024, 2), "2024-02-29");
    assert_eq!(month(at(1_793_491_199)), (2026, 10), "2026-10-31 23:59:59");
    assert_eq!(month(at(1_793_491_200)), (2026, 11), "2026-11-01");
    assert_eq!(month(at(1_798_761_599)), (2026, 12), "2026-12-31 23:59:59");
    assert_eq!(month(at(1_798_761_600)), (2027, 1), "2027-01-01");
}

#[test]
fn the_menu_shows_after_the_luks_dialog_is_skipped_and_the_countdown_stopped() {
    for luks in [false, true] {
        let found = checked(&format!("start-{luks}"), started(luks), |test, vm| {
            start(test, &mut serial(), vm)
        });

        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(all_ok(&found), "{found:#?}");
    }
}

#[test]
fn the_shell_opens_from_the_palette_and_gives_the_menu_back() {
    let mut script = shell_opened();
    script.extend(shell_left());
    script.extend(powered_off());

    let found = checked("shell", script, |test, vm| {
        let mut serial = serial();
        shell(test, &mut serial, vm)?;
        leave_shell(test, &mut serial, vm)?;
        power_off(test, &mut serial, vm)
    });

    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(all_ok(&found), "{found:#?}");
}

#[test]
fn the_volume_s_name_is_its_mapping_s() {
    assert_eq!(VOLUME, format!("luks-{UUID}"));
}
