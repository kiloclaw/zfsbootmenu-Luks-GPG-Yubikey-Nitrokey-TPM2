//! Tests of the command line saved for every boot, on [`FakeZfs`] with
//! needs that record what was asked of them, and on the commands `zfs`
//! would be given.

use pretty_assertions::assert_eq;
use zbm_testkit::FakeRunner;
use zbm_zfs::ZfsCli;

use super::*;

const COMMAND_LINE: &str = "org.zfsbootmenu:commandline";

fn line(text: &str) -> Untrusted<String> {
    Untrusted::new(text.to_owned())
}

/// What `zfs` was asked to set or inherit.
fn written(zfs: &FakeZfs) -> Vec<String> {
    zfs.calls()
        .into_iter()
        .filter(|call| call.starts_with("set ") || call.starts_with("inherit "))
        .collect()
}

#[test]
fn a_command_line_is_saved_with_zfs_set_on_its_environment() {
    let zfs = machine();
    // A user property is set without the environment's key.
    let mut needs = FakeNeeds {
        locked: true,
        ..FakeNeeds::default()
    };
    let void = dataset("zroot/ROOT/void");

    let saved = Action::SetCommandLine(void.clone(), line("quiet loglevel=4"));
    let events = perform(&zfs, &mut needs, &saved);

    assert_eq!(
        outcome(&events),
        &Outcome::Done("the command line of zroot/ROOT/void is saved".to_owned())
    );
    assert_eq!(needs.asked, ["writable zroot"]);
    assert_eq!(
        written(&zfs),
        [format!(
            "set zroot/ROOT/void {COMMAND_LINE}=quiet loglevel=4"
        )]
    );
    assert_eq!(
        zfs.property(&void, COMMAND_LINE).unwrap().value,
        "quiet loglevel=4"
    );
}

#[test]
fn an_empty_line_inherits_the_command_line() {
    let zfs = machine();
    let mut needs = FakeNeeds::default();
    let void = dataset("zroot/ROOT/void");

    let empty = Action::SetCommandLine(void.clone(), line(""));
    let events = perform(&zfs, &mut needs, &empty);
    assert_eq!(
        outcome(&events),
        &Outcome::Done("zroot/ROOT/void inherits its command line".to_owned())
    );
    let events = perform(&zfs, &mut needs, &Action::InheritCommandLine(void));
    assert!(is_done(&events));

    let inherit = format!("inherit zroot/ROOT/void {COMMAND_LINE}");
    assert_eq!(written(&zfs), [inherit.clone(), inherit]);
    assert_eq!(needs.asked, ["writable zroot", "writable zroot"]);
}

#[test]
fn a_pool_that_cannot_be_written_fails_the_save_before_zfs_set() {
    let zfs = machine();
    let mut needs = FakeNeeds {
        read_only: true,
        ..FakeNeeds::default()
    };
    let saved = Action::SetCommandLine(dataset("zroot/ROOT/void"), line("quiet"));

    let events = perform(&zfs, &mut needs, &saved);

    let Outcome::Failed { what, why, .. } = outcome(&events) else {
        panic!("{events:?}");
    };
    assert_eq!(what, "unable to save the command line of zroot/ROOT/void");
    assert_eq!(why, &["zroot may not be imported read/write"]);
    assert!(written(&zfs).is_empty());
}

#[test]
fn the_line_is_one_argument_and_nothing_in_it_is_expanded() {
    let runner = FakeRunner::new();
    let zfs = ZfsCli::new(&runner);
    let mut needs = FakeNeeds::default();
    let typed = r#"quiet %{parent} root=$ROOT "a b" 'c' `d` ;e *"#;
    let saved = Action::SetCommandLine(dataset("zroot/ROOT/void"), line(typed));

    let events = Changes {
        zfs: &zfs,
        needs: &mut needs,
    }
    .perform(&saved)
    .unwrap();

    assert!(is_done(&events), "{events:?}");
    let calls = runner.calls();
    let [set] = calls.as_slice() else {
        panic!("{:?}", runner.lines());
    };
    assert_eq!(set.program(), "zfs");
    let args: Vec<String> = set
        .get_args()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "set".to_owned(),
            format!("{COMMAND_LINE}={typed}"),
            "zroot/ROOT/void".to_owned()
        ]
    );
}

#[test]
fn a_line_with_a_control_character_or_over_8191_bytes_is_refused_before_anything_runs() {
    let void = dataset("zroot/ROOT/void");
    let longest = "x".repeat(8191);
    let longer = "x".repeat(8192);
    let refused = [
        (
            "quiet\0single",
            "the command line holds a control character",
        ),
        (
            "quiet\nsingle",
            "the command line holds a control character",
        ),
        (
            "quiet\u{1b}[2J",
            "the command line holds a control character",
        ),
        (
            longer.as_str(),
            "the command line has 8192 bytes, more than the 8191 ZFS keeps",
        ),
    ];
    for (text, reason) in refused {
        let zfs = machine();
        let mut needs = FakeNeeds::default();
        let saved = Action::SetCommandLine(void.clone(), line(text));

        let events = perform(&zfs, &mut needs, &saved);

        let Outcome::Failed { what, why, .. } = outcome(&events) else {
            panic!("{text:?}: {events:?}");
        };
        assert_eq!(what, "unable to save the command line of zroot/ROOT/void");
        assert_eq!(why, &[reason]);
        assert!(needs.asked.is_empty(), "{text:?}: {:?}", needs.asked);
        assert!(written(&zfs).is_empty(), "{text:?}");
    }
    let zfs = machine();
    let saved = Action::SetCommandLine(void, line(&longest));
    assert!(is_done(&perform(&zfs, &mut FakeNeeds::default(), &saved)));
}

#[test]
fn the_locked_menu_keeps_the_saved_command_line() {
    use crate::model::Snapshot;
    let void = dataset("zroot/ROOT/void");
    let saved = Action::SetCommandLine(void.clone(), line("quiet"));
    let inherited = Action::InheritCommandLine(void.clone());
    let locked = Snapshot {
        menu_locked: true,
        bootfs: Some(void.clone()),
        ..Snapshot::default()
    };

    for action in [saved, inherited] {
        // Whatever the default: the PIN is asked on the console.
        assert!(action.is_kept_by_lock(Some(&void)), "{action:?}");
        assert!(action.is_kept_by_lock(None), "{action:?}");
        assert!(action.needs_console_on(&locked), "{action:?}");
        assert!(!action.needs_console_on(&Snapshot::default()), "{action:?}");
        assert_eq!(action.changed_environment(), Some(&void));
        assert!(action.has_plan() && action.needs_writable_pool());
    }
}
