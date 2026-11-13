//! The directories of the filesystems' state: never shared, never inside
//! another's (SB19).

use std::path::{Component, Path, PathBuf};

use proptest::prelude::*;
use zbm_platform::Files as _;
use zbm_testkit::fakes::FakeFiles;

use crate::{BASE, State};

/// The directory of every filesystem's state.
const E: &str = "/zfsbootmenu/environments";

fn below(rest: &str) -> PathBuf {
    Path::new(E).join(rest)
}

/// The paths of the state of `name`: its mount point, its kernel list and
/// its command line cache.
fn paths(state: &State<'_>, name: &str) -> [PathBuf; 3] {
    let cache = state.kcl_cache(name, "x").expect("a kernel");
    let cache = cache.parent().expect("a directory").to_path_buf();
    [state.mount_point(name), state.kernels(name), cache]
}

/// Whether `path` is below [`E`], with only names below it.
fn is_below_the_environments(path: &Path) -> bool {
    path.strip_prefix(E).is_ok_and(|rest| {
        rest.components().count() > 0
            && rest
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
    })
}

#[test]
fn a_child_named_mnt_has_a_directory_of_its_own() {
    let files = FakeFiles::new();
    let state = State::new(&files, BASE);
    assert_eq!(state.environments(), Path::new(E));
    assert_eq!(state.mount_point("p/a/mnt"), below("p/a/+mnt/mnt"));
    assert_eq!(state.kernels("p/a/mnt"), below("p/a/+mnt/kernels"));
    assert_eq!(
        state.kcl_cache("p/a/mnt", "/boot/vmlinuz"),
        Some(below("p/a/+mnt/kcl/boot:vmlinuz.kcl"))
    );
    assert_eq!(state.mount_point("p/a"), below("p/a/mnt"));
    let made = state.environment("p/a/mnt").unwrap();
    assert_eq!(made, below("p/a/+mnt"));
    assert!(files.is_dir(&made));
}

#[test]
fn children_named_kernels_or_kcl_are_escaped_too() {
    let files = FakeFiles::new();
    let state = State::new(&files, BASE);
    assert_eq!(state.kernels("p/a/kernels"), below("p/a/+kernels/kernels"));
    assert_eq!(state.mount_point("p/kcl/b"), below("p/+kcl/b/mnt"));
    assert_eq!(
        state.kcl_cache("p/a/kcl/default.kcl", "/boot/x"),
        Some(below("p/a/+kcl/default.kcl/kcl/boot:x.kcl"))
    );
    // One rule for every piece, the pool's too.
    assert_eq!(state.mount_point("mnt/ROOT"), below("+mnt/ROOT/mnt"));
    // A snapshot's piece holds `@`, so it is never one of the three.
    assert_eq!(state.mount_point("p/a/mnt@s"), below("p/a/mnt@s/mnt"));
}

#[test]
fn ordinary_names_keep_the_legacy_paths() {
    let files = FakeFiles::new();
    let state = State::new(&files, BASE);
    for name in [
        "zroot/ROOT/void",
        "zroot/ROOT/void@snap",
        "bpool/BOOT/debian",
        "p/a b",
        "tank/mnt-old/kcl2",
    ] {
        assert_eq!(state.mount_point(name), below(&format!("{name}/mnt")));
        assert_eq!(state.kernels(name), below(&format!("{name}/kernels")));
        assert_eq!(
            state.kcl_cache(name, "/boot/vmlinuz"),
            Some(below(&format!("{name}/kcl/boot:vmlinuz.kcl")))
        );
    }
}

#[test]
fn a_name_that_is_no_dataset_name_stays_below_its_directory() {
    let files = FakeFiles::new();
    let state = State::new(&files, BASE);
    for (name, directory) in [
        ("../x", "+../x"),
        ("a//b", "a/+/b"),
        ("+y", "++y"),
        (".", "+."),
        ("/etc", "+/etc"),
        ("a/", "a/+"),
        ("", "+"),
    ] {
        let mnt = state.mount_point(name);
        assert_eq!(mnt, below(&format!("{directory}/mnt")), "{name:?}");
        for path in paths(&state, name) {
            assert!(is_below_the_environments(&path), "{name:?}: {path:?}");
        }
    }
}

/// A piece of a name, or of a text that is no name.
fn piece() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "a", "b", "a b", "mnt", "kernels", "kcl", "m:1", "", ".", "..", "+x", "++x",
    ])
}

/// A text made of [`piece`]s joined by `/`, with an optional leading or
/// trailing `/` and an optional snapshot.
fn name() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(piece(), 1..5),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(pieces, leading, trailing, snapshot)| {
            let mut name = pieces.join("/");
            if leading {
                name.insert(0, '/');
            }
            if trailing {
                name.push('/');
            }
            if snapshot {
                name.push_str("@s");
            }
            name
        })
}

proptest! {
    #[test]
    fn no_two_filesystems_share_or_nest_a_state_path(one in name(), other in name()) {
        prop_assume!(one != other);
        let files = FakeFiles::new();
        let state = State::new(&files, BASE);
        let (ones, others) = (paths(&state, &one), paths(&state, &other));
        for path in ones.iter().chain(&others) {
            prop_assert!(is_below_the_environments(path), "{path:?}");
        }
        for a in &ones {
            for b in &others {
                prop_assert!(!b.starts_with(a), "{one:?}: {a:?} holds {other:?}: {b:?}");
                prop_assert!(!a.starts_with(b), "{other:?}: {b:?} holds {one:?}: {a:?}");
            }
        }
    }
}
