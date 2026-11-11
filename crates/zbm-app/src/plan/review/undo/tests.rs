//! What undoes a change that loses nothing, on [`FakeZfs`].

use pretty_assertions::assert_eq;
use zbm_testkit::zfs::FakeZfs;
use zbm_types::{ImportPolicy, SnapshotName, Untrusted};

use super::*;

fn dataset(text: &str) -> DatasetName {
    text.parse().unwrap()
}

fn pool(text: &str) -> PoolName {
    text.parse().unwrap()
}

fn import(access: Access, policy: ImportPolicy) -> ImportOptions {
    ImportOptions {
        access,
        policy,
        rewind: false,
        id: None,
    }
}

/// The undo `zfs` gives `action`.
fn undo(zfs: &FakeZfs, action: &Action) -> Option<Action> {
    review(zfs, action).undo.map(|undo| *undo)
}

#[test]
fn a_snapshot_is_undone_by_destroying_it() {
    let snapshot: SnapshotName = "zroot/ROOT/void@new".parse().unwrap();

    let review = review(&FakeZfs::new(), &Action::CreateSnapshot(snapshot.clone()));

    assert_eq!(
        review,
        Review {
            undo: Some(Box::new(Action::Destroy(vec![snapshot]))),
            ..Review::default()
        }
    );
}

#[test]
fn a_plain_import_is_undone_by_exporting_it() {
    let zfs = FakeZfs::new().pool("zroot").exported("tank", 7);

    for access in [Access::ReadOnly, Access::ReadWrite] {
        let action = Action::Import(pool("tank"), import(access, ImportPolicy::Strict));
        assert_eq!(undo(&zfs, &action), Some(Action::Export(pool("tank"))));
    }
    // A list of the imported pools that fails leaves its undo unknown.
    let unread = zfs.failing("pools", "out of memory");
    let plain = Action::Import(pool("tank"), import(Access::ReadOnly, ImportPolicy::Strict));
    assert_eq!(review(&unread, &plain), Review::default());
}

#[test]
fn the_import_of_a_pool_named_like_an_imported_one_has_no_undo() {
    // ZFS refuses it; an export would name the pool imported already.
    let zfs = FakeZfs::new().pool("tank").exported("tank", 7);
    let other = ImportOptions {
        id: Some(7),
        ..import(Access::ReadOnly, ImportPolicy::Strict)
    };

    assert_eq!(
        review(&zfs, &Action::Import(pool("tank"), other)),
        Review::default()
    );
}

#[test]
fn the_read_write_toggle_is_undone_by_the_other_access() {
    let read_only = FakeZfs::new().pool("zroot").read_only("zroot");
    let read_write = FakeZfs::new().pool("zroot");
    let to = |access| Action::Import(pool("zroot"), import(access, ImportPolicy::Strict));

    assert_eq!(
        undo(&read_only, &to(Access::ReadWrite)),
        Some(to(Access::ReadOnly))
    );
    assert_eq!(
        undo(&read_write, &to(Access::ReadOnly)),
        Some(to(Access::ReadWrite))
    );
    // The access it has already changes nothing, and has nothing to undo.
    assert_eq!(
        review(&read_write, &to(Access::ReadWrite)),
        Review::default()
    );
}

#[test]
fn the_last_host_s_import_of_another_system_s_pool_says_the_host_id_stays() {
    let zfs = FakeZfs::new().foreign("tank", 7, "0badf00d", "other", 0);
    let hostid = |id| {
        let options = ImportOptions {
            id,
            ..import(Access::ReadOnly, ImportPolicy::Hostid)
        };
        Action::Import(pool("tank"), options)
    };

    for action in [hostid(None), hostid(Some(7))] {
        assert_eq!(
            review(&zfs, &action),
            Review {
                host_id_stays: true,
                ..Review::default()
            },
            "{action:?}"
        );
    }
}

#[test]
fn the_last_host_s_import_of_this_host_s_pool_is_undone_by_its_export() {
    // The plain import comes first, and adopts no host ID.
    let exported = FakeZfs::new().exported("tank", 7);
    let hostid = Action::Import(pool("tank"), import(Access::ReadOnly, ImportPolicy::Hostid));
    assert_eq!(undo(&exported, &hostid), Some(Action::Export(pool("tank"))));
    // A pool imported already adopts no host ID: its toggle is undone.
    let imported = FakeZfs::new().pool("tank");
    let toggle = Action::Import(pool("tank"), import(Access::ReadOnly, ImportPolicy::Hostid));
    let reviewed = review(&imported, &toggle);
    assert!(
        !reviewed.host_id_stays && reviewed.undo.is_some(),
        "{reviewed:?}"
    );
    // A scan that fails, or does not list the pool, leaves it unknown.
    let unscanned = exported.failing("importable", "cancelled");
    assert_eq!(review(&unscanned, &hostid), Review::default());
    assert_eq!(review(&FakeZfs::new(), &hostid), Review::default());
}

#[test]
fn a_new_default_is_undone_by_the_old_one() {
    let zfs = FakeZfs::new()
        .pool("zroot")
        .filesystem("zroot/ROOT/old", &[])
        .filesystem("zroot/ROOT/new", &[])
        .bootfs("zroot", "zroot/ROOT/old");
    let default = |name| Action::SetDefaultEnvironment(dataset(name));

    assert_eq!(
        undo(&zfs, &default("zroot/ROOT/new")),
        Some(default("zroot/ROOT/old"))
    );
    // Without a default before, or with the same one, nothing undoes it.
    assert_eq!(undo(&zfs, &default("zroot/ROOT/old")), None);
    let none = FakeZfs::new().pool("zroot");
    assert_eq!(undo(&none, &default("zroot/ROOT/new")), None);
    // A pool that could not be read has no default known.
    assert_eq!(undo(&FakeZfs::new(), &default("zroot/ROOT/new")), None);
}

#[test]
fn a_kernel_set_where_it_was_inherited_is_undone_by_inheriting() {
    let kernel = "org.zfsbootmenu:kernel";
    let zfs = FakeZfs::new()
        .pool("zroot")
        .filesystem("zroot/ROOT", &[(kernel, "vmlinuz-6.1")])
        .filesystem("zroot/ROOT/inherits", &[])
        .filesystem("zroot/ROOT/own", &[(kernel, "vmlinuz-6.6")])
        .filesystem("zroot/unset", &[]);
    let set = |name| Action::SetDefaultKernel(dataset(name), Untrusted::new("6.8".to_owned()));

    for name in ["zroot/ROOT/inherits", "zroot/unset"] {
        assert_eq!(
            undo(&zfs, &set(name)),
            Some(Action::InheritDefaultKernel(dataset(name))),
            "{name}"
        );
    }
    // Its own kernel, or one that could not be read, has no undo.
    assert_eq!(undo(&zfs, &set("zroot/ROOT/own")), None);
    assert_eq!(undo(&zfs, &set("zroot/gone")), None);
}

#[test]
fn a_clone_a_copy_an_export_or_an_inherited_kernel_has_no_undo() {
    let zfs = FakeZfs::new().pool("zroot");
    let snapshot: SnapshotName = "zroot/ROOT/void@old".parse().unwrap();
    let copy = dataset("zroot/ROOT/copy");

    for action in [
        Action::Clone(snapshot.clone(), copy.clone()),
        Action::CloneAndPromote(snapshot.clone(), copy.clone()),
        Action::Duplicate(snapshot.clone(), copy.clone()),
        Action::BootSnapshot(snapshot.clone(), copy),
        Action::RestoreFiles {
            snapshot,
            paths: Vec::new(),
        },
        Action::Export(pool("zroot")),
        Action::InheritDefaultKernel(dataset("zroot/ROOT/void")),
    ] {
        assert_eq!(review(&zfs, &action), Review::default(), "{action:?}");
    }
}

#[test]
fn the_undo_of_a_saved_line_sets_the_old_one_or_inherits_it() {
    let line = "org.zfsbootmenu:commandline";
    let zfs = FakeZfs::new()
        .pool("zroot")
        .filesystem("zroot/ROOT", &[(line, "quiet %{parent}")])
        .filesystem("zroot/ROOT/void", &[(line, "loglevel=4 %{parent}")])
        .filesystem("zroot/ROOT/arch", &[]);
    let typed = Untrusted::new("quiet".to_owned());
    let saved = |name: &str| Action::SetCommandLine(dataset(name), typed.clone());

    // Set on the environment: its line as it is, `%{parent}` unexpanded.
    let old = Untrusted::new("loglevel=4 %{parent}".to_owned());
    let set_back = Action::SetCommandLine(dataset("zroot/ROOT/void"), old);
    assert_eq!(
        undo(&zfs, &saved("zroot/ROOT/void")),
        Some(set_back.clone())
    );
    let inherit = Action::InheritCommandLine(dataset("zroot/ROOT/void"));
    assert_eq!(undo(&zfs, &inherit), Some(set_back));
    // Inherited, or none: inherited again.
    let inherits = Some(Action::InheritCommandLine(dataset("zroot/ROOT/arch")));
    assert_eq!(undo(&zfs, &saved("zroot/ROOT/arch")), inherits);
    let bare = FakeZfs::new().pool("tank").filesystem("tank/be", &[]);
    let saved_bare = Action::SetCommandLine(dataset("tank/be"), typed.clone());
    let inherits_bare = Some(Action::InheritCommandLine(dataset("tank/be")));
    assert_eq!(undo(&bare, &saved_bare), inherits_bare);
    // A line that cannot be read leaves the undo unknown.
    assert_eq!(review(&zfs, &saved("zroot/ROOT/gone")), Review::default());
}
