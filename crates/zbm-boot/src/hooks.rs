//! Finding hooks.
//!
//! `zfsbootmenu-run-hooks` runs `"/libexec/hooks/${stage}"/*` in the order the
//! glob gives (by name, in the C locale; names starting with a dot are left
//! out), skipping every entry for which `[ -x ]` is false. For the root user
//! who runs it, `[ -x ]` is true for a file with any execute bit and for every
//! directory.

use std::path::{Path, PathBuf};

use zbm_system::files::{FileKind, Files};

/// Where the hook directories are in the image.
pub const HOOKS: &str = "/libexec/hooks";

/// The hook stages, in the order a boot runs them.
pub const STAGES: [&str; 5] = [
    "early-setup.d",
    "setup.d",
    "load-key.d",
    "boot-sel.d",
    "teardown.d",
];

/// The hooks of `stage` below `root` that would run, in order. A missing
/// directory has none.
pub fn executable_hooks(files: &dyn Files, root: &Path, stage: &str) -> Vec<PathBuf> {
    let dir = root.join(stage);
    let Ok(names) = files.read_dir(&dir) else {
        return Vec::new();
    };
    names
        .into_iter()
        .filter(|name| !name.as_encoded_bytes().starts_with(b"."))
        .map(|name| dir.join(name))
        .filter(|path| {
            files.metadata(path).is_ok_and(|metadata| {
                metadata.kind == FileKind::Directory || metadata.is_executable()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testutil::fakes::FakeFiles;

    #[test]
    fn lists_what_the_legacy_glob_and_test_run() {
        let files = FakeFiles::new();
        let root = Path::new(HOOKS);
        files.file_with_mode("/libexec/hooks/early-setup.d/20-b.sh", "", 0o755);
        files.file_with_mode("/libexec/hooks/early-setup.d/10-a.sh", "", 0o700);
        files.file_with_mode("/libexec/hooks/early-setup.d/30-off.sh", "", 0o644);
        files.file_with_mode("/libexec/hooks/early-setup.d/40-done.sh.completed", "", 0);
        files.file_with_mode("/libexec/hooks/early-setup.d/.hidden", "", 0o755);
        files.file_with_mode("/libexec/hooks/early-setup.d/Z-upper", "", 0o001);
        files.dir("/libexec/hooks/early-setup.d/50-dir");
        files.symlink("/libexec/hooks/early-setup.d/60-link", "20-b.sh");
        files.symlink("/libexec/hooks/early-setup.d/70-dangling", "missing");
        let names: Vec<String> = executable_hooks(&files, root, "early-setup.d")
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            ["10-a.sh", "20-b.sh", "50-dir", "60-link", "Z-upper"]
        );
        assert!(executable_hooks(&files, root, "load-key.d").is_empty());
        assert_eq!(STAGES.len(), 5);
    }

    /// Whether the tests run as root, as the legacy code always does; `[ -x ]`
    /// means something else for other users.
    fn running_as_root() -> bool {
        std::fs::read_to_string("/proc/self/status").is_ok_and(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("Uid:"))
                .and_then(|ids| ids.split_whitespace().nth(1))
                == Some("0")
        })
    }

    #[test]
    fn matches_the_legacy_glob_on_real_files() {
        use std::os::unix::fs::PermissionsExt;
        use zbm_system::files::RealFiles;
        use zbm_testutil::TempDir;
        use zbm_testutil::legacy::Bash;

        if !running_as_root() {
            eprintln!("skipped: the legacy code runs as root");
            return;
        }
        let dir = TempDir::new("zbm-hooks");
        let stage = dir.join("teardown.d");
        std::fs::create_dir(&stage).unwrap();
        for (name, mode) in [
            ("90-xhci", 0o755),
            ("80-luks", 0o700),
            ("85-off", 0o644),
            ("10 space", 0o100),
            (".hidden", 0o755),
            ("x.completed", 0o000),
            ("B", 0o010),
        ] {
            let path = dir.write(format!("teardown.d/{name}"), "");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        std::fs::create_dir(stage.join("sub")).unwrap();
        std::os::unix::fs::symlink("missing", stage.join("dangling")).unwrap();

        // The loop of zfsbootmenu-run-hooks
        let output = Bash::new()
            .code(
                r#"list_hooks() {
                  for _hook in "$1"/*; do [ -x "${_hook}" ] || continue; echo "${_hook##*/}"; done
                }"#,
            )
            .call("list_hooks", &[&stage.to_string_lossy()], b"");
        let legacy: Vec<String> = output.stdout_text().lines().map(str::to_owned).collect();
        let rust: Vec<String> = executable_hooks(&RealFiles, dir.path(), "teardown.d")
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(rust, legacy);
        assert_eq!(rust, ["10 space", "80-luks", "90-xhci", "B", "sub"]);
    }
}
