//! The system around a build: the boot partition, the hook directories, the
//! hostid check and the temporary work directory.

use std::path::{Path, PathBuf};

use zbm_config::spec::OptionSpec;
use zbm_config::spec::generate::GLOBAL_BOOT_MOUNT_POINT;
use zbm_platform::filetest::{is_dir, is_executable, is_file, is_writable, strerror};
use zbm_platform::shell::quote;

use super::{Generator, Step, Stop};
use crate::glob::glob;
use crate::perl;

/// A temporary directory removed when the program ends, as `File::Temp->newdir`
/// arranges.
pub(super) struct WorkDir(pub(super) PathBuf);

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Generator<'_, '_, '_> {
    /// Mounts `Global.BootMountPoint` unless it is mounted already.
    pub(super) fn mount_boot(&mut self) -> Step<()> {
        let Some(mount_point) = self
            .setting(&GLOBAL_BOOT_MOUNT_POINT)?
            .filter(|text| !text.is_empty())
        else {
            return Ok(());
        };
        let (_, status) = self.execute(&format!("mountpoint {}", quote(&mount_point)))?;
        if status != 0 {
            self.print(&format!("Mounting {mount_point}\n"));
            let (output, status) = self.execute(&format!("mount {}", quote(&mount_point)))?;
            if status == 0 {
                self.mounted = Some(mount_point);
            } else {
                self.print_lines(&output);
                self.print(&format!("Unable to mount {mount_point}\n"));
                return Err(Stop::Exit(status));
            }
        }
        Ok(())
    }

    /// Runs the executables in `Global.PreHooksDir` or `Global.PostHooksDir`, in
    /// glob order.
    pub(super) fn run_hooks(&mut self, key: &OptionSpec) -> Step<()> {
        let Some(dir) = self.setting(key)?.filter(|text| !text.is_empty()) else {
            return Ok(());
        };
        if !is_dir(&dir) {
            return Ok(());
        }
        for hook in glob(&format!("{dir}/*")) {
            if !is_executable(&hook) {
                continue;
            }
            self.log(&format!("Processing hook: {hook}"));
            // The Perl program logs the whole result of `execute`, whose last element is
            // the exit status.
            let (mut output, status) = self.execute(&quote(&hook))?;
            output.push(status.to_string().into_bytes());
            self.log_lines(&output);
        }
        Ok(())
    }

    /// Warns when the SPL module's hostid and `/etc/hostid` differ.
    pub(super) fn check_hostid(&mut self) {
        let module = if is_file(&self.context.spl_hostid) {
            let text = std::fs::read(&self.context.spl_hostid).unwrap_or_default();
            let first = text.split_inclusive(|byte| *byte == b'\n').next();
            first.map_or_else(
                || "00000000".to_owned(),
                |line| perl::hex_of_numeric_text(&String::from_utf8_lossy(line)),
            )
        } else {
            "00000000".to_owned()
        };
        if module == "00000000" || !is_file(&self.context.etc_hostid) {
            return;
        }
        let bytes = std::fs::read(&self.context.etc_hostid).unwrap_or_default();
        let system = match bytes.get(..4) {
            Some(word) => format!(
                "{:08x}",
                u32::from_le_bytes([word[0], word[1], word[2], word[3]])
            ),
            None => "00000000".to_owned(),
        };
        if module != system {
            self.print(&format!(
                "SPL ({module}) and system ({system}) hostids do not match!\n"
            ));
        }
    }

    /// File::Spec's `tmpdir`: `$TMPDIR` when it is a writable directory, otherwise
    /// `/tmp`, otherwise `.`.
    fn temp_base(&self) -> PathBuf {
        let candidates = [self.context.tmpdir.clone(), Some(PathBuf::from("/tmp"))];
        let base = candidates
            .into_iter()
            .flatten()
            .find(|dir| is_dir(dir) && is_writable(dir))
            .unwrap_or_else(|| PathBuf::from("."));
        let base = canonical_path(&base);
        if base.is_absolute() {
            base
        } else {
            std::env::current_dir().map_or(base.clone(), |cwd| cwd.join(&base))
        }
    }

    /// Creates the work directory in the temporary directory.
    pub(super) fn work_dir(&mut self) -> Step<WorkDir> {
        let base = self.temp_base();
        zbm_platform::filetest::create_temp_dir(&base, "XXXXXXXXXX")
            .map(WorkDir)
            .map_err(|error| {
                Stop::fatal(
                    format!(
                        "Error in tempdir() using {}/XXXXXXXXXX: Could not create directory: {}",
                        base.display(),
                        strerror(&error)
                    ),
                    &error,
                )
            })
    }
}

/// File::Spec's `canonpath` on Unix: no repeated `/`, no `/.` components, no
/// leading `./` and no trailing `/`.
pub(super) fn canonical_path(path: &Path) -> PathBuf {
    let text = path.display().to_string();
    let mut parts: Vec<&str> = Vec::new();
    for (index, part) in text.split('/').enumerate() {
        if part.is_empty() && index > 0 || part == "." && (index > 0 || text.len() > 1) {
            continue;
        }
        parts.push(part);
    }
    let joined = parts.join("/");
    if joined.is_empty() {
        PathBuf::from(if text.starts_with('/') { "/" } else { "." })
    } else {
        PathBuf::from(joined)
    }
}
