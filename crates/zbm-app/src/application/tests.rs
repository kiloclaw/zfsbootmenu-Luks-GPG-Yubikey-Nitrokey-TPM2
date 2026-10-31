use std::path::Path;

use zbm_config::args::ZbmArgs;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::model::{self, Level, Timeout};

#[test]
fn the_snapshot_reads_the_machine_and_what_the_runtime_holds() {
    let fake = FakeSystem::new();
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    fake.files.file("/layouts/layouts", "us\nde\n").file(
        "/zfsbootmenu/journal.jsonl",
        "{\"level\":\"WARN\",\"message\":\"slow\"}\n",
    );
    let (mut options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline("zbm.timeout=7"));
    options.control_term = "ttyS0".into();
    let settings = zbm_luks::unlocker::Settings::new("/zfsbootmenu");
    let prompter = crate::read::tests::NoPrompts;
    let luks = Luks {
        context: fake.context(),
        settings: &settings,
        interaction: &prompter,
    };
    let runtime = Runtime::default()
        .with_options(options)
        .with_base(Path::new("/zfsbootmenu"))
        .with_layouts(Path::new("/layouts"));

    let app = App::new(Context::new(fake.system()), &runtime);
    let bare = app.snapshot().unwrap();
    let snapshot = app.with_luks(luks).snapshot().unwrap();

    assert!(bare.environments.is_empty() && bare.volumes.is_empty());
    assert_eq!(snapshot.timeout, Timeout::Seconds(7));
    assert_eq!(snapshot.keyboard.layouts, ["us", "de"]);
    assert!(!snapshot.keyboard.local);
    assert_eq!(snapshot.log.count(Level::Warning), 1);
    assert!(snapshot.volumes.is_empty() && snapshot.now > 0);
    assert_eq!(Runtime::default().layouts, Path::new(LAYOUTS));
    assert_eq!(Runtime::default().base, Path::new("/zfsbootmenu"));
}

/// A recovery that opens no shell.
struct NoShell;

impl Recovery for NoShell {
    fn unlock_late(&self) -> Result<(), zbm_boot::BootError> {
        Err(zbm_boot::BootError::NoCryptsetup)
    }

    fn emergency_shell(&self, _message: &str) {}
}

#[test]
fn changes_apply_quietly_or_on_the_console() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    let runtime = Runtime::default();
    let app = App::new(Context::new(fake.system()), &runtime);
    let take = Action::CreateSnapshot("tank/ROOT/be@new".parse().unwrap());
    let is_done = |events: &[Event]| {
        matches!(
            events.last(),
            Some(Event::Finished(model::Outcome::Done(_)))
        )
    };

    assert!(is_done(&app.apply(&take, Through::Quietly(fake.system()))));
    let on_console = Through::Console(fake.system(), &NoShell);
    assert!(is_done(&app.apply(&take, on_console)));
    let taken = fake
        .runner
        .lines()
        .into_iter()
        .filter(|line| line == "zfs snapshot tank/ROOT/be@new")
        .count();
    assert_eq!(taken, 2);

    // An action of the console applied quietly is one it cannot do.
    let reboot = app.apply(&Action::Reboot, Through::Quietly(fake.system()));
    assert!(!is_done(&reboot));
}

/// Whether `events` end as the locked menu refuses.
fn is_refused(events: &[Event]) -> bool {
    events == auth::refused()
}

#[test]
fn the_locked_menu_refuses_what_it_keeps_until_the_pin_is_typed_on_the_console() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.files.file("/zfsbootmenu/bootfs", "tank/ROOT/be\n");
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    let runtime = Runtime::default();
    let authority = Authority::new(auth::Lock::On("hash".to_owned()));
    let app = App::new(Context::new(fake.system()), &runtime).with_authority(&authority);
    let rollback = Action::Rollback("tank/ROOT/be@old".parse().unwrap());
    let other = Action::Boot {
        environment: "tank/ROOT/other".parse().unwrap(),
        kernel: None,
        command_line: None,
        countdown: false,
    };

    assert!(app.snapshot().unwrap().menu_locked);
    // Quietly, it is refused at once; on the console without keyholders,
    // nothing checks a PIN.
    assert!(is_refused(
        &app.apply(&rollback, Through::Quietly(fake.system()))
    ));
    assert!(is_refused(
        &app.apply(&other, Through::Console(fake.system(), &NoShell))
    ));
    // What it does not keep goes on.
    let plan = Action::Plan(Box::new(rollback.clone()));
    assert!(!is_refused(
        &app.apply(&plan, Through::Quietly(fake.system()))
    ));

    // A wrong PIN, then a blank line: refused; the right one: done, and
    // the menu is open while it is used.
    for (typed, right) in [
        (&b"wrong pin"[..], false),
        (b"", false),
        (b"correct horse", true),
    ] {
        let (client, keyholder) = zbm_sys::socket::Connection::pair().unwrap();
        fake.console.secret(typed);
        let runner = &fake.runner;
        let events = std::thread::scope(|scope| {
            zbm_platform::event::spawn_scoped_traced(scope, move || {
                let _ = crate::keyholder::serve(
                    &keyholder,
                    runner,
                    |_, _| Vec::new(),
                    |pin| pin.expose() == b"correct horse",
                );
            });
            // Unused, the client's end goes, and the keyholder ends.
            let keyholders = crate::keyholder::tests::Given::new(client);
            let events = app
                .with_keyholders(&keyholders)
                .apply(&rollback, Through::Console(fake.system(), &NoShell));
            drop(keyholders);
            events
        });
        assert_eq!(is_refused(&events), !right, "{typed:?}: {events:?}");
    }
    assert!(!app.snapshot().unwrap().menu_locked);
    assert!(fake.console.output_text().contains(auth::LOCKED_PROMPT));

    // No keyholder starts: the PIN cannot be checked, and nothing runs.
    authority.forget();
    let none = crate::keyholder::tests::Given::none();
    fake.console.secret(b"correct horse");
    let events = app
        .with_keyholders(&none)
        .apply(&rollback, Through::Console(fake.system(), &NoShell));
    assert!(is_refused(&events), "{events:?}");
}

#[test]
fn the_lock_keeps_all_but_the_default_boot_and_what_changes_nothing() {
    let default: zbm_types::DatasetName = "tank/ROOT/be".parse().unwrap();
    let boot = |environment: &str, kernel: Option<&str>, countdown| Action::Boot {
        environment: environment.parse().unwrap(),
        kernel: kernel.map(|kernel| zbm_types::Untrusted::new(kernel.to_owned())),
        command_line: None,
        countdown,
    };
    let bootfs = Some(&default);
    assert!(!boot("tank/ROOT/be", None, false).is_kept_by_lock(bootfs));
    assert!(!boot("tank/ROOT/other", None, true).is_kept_by_lock(bootfs));
    assert!(boot("tank/ROOT/other", None, false).is_kept_by_lock(bootfs));
    assert!(boot("tank/ROOT/be", Some("6.1"), false).is_kept_by_lock(bootfs));
    assert!(boot("tank/ROOT/be", None, false).is_kept_by_lock(None));
    let pool: zbm_types::PoolName = "tank".parse().unwrap();
    assert!(Action::Import(pool.clone(), zbm_types::ImportPolicy::Force).is_kept_by_lock(bootfs));
    assert!(!Action::Import(pool, zbm_types::ImportPolicy::Strict).is_kept_by_lock(bootfs));
    for open in [Action::Shell, Action::Reboot, Action::UnlockAll] {
        assert!(!open.is_kept_by_lock(bootfs), "{open:?}");
    }
    assert!(Action::Chroot("tank/ROOT/be".to_owned()).is_kept_by_lock(bootfs));
}
