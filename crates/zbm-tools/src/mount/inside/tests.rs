use std::path::{Component, Path, PathBuf};

use pretty_assertions::assert_eq;
use proptest::prelude::*;
use zbm_testkit::fakes::FakeFiles;

use super::*;

/// Where the chroot's root is mounted.
const ROOT: &str = "/zfsbootmenu/environments/tank/ROOT/be/mnt";

/// `path` below [`ROOT`].
fn below(path: &str) -> PathBuf {
    Path::new(ROOT).join(path)
}

/// A root with `/etc`, and this system's files at the same paths outside
/// it, which no walk may find: `/etc/resolv.conf`, `/run/x/y`, and the
/// link `/link` to `/etc`.
fn files() -> FakeFiles {
    let files = FakeFiles::new();
    files.dir(below("etc"));
    files
        .file("/etc/resolv.conf", "nameserver 10.0.2.3\n")
        .file("/run/x/y", "")
        .symlink("/link", "/etc");
    files
}

/// Where `path` leads below [`ROOT`] in `files`.
fn resolved(files: &FakeFiles, path: &str) -> Result<Inside, Unresolved> {
    resolve(files, Path::new(ROOT), Path::new(path))
}

/// `path`, given as the chroot names it, inside the root.
fn in_root(path: &str) -> InRoot {
    InRoot(
        Path::new(path)
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => Some(name.to_owned()),
                _ => None,
            })
            .collect(),
    )
}

/// What `resolved` gives when it leads to `path` inside the root, with
/// links `followed` or not.
fn inside(path: &str, followed: bool) -> Inside {
    Inside {
        path: in_root(path),
        followed,
    }
}

#[test]
fn a_path_without_links_resolves_to_itself_inside_the_root() {
    let files = files();
    files.file(below("etc/resolv.conf"), "nameserver 127.0.0.53\n");
    let found = resolved(&files, "/etc/resolv.conf").unwrap();
    assert_eq!(found, inside("/etc/resolv.conf", false));
    assert_eq!(found.path.under(Path::new(ROOT)), below("etc/resolv.conf"));
    assert_eq!(found.path.in_chroot(), Path::new("/etc/resolv.conf"));
    // `.` is skipped, and the root is the root.
    assert_eq!(
        resolved(&files, "/./etc/./resolv.conf").unwrap(),
        inside("/etc/resolv.conf", false)
    );
    assert_eq!(resolved(&files, "/").unwrap(), inside("/", false));
    assert_eq!(
        resolved(&files, "/").unwrap().path.under(Path::new(ROOT)),
        Path::new(ROOT)
    );
}

#[test]
fn an_absolute_link_is_followed_from_the_root_never_the_image_s() {
    let files = files();
    files
        .symlink(below("etc/resolv.conf"), "/run/x/y")
        .file(below("run/x/y"), "");
    assert_eq!(
        resolved(&files, "/etc/resolv.conf").unwrap(),
        inside("/run/x/y", true)
    );
    // The image's `/link` leads to its `/etc`; the root's, to the root's.
    files.symlink(below("link"), "/etc");
    assert_eq!(
        resolved(&files, "/link/resolv.conf").unwrap(),
        inside("/run/x/y", true)
    );
}

#[test]
fn a_relative_link_is_followed_from_its_own_directory() {
    let files = files();
    files
        .symlink(below("etc/resolv.conf"), "../run/x/y")
        .file(below("run/x/y"), "");
    assert_eq!(
        resolved(&files, "/etc/resolv.conf").unwrap(),
        inside("/run/x/y", true)
    );
    files
        .symlink(below("etc/resolv.conf"), "resolv/main")
        .file(below("etc/resolv/main"), "");
    assert_eq!(
        resolved(&files, "/etc/resolv.conf").unwrap(),
        inside("/etc/resolv/main", true)
    );
}

#[test]
fn dot_dot_never_climbs_above_the_root() {
    let files = files();
    files
        .symlink(below("etc/resolv.conf"), "../../../../../../../../etc/x")
        .file(below("etc/x"), "");
    assert_eq!(
        resolved(&files, "/etc/resolv.conf").unwrap(),
        inside("/etc/x", true)
    );
    assert_eq!(
        resolved(&files, "/../../etc/x").unwrap(),
        inside("/etc/x", false)
    );
}

#[test]
fn a_link_in_a_directory_on_the_way_is_followed_inside_the_root() {
    let files = files();
    files
        .symlink(below("var/run"), "../run")
        .symlink(below("etc/resolv.conf"), "/var/run/x/y")
        .file(below("run/x/y"), "");
    assert_eq!(
        resolved(&files, "/etc/resolv.conf").unwrap(),
        inside("/run/x/y", true)
    );
}

#[test]
fn a_dangling_link_names_the_missing_path_inside_the_root() {
    let files = files();
    files.dir(below("run")).symlink(
        below("etc/resolv.conf"),
        "/run/systemd/resolve/stub-resolv.conf",
    );
    let Err(Unresolved::Missing { path, names_only }) = resolved(&files, "/etc/resolv.conf") else {
        panic!("not missing");
    };
    assert_eq!(path, in_root("/run/systemd/resolve/stub-resolv.conf"));
    assert!(names_only);
    assert!(path.is_below("run"));
    assert_eq!(path.top(), in_root("/run"));
    assert!(!path.top().is_below("run"));
    // So is a file that is not there, with no link.
    let Err(Unresolved::Missing { path, .. }) = resolved(&files, "/etc/hosts") else {
        panic!("not missing");
    };
    assert_eq!(path, in_root("/etc/hosts"));
}

#[test]
fn a_missing_path_followed_by_dot_dot_is_not_names_only() {
    let files = files();
    files
        .dir(below("run"))
        .symlink(below("etc/resolv.conf"), "/run/gone/../x");
    let Err(Unresolved::Missing { path, names_only }) = resolved(&files, "/etc/resolv.conf") else {
        panic!("not missing");
    };
    assert!(!names_only);
    assert_eq!(path, in_root("/run/gone"));
}

#[test]
fn a_path_through_tmp_proc_sys_or_dev_is_covered() {
    for covered in ["/tmp", "/proc/self", "/sys/x", "/dev/shm/resolv.conf"] {
        let files = files();
        files.symlink(below("etc/resolv.conf"), covered);
        let found = resolved(&files, "/etc/resolv.conf");
        let Err(Unresolved::Covered(path)) = found else {
            panic!("{covered}: {found:?}");
        };
        assert_eq!(path.top(), in_root(covered).top());
    }
}

#[test]
fn a_path_named_like_a_covered_one_is_not_covered() {
    let files = files();
    files.file(below("devices/x"), "").file(below("tmpfoo"), "");
    assert_eq!(
        resolved(&files, "/devices/x").unwrap(),
        inside("/devices/x", false)
    );
    assert_eq!(
        resolved(&files, "/tmpfoo").unwrap(),
        inside("/tmpfoo", false)
    );
}

#[test]
fn a_link_loop_stops_after_forty_links() {
    let looped = files();
    looped
        .symlink(below("etc/a"), "b")
        .symlink(below("etc/b"), "a");
    let found = resolved(&looped, "/etc/a");
    assert!(matches!(found, Err(Unresolved::TooManyLinks)), "{found:?}");
    // Forty are still followed.
    let files = files();
    for n in 0..39 {
        files.symlink(below(&format!("etc/{n}")), format!("{}", n + 1));
    }
    files.symlink(below("etc/39"), "x").file(below("etc/x"), "");
    assert_eq!(resolved(&files, "/etc/0").unwrap(), inside("/etc/x", true));
}

#[test]
fn a_file_on_the_way_is_not_a_directory() {
    let files = files();
    files
        .file(below("etc/file"), "")
        .symlink(below("etc/resolv.conf"), "file/x");
    let found = resolved(&files, "/etc/resolv.conf");
    let Err(Unresolved::NotADirectory(path)) = found else {
        panic!("{found:?}");
    };
    assert_eq!(path, in_root("/etc/file"));
}

#[test]
fn a_link_that_cannot_be_read_is_an_error() {
    let files = files();
    files.fail(below("etc/resolv.conf"), io::ErrorKind::PermissionDenied);
    let found = resolved(&files, "/etc/resolv.conf");
    let Err(Unresolved::Io(error)) = found else {
        panic!("{found:?}");
    };
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn why_a_path_leads_nowhere_reads_as_plain_words() {
    let missing = Unresolved::Missing {
        path: in_root("/etc/resolv.conf"),
        names_only: true,
    };
    let denied = io::Error::from(io::ErrorKind::PermissionDenied);
    let cases = [
        (missing, "/etc/resolv.conf is missing in the environment"),
        (
            Unresolved::Covered(in_root("/dev/shm/x")),
            "it leads into this system's /dev",
        ),
        (
            Unresolved::NotADirectory(in_root("/etc/file")),
            "/etc/file is not a directory",
        ),
        (Unresolved::TooManyLinks, "too many symbolic links"),
        (Unresolved::Io(denied), "permission denied"),
    ];
    for (unresolved, said) in cases {
        assert_eq!(unresolved.to_string(), said);
    }
}

/// The first link on `path`, below [`ROOT`], in `files`.
fn first(files: &dyn Files, path: &str) -> io::Result<Option<(InRoot, PathBuf)>> {
    InRoot::new(Path::new(path))
        .unwrap()
        .first_link(files, Path::new(ROOT))
}

#[test]
fn in_root_takes_the_names_and_refuses_dot_dot() {
    assert_eq!(
        InRoot::new(Path::new("/sys/./firmware/efi/")),
        Some(in_root("/sys/firmware/efi"))
    );
    assert_eq!(InRoot::new(Path::new("dev/pts")), Some(in_root("/dev/pts")));
    assert_eq!(InRoot::new(Path::new("/")), Some(InRoot::default()));
    assert_eq!(InRoot::new(Path::new("tmp/../etc")), None);
    assert_eq!(InRoot::new(Path::new("../tmp")), None);
}

#[test]
fn a_path_with_no_link_has_none_on_its_way() {
    let files = files();
    files.dir(below("sys/firmware/efi/efivars"));
    assert_eq!(first(&files, "/sys/firmware/efi/efivars").unwrap(), None);
    assert_eq!(first(&files, "/").unwrap(), None);
}

#[test]
fn a_link_at_the_last_name_is_the_first_link() {
    let files = files();
    files.symlink(below("tmp"), "/etc");
    assert_eq!(
        first(&files, "/tmp").unwrap(),
        Some((in_root("/tmp"), PathBuf::from("/etc")))
    );
    files.symlink(below("dev"), "../../../../../../etc");
    assert_eq!(
        first(&files, "/dev").unwrap(),
        Some((in_root("/dev"), PathBuf::from("../../../../../../etc")))
    );
}

#[test]
fn a_link_in_the_middle_is_found_before_the_names_after_it() {
    let files = files();
    // The image's `/etc/firmware` is a link too: following `sys` would
    // find it, and the walk must not.
    files
        .symlink(below("sys"), "/etc")
        .symlink("/etc/firmware", "/run/x");
    assert_eq!(
        first(&files, "/sys/firmware/efi/efivars").unwrap(),
        Some((in_root("/sys"), PathBuf::from("/etc")))
    );
    files.dir(below("dev")).symlink(below("dev/pts"), "/etc");
    assert_eq!(
        first(&files, "/dev/pts/0").unwrap(),
        Some((in_root("/dev/pts"), PathBuf::from("/etc")))
    );
}

#[test]
fn a_missing_name_ends_the_walk_with_no_link() {
    let files = files();
    assert_eq!(first(&files, "/proc").unwrap(), None);
    assert_eq!(first(&files, "/sys/firmware/efi/efivars").unwrap(), None);
}

#[test]
fn a_file_on_the_way_ends_the_walk_with_no_link() {
    let files = files();
    files.file(below("dev"), "");
    assert_eq!(first(&files, "/dev/pts").unwrap(), None);
}

#[test]
fn a_file_at_the_last_name_is_no_link() {
    let files = files();
    files.file(below("etc/resolv.conf"), "nameserver 127.0.0.53\n");
    assert_eq!(first(&files, "/etc/resolv.conf").unwrap(), None);
}

#[test]
fn a_name_that_cannot_be_read_is_an_error() {
    let files = files();
    files.fail(below("etc"), io::ErrorKind::PermissionDenied);
    let error = first(&files, "/etc/resolv.conf").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn a_real_link_is_found_with_the_real_files() {
    let dir = zbm_testkit::TempDir::new("zbm-inside");
    let root = dir.join("root");
    dir.write("root/etc/file", "");
    std::os::unix::fs::symlink("/etc", root.join("tmp")).unwrap();
    let files = zbm_platform::RealFiles;
    let walk = |path: &str| {
        InRoot::new(Path::new(path))
            .unwrap()
            .first_link(&files, &root)
    };
    assert_eq!(
        walk("/tmp/x").unwrap(),
        Some((in_root("/tmp"), PathBuf::from("/etc")))
    );
    // A directory and a file are no link (`EINVAL`), a missing name ends
    // the walk (`ENOENT`), and so does a name below a file (`ENOTDIR`).
    assert_eq!(walk("/etc/file").unwrap(), None);
    assert_eq!(walk("/etc/missing/x").unwrap(), None);
    assert_eq!(walk("/etc/file/x").unwrap(), None);
}

/// The names of the random trees: a few, among them those the chroot
/// covers, so that walks meet them.
const NAMES: [&str; 6] = ["a", "b", "etc", "run", "tmp", "devices"];

/// A name of [`NAMES`].
fn name() -> impl Strategy<Value = String> {
    proptest::sample::select(NAMES.as_slice()).prop_map(str::to_owned)
}

/// A relative path of one to three names.
fn relative() -> impl Strategy<Value = String> {
    proptest::collection::vec(name(), 1..4).prop_map(|names| names.join("/"))
}

/// A link's target: absolute or relative, with `..` and `.` among the
/// names.
fn target() -> impl Strategy<Value = String> {
    let part = prop_oneof![name(), Just("..".to_owned()), Just(".".to_owned())];
    (any::<bool>(), proptest::collection::vec(part, 0..5)).prop_map(|(absolute, parts)| {
        let joined = parts.join("/");
        if absolute {
            format!("/{joined}")
        } else {
            joined
        }
    })
}

/// What an entry of a random tree is.
#[derive(Clone, Debug)]
enum Entry {
    Dir,
    File,
    Link(String),
}

fn entry() -> impl Strategy<Value = Entry> {
    prop_oneof![
        Just(Entry::Dir),
        Just(Entry::File),
        target().prop_map(Entry::Link),
    ]
}

/// Adds `entries` below `base` in `files`, each where its path says.
fn add(files: &FakeFiles, base: &Path, entries: &[(String, Entry)]) {
    for (path, entry) in entries {
        let path = base.join(path);
        match entry {
            Entry::Dir => {
                files.dir(&path);
            }
            Entry::File => {
                files.file(&path, "");
            }
            Entry::Link(target) => {
                files.symlink(&path, target);
            }
        }
    }
}

/// Whether `path`, below [`ROOT`], is free of links in `files`, and has
/// no `..`: each of its names below the root is read, shortest first.
fn is_link_free(files: &FakeFiles, path: &Path) -> bool {
    let Ok(rest) = path.strip_prefix(ROOT) else {
        return false;
    };
    let mut at = PathBuf::from(ROOT);
    rest.components().all(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        at.push(name);
        files.read_link(&at).is_err()
    })
}

proptest! {
    #[test]
    fn no_resolved_path_leaves_the_root(
        inner in proptest::collection::vec((relative(), entry()), 0..12),
        outer in proptest::collection::vec((relative(), entry()), 0..6),
        path in target(),
    ) {
        let files = FakeFiles::new();
        files.dir(ROOT);
        add(&files, Path::new("/"), &outer);
        add(&files, Path::new(ROOT), &inner);
        let path = format!("/{path}");
        // The path, and whether its last name was read: a covered one is
        // refused before.
        let found = match resolve(&files, Path::new(ROOT), Path::new(&path)) {
            Ok(found) => Some((found.path, true)),
            Err(Unresolved::Missing { path, .. } | Unresolved::NotADirectory(path)) => {
                Some((path, true))
            }
            Err(Unresolved::Covered(path)) => Some((path, false)),
            Err(Unresolved::TooManyLinks | Unresolved::Io(_)) => None,
        };
        if let Some((found, read)) = found {
            let under = found.under(Path::new(ROOT));
            prop_assert!(under.starts_with(ROOT), "{}", under.display());
            prop_assert!(
                under.strip_prefix(ROOT).is_ok_and(|rest| rest
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))),
                "{}",
                under.display()
            );
            // No name read on the way is a link.
            let checked = if read { under.as_path() } else {
                under.parent().unwrap_or(Path::new(ROOT))
            };
            prop_assert!(is_link_free(&files, checked), "{}", under.display());
        }
    }
}

proptest! {
    #[test]
    fn the_first_link_is_the_only_link_read_and_none_means_link_free(
        inner in proptest::collection::vec((relative(), entry()), 0..12),
        outer in proptest::collection::vec((relative(), entry()), 0..6),
        path in relative(),
    ) {
        let files = FakeFiles::new();
        files.dir(ROOT);
        add(&files, Path::new("/"), &outer);
        add(&files, Path::new(ROOT), &inner);
        let names = InRoot::new(Path::new(&path)).unwrap();
        let under = names.under(Path::new(ROOT));
        match names.first_link(&files, Path::new(ROOT)) {
            Ok(None) => prop_assert!(is_link_free(&files, &under), "{}", under.display()),
            Ok(Some((link, target))) => {
                let at = link.under(Path::new(ROOT));
                prop_assert!(under.starts_with(&at), "{}", at.display());
                prop_assert_eq!(files.read_link(&at).ok(), Some(target));
                // No shorter prefix is a link.
                let parent = at.parent().unwrap_or(Path::new(ROOT));
                prop_assert!(is_link_free(&files, parent), "{}", at.display());
            }
            Err(error) => prop_assert!(false, "{error}"),
        }
    }
}
