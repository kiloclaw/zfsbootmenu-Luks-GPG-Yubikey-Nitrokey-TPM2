//! The kinds of action the application offers, and what it does with the
//! others.

use zbm_testkit::fakes::FakeSystem;
use zbm_types::DatasetName;

use super::tests::NoShell;
use super::*;
use crate::boot::OS_INDICATIONS_SUPPORTED;
use crate::model;

#[test]
fn firmware_setup_is_offered_only_where_the_firmware_says_it_can_open() {
    // No variable, as on a machine that boots from a BIOS; the bit clear;
    // the bit set, in the first byte after the four of the attributes, of
    // a 64-bit value, as systemd reads it, and of no 64-bit value.
    let cases: [(Option<Vec<u8>>, bool); 4] = [
        (None, false),
        (Some(vec![7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), false),
        (Some(vec![7, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]), true),
        (Some(vec![7, 0, 0, 0, 1, 0, 0, 0]), false),
    ];
    for (supported, offered) in cases {
        let fake = FakeSystem::new();
        fake.runner.respond_program("zpool", 0, "");
        fake.runner.respond_program("zfs", 0, "");
        if let Some(value) = &supported {
            fake.files.file(OS_INDICATIONS_SUPPORTED, value.clone());
        }
        let runtime = Runtime::default();
        let app = App::new(Context::new(fake.system()), &runtime);

        let offers = app.snapshot().unwrap().offers;

        assert!(offers.contains(&model::Offer::Power));
        assert_eq!(
            offers.contains(&model::Offer::FirmwareSetup),
            offered,
            "{supported:?}"
        );
    }
}

/// An action of `kind`, on the environment `tank/ROOT/be` or its snapshot
/// `old`.
fn of_kind(kind: model::Offer) -> Action {
    use model::Offer;
    let environment: DatasetName = "tank/ROOT/be".parse().unwrap();
    let snapshot: zbm_types::SnapshotName = "tank/ROOT/be@old".parse().unwrap();
    let target: DatasetName = "tank/ROOT/new".parse().unwrap();
    match kind {
        Offer::Boot => Action::Boot {
            environment,
            kernel: None,
            command_line: None,
            countdown: false,
        },
        Offer::BootSnapshot => Action::BootSnapshot(snapshot, target),
        Offer::SetDefaultEnvironment => Action::SetDefaultEnvironment(environment),
        Offer::DefaultKernel => Action::InheritDefaultKernel(environment),
        Offer::CommandLine => Action::InheritCommandLine(environment),
        Offer::CreateSnapshot => Action::CreateSnapshot("tank/ROOT/be@new".parse().unwrap()),
        Offer::Rollback => Action::Rollback(snapshot),
        Offer::Destroy => Action::Destroy(vec![snapshot]),
        Offer::Clone => Action::Clone(snapshot, target),
        Offer::Duplicate => Action::Duplicate(snapshot, target),
        Offer::Diff => Action::Diff(snapshot, None),
        Offer::RestoreFiles => Action::RestoreFiles {
            snapshot,
            paths: vec![zbm_types::Untrusted::new("/etc/fstab".to_owned())],
        },
        Offer::Chroot => Action::Chroot(environment.into()),
        Offer::Import => Action::Import(environment.pool(), model::ImportOptions::default()),
        Offer::Export => Action::Export(environment.pool()),
        Offer::OpenVolume => Action::Open {
            device: "/dev/sda2".to_owned(),
            name: None,
        },
        Offer::Reseal => Action::Reseal("luks-a".to_owned()),
        Offer::ResetBootTries => Action::ResetBootTries(environment),
        Offer::Shell => Action::Shell,
        Offer::FirmwareSetup => Action::FirmwareSetup,
        Offer::Power => Action::Reboot,
    }
}

#[test]
fn every_kind_the_app_offers_is_performed_and_the_others_are_unavailable() {
    // The chroot on every image; opening a volume with a LUKS session, in
    // an image with `cryptsetup`; the firmware setup where the firmware
    // says it can open it.
    for (has_firmware_setup, has_luks) in [(false, false), (true, true)] {
        let fake = FakeSystem::new();
        fake.files.dir("/zfsbootmenu");
        if has_firmware_setup {
            fake.files.file(
                OS_INDICATIONS_SUPPORTED,
                [7, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
            );
        }
        fake.environment.set("PATH", "/sbin");
        fake.files.file_with_mode("/sbin/cryptsetup", "", 0o755);
        fake.runner.respond_program("zpool", 0, "");
        fake.runner.respond_program("zfs", 0, "");
        let runtime = Runtime::default();
        let settings = zbm_luks::unlocker::Settings::new("/zfsbootmenu");
        let prompter = crate::read::tests::NoPrompts;
        let luks = Luks {
            context: fake.context(),
            settings: &settings,
            interaction: &prompter,
        };
        let mut app = App::new(Context::new(fake.system()), &runtime);
        if has_luks {
            app = app.with_luks(luks);
        }
        let offers = app.snapshot().unwrap().offers;

        assert!(!offers.is_empty());
        assert!(offers.contains(&model::Offer::Chroot));
        assert_eq!(offers.contains(&model::Offer::OpenVolume), has_luks);
        assert_eq!(
            offers.contains(&model::Offer::FirmwareSetup),
            has_firmware_setup
        );
        for kind in model::Offer::ALL {
            let action = of_kind(kind);
            assert_eq!(action.offer(), Some(kind));
            let events = app.apply(&action, Through::Console(fake.system(), &NoShell));
            let is_unavailable = events == unavailable(&action);
            assert_eq!(
                is_unavailable,
                !offers.contains(&kind),
                "{kind:?}: {events:?}"
            );
        }
        // What only reads has no kind, and is performed whatever is offered.
        let device = zbm_types::Untrusted::new("/dev/sda2".to_owned());
        for action in [Action::ScanPools, Action::LoadDeviceLog(device)] {
            assert!(action.is_read() && action.offer().is_none());
            for through in [
                Through::Quietly(fake.system()),
                Through::Console(fake.system(), &NoShell),
            ] {
                let events = app.apply(&action, through);
                assert_ne!(events, unavailable(&action), "{action:?}");
            }
        }
    }
}
