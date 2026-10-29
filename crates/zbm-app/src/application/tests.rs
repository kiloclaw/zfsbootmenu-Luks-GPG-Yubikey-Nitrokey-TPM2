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
