//! A fake `zfs` command for parity tests.
//!
//! Parity tests run legacy scripts and Rust code against the same fake, so both
//! see the same datasets and properties. The fake keeps datasets and user
//! properties in a directory and follows the inheritance rules of ZFS user
//! properties: a property not set on a dataset is inherited from the nearest
//! ancestor that sets it, with the source `inherited from <ancestor>`, and is
//! `-` when no ancestor sets it.
//!
//! It supports the commands ZFSBootMenu's host tools run:
//!
//! - `zfs list [-H] [-o name] [-t filesystem] <dataset>`
//! - `zfs get -H -o value|source [-s inherited] <property> <dataset>`
//! - `zfs set <property>=<value> <dataset>`
//! - `zfs inherit <property> <dataset>`
//!
//! Every call is appended to the file `calls` in the fake's directory.
#![allow(
    clippy::disallowed_methods,
    reason = "the fake is a real program that legacy scripts run, \
              keeping its datasets in real files"
)]

use std::path::{Path, PathBuf};

use crate::TempDir;

const SCRIPT: &str = r#"#!/bin/sh
# Fake zfs for parity tests; see zbm-testkit/src/zfs_script.rs
data="$(dirname "$0")/.."
printf '%s\n' "$*" >> "${data}/calls"

exists() { grep -qxF "$1" "${data}/datasets"; }
propfile() { printf '%s/props/%s/%s' "${data}" "$(printf '%s' "$1" | sed 's|/|%|g')" "$2"; }
missing() { echo "cannot open '$1': dataset does not exist" >&2; exit 1; }

cmd="$1"; shift
case "${cmd}" in
  list)
    ds=
    while [ $# -gt 0 ]; do
      case "$1" in
        -H) ;;
        -o|-t) shift ;;
        *) ds="$1" ;;
      esac
      shift
    done
    exists "${ds}" || missing "${ds}"
    printf '%s\n' "${ds}"
    ;;
  get)
    field=value; only=
    while [ $# -gt 2 ]; do
      case "$1" in
        -H) ;;
        -o) shift; field="$1" ;;
        -s) shift; only="$1" ;;
      esac
      shift
    done
    prop="$1"; ds="$2"
    exists "${ds}" || missing "${ds}"
    cur="${ds}"; value=-; source=-
    while :; do
      if [ -f "$(propfile "${cur}" "${prop}")" ]; then
        value="$(cat "$(propfile "${cur}" "${prop}")")"
        if [ "${cur}" = "${ds}" ]; then source=local; else source="inherited from ${cur}"; fi
        break
      fi
      case "${cur}" in */*) cur="${cur%/*}" ;; *) break ;; esac
    done
    if [ -n "${only}" ]; then
      case "${source}" in "${only}"*) ;; *) exit 0 ;; esac
    fi
    if [ "${field}" = source ]; then printf '%s\n' "${source}"; else printf '%s\n' "${value}"; fi
    ;;
  set)
    assignment="$1"; ds="$2"
    exists "${ds}" || missing "${ds}"
    file="$(propfile "${ds}" "${assignment%%=*}")"
    mkdir -p "$(dirname "${file}")"
    printf '%s' "${assignment#*=}" > "${file}"
    ;;
  inherit)
    prop="$1"; ds="$2"
    exists "${ds}" || missing "${ds}"
    rm -f "$(propfile "${ds}" "${prop}")"
    ;;
  *)
    echo "fake zfs: unsupported command ${cmd}" >&2
    exit 2
    ;;
esac
"#;

/// A fake `zfs` with its datasets and properties.
#[derive(Debug)]
pub struct ZfsScript {
    dir: TempDir,
}

impl ZfsScript {
    /// A fake that knows the given datasets, with no properties set.
    pub fn new(datasets: &[&str]) -> ZfsScript {
        let dir = TempDir::new("zbm-fake-zfs");
        let mut list: String = datasets.iter().flat_map(|name| [*name, "\n"]).collect();
        if list.is_empty() {
            list.push('\n');
        }
        dir.write("datasets", list);
        dir.write("calls", "");
        dir.write_executable("bin/zfs", SCRIPT);
        ZfsScript { dir }
    }

    /// The fake `zfs` program.
    pub fn program(&self) -> PathBuf {
        self.dir.join("bin/zfs")
    }

    /// The directory holding the fake program, to put first in `PATH`.
    pub fn bin_dir(&self) -> PathBuf {
        self.dir.join("bin")
    }

    /// `PATH` with the fake's directory first.
    pub fn path_env(&self) -> String {
        format!(
            "{}:{}",
            self.bin_dir().display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    /// The directory of the fake's data.
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    fn prop_path(&self, dataset: &str, property: &str) -> PathBuf {
        self.dir
            .join("props")
            .join(dataset.replace('/', "%"))
            .join(property)
    }

    /// Sets a property locally on a dataset.
    pub fn set(&self, dataset: &str, property: &str, value: &str) {
        let path = self.prop_path(dataset, property);
        std::fs::create_dir_all(path.parent().expect("property files have a parent"))
            .expect("the property directory can be created");
        std::fs::write(path, value).expect("the property can be written");
    }

    /// The local value of a property, if it is set on the dataset.
    pub fn local(&self, dataset: &str, property: &str) -> Option<String> {
        std::fs::read_to_string(self.prop_path(dataset, property)).ok()
    }

    /// The commands run so far, one per line.
    pub fn calls(&self) -> String {
        std::fs::read_to_string(self.dir.join("calls")).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_platform::exec::{Command, Runner, Stream, SystemRunner};

    fn run(fake: &ZfsScript, args: &[&str]) -> (i32, String) {
        let output = SystemRunner
            .run(&Command::new(fake.program()).args(args).stderr(Stream::Null))
            .unwrap();
        (output.status.shell_code(), output.stdout_text())
    }

    #[test]
    fn properties_are_inherited() {
        let fake = ZfsScript::new(&["pool", "pool/ROOT", "pool/ROOT/be"]);
        fake.set("pool/ROOT", "org.zfsbootmenu:commandline", "quiet");
        let get = |field: &str, ds: &str| {
            run(
                &fake,
                &["get", "-H", "-o", field, "org.zfsbootmenu:commandline", ds],
            )
            .1
        };
        assert_eq!(get("value", "pool/ROOT/be"), "quiet\n");
        assert_eq!(get("source", "pool/ROOT/be"), "inherited from pool/ROOT\n");
        assert_eq!(get("source", "pool/ROOT"), "local\n");
        assert_eq!(get("value", "pool"), "-\n");
        assert_eq!(
            run(
                &fake,
                &[
                    "get",
                    "-H",
                    "-o",
                    "source",
                    "-s",
                    "inherited",
                    "org.zfsbootmenu:commandline",
                    "pool/ROOT"
                ]
            )
            .1,
            ""
        );
        assert_eq!(run(&fake, &["list", "-o", "name", "-H", "nope"]).0, 1);
        assert_eq!(
            run(&fake, &["list", "-H", "-o", "name", "pool"]).1,
            "pool\n"
        );
    }

    #[test]
    fn set_and_inherit_change_local_values() {
        let fake = ZfsScript::new(&["pool", "pool/be"]);
        run(
            &fake,
            &["set", "org.zfsbootmenu:commandline=a b=c", "pool/be"],
        );
        assert_eq!(
            fake.local("pool/be", "org.zfsbootmenu:commandline")
                .as_deref(),
            Some("a b=c")
        );
        run(
            &fake,
            &["inherit", "org.zfsbootmenu:commandline", "pool/be"],
        );
        assert_eq!(fake.local("pool/be", "org.zfsbootmenu:commandline"), None);
        assert!(
            fake.calls()
                .contains("inherit org.zfsbootmenu:commandline pool/be")
        );
    }
}
