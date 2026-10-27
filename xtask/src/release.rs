//! Release builds carry no test features (ROADMAP.md SE2).
//!
//! The test features let the VM tests into what a release must not do:
//! `zbm debug panic`, the search for secrets, the menu on the display
//! without a terminal, a copy of the screen in a file. Only the VM tests'
//! builds turn them on, through `ZBM_CARGO_FEATURES` of
//! `packaging/container/build-init.sh`. The check fails when
//!
//! - a feature of [`TEST_FEATURES`] is gone, so that the list is wrong;
//! - another feature, `default` among them, turns one on;
//! - a crate depends on another with one of them;
//! - a file that builds releases, a workflow or a packaging script, sets
//!   features at all.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::process::Program;

/// The test features, as `(package, feature)`.
pub const TEST_FEATURES: &[(&str, &str)] = &[
    ("zbm", "console-mirror"),
    ("zbm", "crash-test"),
    ("zbm", "menu-demo"),
    ("zbm", "secret-test"),
    ("zbm-startup", "console-mirror"),
];

/// The only file that may pass features to a build: it takes them from
/// the VM tests.
const FEATURES_FROM_TESTS: &str = "packaging/container/build-init.sh";

/// What turns features on in a file that builds.
const FEATURE_WORDS: &[&str] = &["--features", "--all-features", "ZBM_CARGO_FEATURES"];

/// The directories whose files build releases.
const RELEASE_DIRS: &[&str] = &[".github/workflows", "packaging", "releng"];

/// Runs the check over the workspace at `root`.
///
/// # Errors
///
/// [`Error::Check`] with each problem, and the errors of `cargo metadata`.
pub fn check(root: &Path) -> Result<()> {
    let json = Program::new("cargo", root)
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .output()?;
    let metadata: Value = serde_json::from_str(&json)
        .map_err(|error| Error::Format(format!("cargo metadata: {error}")))?;
    let mut problems = feature_problems(&packages(&metadata));
    for dir in RELEASE_DIRS {
        problems.extend(build_file_problems(root, &root.join(dir)));
    }
    if problems.is_empty() {
        println!("test features: release builds turn none on");
        return Ok(());
    }
    Err(Error::Check {
        check: "test features",
        problems,
    })
}

/// A package's features and the features it asks of its dependencies.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Features {
    /// Each feature and what it turns on.
    pub features: BTreeMap<String, Vec<String>>,
    /// Each dependency and the features asked of it.
    pub dependencies: Vec<(String, Vec<String>)>,
}

/// The packages of `cargo metadata`'s JSON, by name.
fn packages(metadata: &Value) -> BTreeMap<String, Features> {
    let strings = |value: Option<&Value>| -> Vec<String> {
        value
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut packages = BTreeMap::new();
    for package in metadata["packages"].as_array().into_iter().flatten() {
        let Some(name) = package["name"].as_str() else {
            continue;
        };
        let features = package["features"]
            .as_object()
            .map(|features| {
                features
                    .iter()
                    .map(|(name, on)| (name.clone(), strings(Some(on))))
                    .collect()
            })
            .unwrap_or_default();
        let dependencies = package["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|dependency| dependency["kind"].is_null())
            .filter_map(|dependency| {
                let name = dependency["name"].as_str()?.to_owned();
                Some((name, strings(dependency.get("features"))))
            })
            .collect();
        packages.insert(
            name.to_owned(),
            Features {
                features,
                dependencies,
            },
        );
    }
    packages
}

/// Whether `feature` of `package` is a test feature.
fn is_test(package: &str, feature: &str) -> bool {
    TEST_FEATURES.contains(&(package, feature))
}

/// The features that `feature` of `package` turns on, itself included, as
/// `(package, feature)`, across the packages of `all`.
fn turned_on(
    all: &BTreeMap<String, Features>,
    package: &str,
    feature: &str,
) -> BTreeSet<(String, String)> {
    let mut seen = BTreeSet::new();
    let mut pending = vec![(package.to_owned(), feature.to_owned())];
    while let Some((package, feature)) = pending.pop() {
        if !seen.insert((package.clone(), feature.clone())) {
            continue;
        }
        let Some(on) = all.get(&package).and_then(|p| p.features.get(&feature)) else {
            continue;
        };
        for entry in on {
            if entry.starts_with("dep:") {
                continue;
            }
            match entry.split_once('/') {
                Some((dependency, feature)) => {
                    let dependency = dependency.trim_end_matches('?');
                    pending.push((dependency.to_owned(), feature.to_owned()));
                }
                None => pending.push((package.clone(), entry.clone())),
            }
        }
    }
    seen
}

/// The problems of the features of `all`.
pub fn feature_problems(all: &BTreeMap<String, Features>) -> Vec<String> {
    let mut problems = Vec::new();
    for (package, feature) in TEST_FEATURES {
        let declared = all
            .get(*package)
            .is_some_and(|p| p.features.contains_key(*feature));
        if !declared {
            problems.push(format!(
                "{package} has no feature {feature}: update xtask's TEST_FEATURES"
            ));
        }
    }
    for (package, features) in all {
        for feature in features.features.keys() {
            if is_test(package, feature) {
                continue;
            }
            for (on_package, on) in turned_on(all, package, feature) {
                if is_test(&on_package, &on) {
                    problems.push(format!(
                        "{package}'s feature {feature} turns on the test feature {on_package}/{on}"
                    ));
                }
            }
        }
        for (dependency, asked) in &features.dependencies {
            for feature in asked {
                let reached = turned_on(all, dependency, feature);
                if reached.iter().any(|(p, f)| is_test(p, f)) {
                    problems.push(format!(
                        "{package} depends on {dependency} with its test feature {feature}"
                    ));
                }
            }
        }
    }
    problems
}

/// The problems of the files below `dir`, which build releases.
fn build_file_problems(root: &Path, dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let mut pending: Vec<PathBuf> = vec![dir.to_owned()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let relative = path.strip_prefix(root).unwrap_or(&path);
            if relative == Path::new(FEATURES_FROM_TESTS) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            problems.extend(text_problems(relative, &text));
        }
    }
    problems.sort();
    problems
}

/// The lines of `text`, the file `path`, that turn features on.
pub fn text_problems(path: &Path, text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| FEATURE_WORDS.iter().any(|word| line.contains(word)))
        .map(|(index, _)| {
            format!(
                "{}:{}: turns features on in a file that builds releases",
                path.display(),
                index + 1
            )
        })
        .collect()
}

#[cfg(test)]
mod tests;
