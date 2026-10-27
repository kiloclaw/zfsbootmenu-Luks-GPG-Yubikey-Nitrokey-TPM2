use std::collections::BTreeMap;
use std::path::Path;

use super::*;

/// A package with `features` (`name`, what it turns on) and the features
/// it asks of its dependencies.
fn package(features: &[(&str, &[&str])], dependencies: &[(&str, &[&str])]) -> Features {
    let owned = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect();
    Features {
        features: features
            .iter()
            .map(|(name, on)| ((*name).to_owned(), owned(on)))
            .collect(),
        dependencies: dependencies
            .iter()
            .map(|(name, on)| ((*name).to_owned(), owned(on)))
            .collect(),
    }
}

/// The workspace as it is: the test features declared, none turned on.
fn workspace() -> BTreeMap<String, Features> {
    BTreeMap::from([
        (
            "zbm".to_owned(),
            package(
                &[
                    ("console-mirror", &["zbm-startup/console-mirror"]),
                    ("crash-test", &[]),
                    ("menu-demo", &[]),
                    ("secret-test", &[]),
                ],
                &[("zbm-startup", &[])],
            ),
        ),
        (
            "zbm-startup".to_owned(),
            package(&[("console-mirror", &[])], &[]),
        ),
    ])
}

#[test]
fn the_workspace_as_it_is_passes() {
    assert_eq!(feature_problems(&workspace()), Vec::<String>::new());
}

#[test]
fn a_default_that_turns_on_a_test_feature_fails() {
    let mut all = workspace();
    let zbm = all.get_mut("zbm").unwrap();
    zbm.features.insert("default".into(), vec!["extras".into()]);
    zbm.features
        .insert("extras".into(), vec!["zbm-startup?/console-mirror".into()]);

    let problems = feature_problems(&all);

    assert_eq!(
        problems,
        [
            "zbm's feature default turns on the test feature zbm-startup/console-mirror",
            "zbm's feature extras turns on the test feature zbm-startup/console-mirror",
        ]
    );
}

#[test]
fn a_dependency_with_a_test_feature_and_a_lost_feature_fail() {
    let mut all = workspace();
    all.get_mut("zbm").unwrap().features.remove("menu-demo");
    all.insert(
        "zbm-tui".into(),
        package(&[], &[("zbm-startup", &["console-mirror"])]),
    );

    let problems = feature_problems(&all);

    assert_eq!(
        problems,
        [
            "zbm has no feature menu-demo: update xtask's TEST_FEATURES",
            "zbm-tui depends on zbm-startup with its test feature console-mirror",
        ]
    );
}

#[test]
fn files_that_build_releases_may_not_turn_features_on() {
    let workflow = "run: cargo build --release\nrun: cargo build --features zbm/secret-test\n";

    assert_eq!(
        text_problems(Path::new(".github/workflows/build.yml"), workflow),
        [".github/workflows/build.yml:2: turns features on in a file that builds releases"]
    );
    assert!(text_problems(Path::new("x"), "cargo build --release --locked\n").is_empty());
}
