//! The first link or mount point on the way from a base to a path below
//! it, where a filesystem would be mounted (SB19).

use std::io;
use std::path::{Path, PathBuf};

use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeFiles;

use super::super::{InRoot, Obstacle};

/// The state directory, where every walk starts.
const BASE: &str = "/zfsbootmenu";
/// The directory of every filesystem's state.
const E: &str = "/zfsbootmenu/environments";
/// Where `tank/ROOT/be` is mounted.
const MP: &str = "/zfsbootmenu/environments/tank/ROOT/be/mnt";

/// The kernel's line for a mount `id` of a ZFS dataset on `point`,
/// escaped as written.
fn line(id: u64, point: &str) -> String {
    format!("{id} 25 0:50 / {point} rw,relatime shared:1 - zfs tank/x rw\n")
}

/// The first obstacle on the way to `at` in `files`, with `table`.
fn obstacle(files: &FakeFiles, at: &str, table: &str) -> Option<Obstacle> {
    let below = Path::new(at).strip_prefix(BASE).expect("below the base");
    let walk = InRoot::new(below).expect("no `..`");
    walk.first_obstacle(files, Path::new(BASE), table.as_bytes())
}

/// The mount point that `found` names, which is one.
fn mount(found: Option<Obstacle>) -> PathBuf {
    match found {
        Some(Obstacle::Mount(path)) => path,
        other => panic!("not a mount point: {other:?}"),
    }
}

#[test]
fn a_mount_point_on_the_way_is_an_obstacle() {
    let files = FakeFiles::new();
    files.dir(MP);
    let table = line(40, &format!("{E}/tank/ROOT"));
    assert_eq!(
        mount(obstacle(&files, MP, &table)),
        PathBuf::from(format!("{E}/tank/ROOT"))
    );
    // The first one on the way, from the base down.
    let table = format!("{}{}", line(41, &format!("{E}/tank/ROOT/be")), line(40, E));
    assert_eq!(mount(obstacle(&files, MP, &table)), PathBuf::from(E));
}

#[test]
fn the_mount_point_itself_is_no_obstacle_to_the_walk() {
    // Nor is the base: the caller decides what a mount on either means.
    let files = FakeFiles::new();
    files.dir(MP);
    let table = format!("{}{}", line(40, MP), line(41, BASE));
    assert!(obstacle(&files, MP, &table).is_none());
    assert!(obstacle(&files, MP, "").is_none());
}

#[test]
fn a_link_on_the_way_is_found_before_any_mount() {
    let files = FakeFiles::new();
    files.symlink(format!("{E}/tank/ROOT"), "/etc");
    let table = line(40, &format!("{E}/tank"));
    match obstacle(&files, MP, &table) {
        Some(Obstacle::Link(link, target)) => {
            assert_eq!(link, PathBuf::from(format!("{E}/tank/ROOT")));
            assert_eq!(target, PathBuf::from("/etc"));
        }
        other => panic!("not a link: {other:?}"),
    }
}

#[test]
fn a_name_that_cannot_be_read_is_unread() {
    let files = FakeFiles::new();
    files.dir(MP);
    files.fail(format!("{E}/tank"), io::ErrorKind::PermissionDenied);
    match obstacle(&files, MP, "") {
        Some(Obstacle::Unread(error)) => {
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        }
        other => panic!("not unread: {other:?}"),
    }
}

#[test]
fn the_base_s_own_children_are_walked() {
    let files = FakeFiles::new();
    files.symlink(E, "/tmp");
    match obstacle(&files, MP, "") {
        Some(Obstacle::Link(link, _)) => assert_eq!(link, PathBuf::from(E)),
        other => panic!("not a link: {other:?}"),
    }
}

#[test]
fn mount_points_are_compared_unescaped() {
    // A dataset with a space in its name: the kernel writes it `\040`.
    let files = FakeFiles::new();
    let at = format!("{E}/tank/my be/x/mnt");
    files.dir(&at);
    let table = line(40, &format!("{E}/tank/my\\040be"));
    assert_eq!(
        mount(obstacle(&files, &at, &table)),
        PathBuf::from(format!("{E}/tank/my be"))
    );
    // Names are compared whole: `tank/my` is no prefix of `tank/my be`.
    let table = line(40, &format!("{E}/tank/my"));
    assert!(obstacle(&files, &at, &table).is_none());
}

#[test]
fn an_obstacle_says_where_and_why() {
    let link = Obstacle::Link(PathBuf::from(format!("{E}/tank")), PathBuf::from("/etc"));
    assert_eq!(link.to_string(), format!("{E}/tank is a link to /etc"));
    let mount = Obstacle::Mount(PathBuf::from(E));
    assert_eq!(mount.to_string(), format!("{E} is a mount point"));
    let unread = Obstacle::Unread(io::Error::other("the table could not be read"));
    assert_eq!(unread.to_string(), "the table could not be read");
}
