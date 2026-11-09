//! Which actions have a plan: [`Action::has_plan`], and [`App::plan`]
//! agrees with it.

use pretty_assertions::assert_eq;
use zbm_types::{Answer, Untrusted};

use super::*;
use crate::model::{ImportOptions, Method};

/// Whether `action` has a plan, said of each kind of action, so that a new
/// kind is said too.
fn is_planned(action: &Action) -> bool {
    match action {
        Action::BootSnapshot(..)
        | Action::SetDefaultEnvironment(_)
        | Action::SetDefaultKernel(..)
        | Action::InheritDefaultKernel(_)
        | Action::CreateSnapshot(_)
        | Action::Rollback(_)
        | Action::Destroy(_)
        | Action::Clone(..)
        | Action::CloneAndPromote(..)
        | Action::Duplicate(..)
        | Action::RestoreFiles { .. }
        | Action::Import(..)
        | Action::Export(_) => true,
        Action::Boot { .. }
        | Action::Diff(..)
        | Action::ResetBootTries(_)
        | Action::Reseal(_)
        | Action::Chroot(_)
        | Action::Unlock { .. }
        | Action::SkipUnlock(_)
        | Action::Answer(_)
        | Action::UnlockAll
        | Action::Open { .. }
        | Action::OpenWithKeyFile { .. }
        | Action::Close(_)
        | Action::Shell
        | Action::FirmwareSetup
        | Action::Reboot
        | Action::PowerOff
        | Action::Plan(_)
        | Action::ScanPools
        | Action::LoadDeviceLog(_)
        | Action::LoadSnapshots(_)
        | Action::LoadHistory => false,
    }
}

/// One action of each kind.
fn every_action() -> Vec<Action> {
    let snapshot: zbm_types::SnapshotName = "tank/ROOT/be@old".parse().unwrap();
    let environment: zbm_types::DatasetName = "tank/ROOT/be".parse().unwrap();
    let copy: zbm_types::DatasetName = "tank/ROOT/copy".parse().unwrap();
    let pool: zbm_types::PoolName = "tank".parse().unwrap();
    let text = |text: &str| Untrusted::new(text.to_owned());
    vec![
        Action::Boot {
            environment: environment.clone(),
            kernel: None,
            command_line: None,
            countdown: false,
        },
        Action::BootSnapshot(snapshot.clone(), copy.clone()),
        Action::SetDefaultEnvironment(environment.clone()),
        Action::SetDefaultKernel(environment.clone(), text("6.8")),
        Action::InheritDefaultKernel(environment.clone()),
        Action::CreateSnapshot("tank/ROOT/be@new".parse().unwrap()),
        Action::Rollback(snapshot.clone()),
        Action::Destroy(vec![snapshot.clone()]),
        Action::Clone(snapshot.clone(), copy.clone()),
        Action::CloneAndPromote(snapshot.clone(), copy.clone()),
        Action::Duplicate(snapshot.clone(), copy),
        Action::Diff(snapshot.clone(), None),
        Action::Diff(snapshot.clone(), Some(snapshot.clone())),
        Action::RestoreFiles {
            snapshot,
            paths: vec![text("/etc/fstab")],
        },
        Action::ResetBootTries(environment.clone()),
        Action::Reseal("root".to_owned()),
        Action::Chroot(environment.clone().into()),
        Action::Import(pool.clone(), ImportOptions::default()),
        Action::Export(pool),
        Action::Unlock {
            volume: "root".to_owned(),
            method: Method::Passphrase,
            secret: None,
        },
        Action::SkipUnlock("root".to_owned()),
        Action::Answer(Answer::Continue),
        Action::UnlockAll,
        Action::Open {
            device: "/dev/sda2".to_owned(),
            name: None,
        },
        Action::OpenWithKeyFile {
            device: "/dev/sda2".parse().unwrap(),
            name: "root".parse().unwrap(),
            key_file: "/etc/key".into(),
            options: zbm_crypt::OpenOptions::default(),
        },
        Action::Close(None),
        Action::Shell,
        Action::FirmwareSetup,
        Action::Reboot,
        Action::PowerOff,
        Action::Plan(Box::new(Action::Reboot)),
        Action::ScanPools,
        Action::LoadDeviceLog(text("/dev/sda2")),
        Action::LoadSnapshots(environment),
        Action::LoadHistory,
    ]
}

#[test]
fn only_a_change_or_the_boot_of_a_snapshot_has_a_plan() {
    let fake = machine("off");
    let runtime = Runtime::default().with_base(Path::new("/zfsbootmenu"));
    let app = App::new(Context::new(fake.system()), &runtime);

    for action in every_action() {
        assert_eq!(action.has_plan(), is_planned(&action), "{action:?}");
        let plan = app.plan(&action, fake.system());
        assert_eq!(plan.is_some(), action.has_plan(), "{action:?}");
    }
    // The differences of a snapshot are a change of package D, which
    // changes nothing; the boot of a snapshot is not one, and has its
    // clone's plan.
    let diff = Action::Diff("tank/ROOT/be@old".parse().unwrap(), None);
    assert!(diff.is_change() && !diff.has_plan());
}
