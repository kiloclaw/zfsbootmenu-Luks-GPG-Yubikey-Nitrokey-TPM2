use super::*;
use crate::workspace::Dependency;
use std::path::PathBuf;

fn package(name: &str, dependencies: &[(&str, DependencyKind, bool)]) -> Package {
    Package {
        name: name.to_owned(),
        dir: PathBuf::from(format!("/w/crates/{name}")),
        targets: Vec::new(),
        dependencies: dependencies
            .iter()
            .map(|(name, kind, member)| Dependency {
                name: (*name).to_owned(),
                kind: *kind,
                member: *member,
            })
            .collect(),
    }
}

fn problems_of(package: &Package) -> Vec<String> {
    package_problems(package)
}

#[test]
fn a_crate_may_depend_on_lower_layers() {
    let luks = package("zbm-luks", &[("zbm-trace", DependencyKind::Normal, true)]);

    assert_eq!(problems_of(&luks), Vec::<String>::new());
}

#[test]
fn a_crate_may_not_depend_on_a_higher_layer() {
    let trace = package("zbm-trace", &[("zbm-boot", DependencyKind::Normal, true)]);

    assert_eq!(
        problems_of(&trace),
        ["zbm-trace -> zbm-boot: depends on a higher layer"]
    );
}

#[test]
fn same_layer_edges_must_be_listed() {
    let listed = package("zbm-boot", &[("zbm-pool", DependencyKind::Normal, true)]);
    let unlisted = package("zbm-pool", &[("zbm-boot", DependencyKind::Normal, true)]);

    assert_eq!(problems_of(&listed), Vec::<String>::new());
    assert_eq!(
        problems_of(&unlisted),
        ["zbm-pool -> zbm-boot: same layer, and not listed in SAME_LAYER"]
    );
}

#[test]
fn test_support_is_only_a_development_dependency() {
    let dev = package("zbm-luks", &[("zbm-testkit", DependencyKind::Dev, true)]);
    let normal = package("zbm-luks", &[("zbm-testkit", DependencyKind::Normal, true)]);

    assert_eq!(problems_of(&dev), Vec::<String>::new());
    assert_eq!(
        problems_of(&normal),
        ["zbm-luks -> zbm-testkit: test support crates may only be development dependencies"]
    );
}

#[test]
fn third_party_crates_must_be_listed_per_crate() {
    let allowed = package(
        "generate-zbm",
        &[("yaml-rust2", DependencyKind::Normal, false)],
    );
    let refused = package("zbm-luks", &[("yaml-rust2", DependencyKind::Normal, false)]);

    assert_eq!(problems_of(&allowed), Vec::<String>::new());
    assert_eq!(
        problems_of(&refused),
        ["zbm-luks -> yaml-rust2: third-party crate not allowed in THIRD_PARTY"]
    );
}

#[test]
fn test_crates_are_allowed_as_development_dependencies_only() {
    let dev = package("zbm-luks", &[("insta", DependencyKind::Dev, false)]);
    let normal = package("zbm-luks", &[("insta", DependencyKind::Normal, false)]);

    assert_eq!(problems_of(&dev), Vec::<String>::new());
    assert_eq!(
        problems_of(&normal),
        ["zbm-luks -> insta: third-party crate not allowed in THIRD_PARTY"]
    );
}

#[test]
fn front_ends_go_through_the_application_layer() {
    let menu = package("zbm-tui", &[("zbm-zfs", DependencyKind::Normal, true)]);
    let startup = package(
        "zbm-startup",
        &[("zbm-tools", DependencyKind::Normal, true)],
    );

    assert_eq!(
        problems_of(&menu),
        ["zbm-tui -> zbm-zfs: front ends may not use adapters: go through zbm-app"]
    );
    assert_eq!(problems_of(&startup), Vec::<String>::new());

    let console = package("zbm-console", &[("zbm-boot", DependencyKind::Normal, true)]);
    let in_tests = package("zbm-console", &[("zbm-boot", DependencyKind::Dev, true)]);
    let binary = package("zbm", &[("zbm-boot", DependencyKind::Normal, true)]);
    assert_eq!(
        problems_of(&console),
        [format!("zbm-console -> zbm-boot: {BEHIND}")]
    );
    assert_eq!(problems_of(&in_tests), Vec::<String>::new());
    assert_eq!(problems_of(&binary), Vec::<String>::new());
}

#[test]
fn nothing_depends_on_tools() {
    let zbm = package("zbm", &[("xtask", DependencyKind::Dev, true)]);

    assert_eq!(
        problems_of(&zbm),
        ["zbm -> xtask: no crate may depend on a developer tool"]
    );
}

#[test]
fn crates_missing_from_the_tables_are_reported() {
    let workspace = Workspace {
        root: PathBuf::from("/w"),
        packages: vec![package("zbm-new", &[])],
    };

    let problems = problems(&workspace);

    assert_eq!(
        problems[0],
        "zbm-new is in no layer: add it to xtask/src/deps.rs"
    );
    assert!(
        problems.contains(&"zbm-sys is in xtask/src/deps.rs but not in the workspace".to_owned())
    );
}

#[test]
fn the_generated_section_replaces_only_the_text_between_the_markers() {
    let page = format!("# Title\n\n{BEGIN}\nold\n{END}\n\nAfter.\n");

    let new = replace_section(&page, "new\n").unwrap();

    assert_eq!(
        new,
        format!("# Title\n\n{BEGIN}\n\nnew\n\n{END}\n\nAfter.\n")
    );
}

#[test]
fn the_generated_section_lists_layers_from_the_top() {
    let text = markdown();

    let front_ends = text.find("| front ends |").unwrap();
    let unsafe_layer = text.find("| unsafe |").unwrap();
    assert!(front_ends < unsafe_layer);
}
