//! The packages of the workspace, as `cargo metadata` describes them.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::process::Program;

/// The workspace: its root directory and its member packages.
#[derive(Clone, Debug)]
pub struct Workspace {
    /// The directory of the workspace's `Cargo.toml`.
    pub root: PathBuf,
    /// The member packages, sorted by name.
    pub packages: Vec<Package>,
}

/// One member package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Package {
    /// The package name, for example `zbm-luks`.
    pub name: String,
    /// The directory of the package's `Cargo.toml`.
    pub dir: PathBuf,
    /// The targets: library, binaries, tests.
    pub targets: Vec<Target>,
    /// The dependencies as the manifest declares them.
    pub dependencies: Vec<Dependency>,
}

/// One target of a package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Target {
    /// What the target builds.
    pub kind: TargetKind,
    /// The root source file.
    pub src: PathBuf,
}

/// What a target builds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetKind {
    /// The library.
    Lib,
    /// A binary.
    Bin,
    /// An integration test in `tests/`.
    Test,
    /// A benchmark, an example or a build script, which `xtask` ignores.
    Other,
}

/// One declared dependency.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dependency {
    /// The package it names.
    pub name: String,
    /// Normal, development or build dependency.
    pub kind: DependencyKind,
    /// Whether it is a member of the workspace.
    pub member: bool,
}

/// The section of `Cargo.toml` a dependency is declared in.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DependencyKind {
    /// `[dependencies]`.
    Normal,
    /// `[dev-dependencies]`.
    Dev,
    /// `[build-dependencies]`.
    Build,
}

impl Workspace {
    /// Reads the workspace that contains `dir` with `cargo metadata`.
    ///
    /// # Errors
    ///
    /// When `cargo metadata` fails or prints something unexpected.
    pub fn load(dir: &Path) -> Result<Workspace> {
        let json = Program::new("cargo", dir)
            .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
            .output()?;
        let value: Value = serde_json::from_str(&json)
            .map_err(|error| Error::Format(format!("cargo metadata: {error}")))?;
        Workspace::from_metadata(&value)
    }

    /// Builds the workspace from the JSON of `cargo metadata --no-deps`.
    ///
    /// # Errors
    ///
    /// [`Error::Format`] when a field is missing.
    pub fn from_metadata(value: &Value) -> Result<Workspace> {
        let root = PathBuf::from(text(value, "workspace_root")?);
        let members: Vec<&str> = array(value, "workspace_members")?
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let mut member_values = Vec::new();
        for package in array(value, "packages")? {
            if members.contains(&text(package, "id")?) {
                member_values.push(package);
            }
        }
        let member_dirs = member_values
            .iter()
            .map(|package| manifest_dir(package))
            .collect::<Result<Vec<_>>>()?;
        let mut packages = member_values
            .iter()
            .map(|package| Package::from_metadata(package, &member_dirs))
            .collect::<Result<Vec<_>>>()?;
        packages.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Workspace { root, packages })
    }

    /// The package named `name`.
    pub fn package(&self, name: &str) -> Option<&Package> {
        self.packages.iter().find(|package| package.name == name)
    }
}

impl Package {
    /// One entry of the `packages` array of `cargo metadata`.
    fn from_metadata(value: &Value, member_dirs: &[PathBuf]) -> Result<Package> {
        let targets = array(value, "targets")?
            .iter()
            .map(Target::from_metadata)
            .collect::<Result<_>>()?;
        let dependencies = array(value, "dependencies")?
            .iter()
            .map(|dependency| Dependency::from_metadata(dependency, member_dirs))
            .collect::<Result<_>>()?;
        Ok(Package {
            name: text(value, "name")?.to_owned(),
            dir: manifest_dir(value)?,
            targets,
            dependencies,
        })
    }
}

impl Target {
    /// One entry of a package's `targets` array.
    fn from_metadata(value: &Value) -> Result<Target> {
        let kinds = array(value, "kind")?;
        let has = |kind: &str| kinds.iter().any(|value| value.as_str() == Some(kind));
        let kind = if has("lib") || has("rlib") || has("proc-macro") {
            TargetKind::Lib
        } else if has("bin") {
            TargetKind::Bin
        } else if has("test") {
            TargetKind::Test
        } else {
            TargetKind::Other
        };
        Ok(Target {
            kind,
            src: PathBuf::from(text(value, "src_path")?),
        })
    }
}

impl Dependency {
    /// One entry of a package's `dependencies` array.
    ///
    /// A dependency is a member when its `path` is the directory of a
    /// member package.
    fn from_metadata(value: &Value, member_dirs: &[PathBuf]) -> Result<Dependency> {
        let name = text(value, "name")?.to_owned();
        let kind = match value.get("kind").and_then(Value::as_str) {
            None => DependencyKind::Normal,
            Some("dev") => DependencyKind::Dev,
            Some("build") => DependencyKind::Build,
            Some(other) => {
                return Err(Error::Format(format!("unknown dependency kind {other}")));
            }
        };
        let member = value
            .get("path")
            .and_then(Value::as_str)
            .is_some_and(|path| member_dirs.iter().any(|dir| dir == Path::new(path)));
        Ok(Dependency { name, kind, member })
    }
}

/// The directory of a package's `Cargo.toml`.
fn manifest_dir(package: &Value) -> Result<PathBuf> {
    let manifest = Path::new(text(package, "manifest_path")?);
    manifest
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| Error::Format(format!("{} has no directory", manifest.display())))
}

/// The string field `key` of `value`.
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Format(format!("cargo metadata: no string field {key}")))
}

/// The array field `key` of `value`.
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Format(format!("cargo metadata: no array field {key}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Metadata of a workspace with one library that has a path dependency
    /// on a member, a registry dependency and a development dependency.
    fn metadata() -> Value {
        serde_json::json!({
            "workspace_root": "/w",
            "workspace_members": [
                "path+file:///w/crates/app#0.1.0",
                "path+file:///w/crates/zbm-trace#0.1.0"
            ],
            "packages": [
                {
                    "id": "path+file:///w/crates/app#0.1.0",
                    "name": "app",
                    "manifest_path": "/w/crates/app/Cargo.toml",
                    "targets": [
                        { "kind": ["lib"], "src_path": "/w/crates/app/src/lib.rs" },
                        { "kind": ["test"], "src_path": "/w/crates/app/tests/it.rs" }
                    ],
                    "dependencies": [
                        { "name": "zbm-trace", "kind": null, "path": "/w/crates/zbm-trace" },
                        { "name": "syn", "kind": null },
                        { "name": "zbm-trace", "kind": "dev", "path": "/w/crates/zbm-trace" }
                    ]
                },
                {
                    "id": "path+file:///w/crates/zbm-trace#0.1.0",
                    "name": "zbm-trace",
                    "manifest_path": "/w/crates/zbm-trace/Cargo.toml",
                    "targets": [],
                    "dependencies": []
                }
            ]
        })
    }

    #[test]
    fn packages_are_read_with_targets_and_dependencies() {
        let workspace = Workspace::from_metadata(&metadata()).unwrap();

        let app = workspace.package("app").unwrap();
        assert_eq!(app.dir, Path::new("/w/crates/app"));
        assert_eq!(app.targets[0].kind, TargetKind::Lib);
        assert_eq!(app.targets[1].kind, TargetKind::Test);
        assert_eq!(
            app.dependencies,
            [
                Dependency {
                    name: "zbm-trace".to_owned(),
                    kind: DependencyKind::Normal,
                    member: true,
                },
                Dependency {
                    name: "syn".to_owned(),
                    kind: DependencyKind::Normal,
                    member: false,
                },
                Dependency {
                    name: "zbm-trace".to_owned(),
                    kind: DependencyKind::Dev,
                    member: true,
                },
            ]
        );
    }

    #[test]
    fn a_missing_field_is_a_format_error() {
        let error = Workspace::from_metadata(&serde_json::json!({})).unwrap_err();

        assert_eq!(
            error.to_string(),
            "cargo metadata: no string field workspace_root"
        );
    }
}
