//! The settings of a run, from the environment, and the constants the
//! scenarios share.
//!
//! - `ZBMTEST_WORK`: the work directory for disks, images and logs
//!   (`/var/tmp/zbm-luks-test`);
//! - `ZBMTEST_IMAGE`: the build container image (`localhost/zbm-builder`);
//! - `ZBMTEST_RUNTIME`: `docker` or `podman` (`docker` when installed);
//! - `ZBMTEST_ACCEL`: `kvm` or `tcg` (`kvm` when `/dev/kvm` is usable);
//! - `ZBMTEST_REUSE`: `1` reuses images built from the same inputs, the
//!   checkout's sources among them;
//! - `ZBMTEST_ZBM_RUNTIME`: `rust` (the default) or `legacy`, the runtime
//!   of the images;
//! - `ZBMTEST_OLD_KERNEL`, `ZBMTEST_OLD_INITRD`: an image with an older
//!   ZFS, for the `oldzfs` tests, which are skipped without it;
//! - `ZBMTEST_REPO`: the checkout to build images from (this crate's);
//! - `ZBMTEST_OVMF_CODE`, `ZBMTEST_OVMF_VARS`: the UEFI firmware code and
//!   the template of its variables, set together (the first pair of
//!   [`OVMF_PAIRS`] whose two files exist).

use std::env;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// The LUKS passphrase of most test volumes.
pub const PW: &str = "pw";

/// A second passphrase, for one volume.
pub const PW_OTHER: &str = "other";

/// The passphrase of ZFS native encryption.
pub const ZFSKEY: &str = "zfskey12";

/// The host ID of the test images and boot environment, `00bab10c`, as
/// `/etc/hostid` holds it.
pub const HOSTID: [u8; 4] = [0x0c, 0xb1, 0xba, 0x00];

/// Fast PBKDF settings for test volumes: emulation is slow, and the boot
/// environment unlocks volumes in parallel. Procedure tests use the
/// defaults.
pub const FAST_PBKDF: &str = "--pbkdf argon2id --pbkdf-memory 65536 --iter-time 500";

/// The UEFI firmware the harness looks for, in order, as pairs of the
/// firmware code and the template of its variables: Debian and Ubuntu,
/// Fedora, Arch.
pub const OVMF_PAIRS: [(&str, &str); 3] = [
    (
        "/usr/share/OVMF/OVMF_CODE_4M.fd",
        "/usr/share/OVMF/OVMF_VARS_4M.fd",
    ),
    (
        "/usr/share/edk2/ovmf/OVMF_CODE.fd",
        "/usr/share/edk2/ovmf/OVMF_VARS.fd",
    ),
    (
        "/usr/share/edk2/x64/OVMF_CODE.4m.fd",
        "/usr/share/edk2/x64/OVMF_VARS.4m.fd",
    ),
];

/// The version of ZFS the images and the boot environment run.
pub const ZFS_VERSION: &str = "2.4.4";

/// The runtime of the ZFSBootMenu images.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZbmRuntime {
    /// The Bash runtime.
    Legacy,
    /// The Rust runtime (`zfsbootmenu_runtime="rust"`).
    Rust,
}

impl ZbmRuntime {
    /// Its name, as `ZBMTEST_ZBM_RUNTIME` gives it.
    pub fn as_str(self) -> &'static str {
        match self {
            ZbmRuntime::Legacy => "legacy",
            ZbmRuntime::Rust => "rust",
        }
    }
}

/// UEFI firmware: its code and its variables, from one package, as the
/// variables of one build only fit its own code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ovmf {
    /// The firmware code, which a boot only reads.
    pub code: PathBuf,
    /// The firmware variables: in [`Settings`], the template; in a
    /// machine, its own copy.
    pub vars: PathBuf,
}

impl Ovmf {
    /// The firmware that `ZBMTEST_OVMF_CODE` and `ZBMTEST_OVMF_VARS` name
    /// when `set` gives either, otherwise the first of [`OVMF_PAIRS`] whose
    /// two files `exists` finds.
    ///
    /// # Errors
    ///
    /// The reason a boot that needs UEFI cannot start: one variable set
    /// without the other, or no pair found, with the paths tried.
    pub fn find(
        set: impl Fn(&str) -> Option<String>,
        exists: impl Fn(&Path) -> bool,
    ) -> std::result::Result<Ovmf, String> {
        let variables = "ZBMTEST_OVMF_CODE and ZBMTEST_OVMF_VARS";
        match (set("ZBMTEST_OVMF_CODE"), set("ZBMTEST_OVMF_VARS")) {
            (Some(code), Some(vars)) => {
                return Ok(Ovmf {
                    code: code.into(),
                    vars: vars.into(),
                });
            }
            (Some(_), None) | (None, Some(_)) => {
                return Err(format!(
                    "{variables} must be set together; only one of them is set"
                ));
            }
            (None, None) => {}
        }
        let found = OVMF_PAIRS
            .iter()
            .find(|(code, vars)| exists(Path::new(code)) && exists(Path::new(vars)));
        if let Some((code, vars)) = found {
            return Ok(Ovmf {
                code: code.into(),
                vars: vars.into(),
            });
        }
        let tried: Vec<String> = OVMF_PAIRS
            .iter()
            .map(|(code, vars)| format!("{code} and {vars}"))
            .collect();
        Err(format!(
            "no UEFI firmware found (tried {}); install OVMF or set {variables}",
            tried.join(", ")
        ))
    }
}

/// The settings of a run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settings {
    /// The work directory.
    pub work: PathBuf,
    /// The build container image.
    pub image: String,
    /// `docker` or `podman`.
    pub container: String,
    /// `kvm` or `tcg`.
    pub accel: String,
    /// Whether images built from the same inputs are reused.
    pub reuse: bool,
    /// The runtime of the images, unless a scenario chooses.
    pub runtime: ZbmRuntime,
    /// The kernel and initramfs of an image with an older ZFS.
    pub old_image: Option<(PathBuf, PathBuf)>,
    /// The checkout images are built from.
    pub repo: PathBuf,
    /// The UEFI firmware, or why none was found; see [`Settings::ovmf`].
    pub firmware: std::result::Result<Ovmf, String>,
}

impl Settings {
    /// The settings from the environment.
    pub fn from_env() -> Settings {
        let docker = program_exists("docker");
        let kvm = Path::new("/dev/kvm").exists()
            && std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/kvm")
                .is_ok();
        Settings::from_vars(|name| env::var(name).ok(), docker, kvm, Path::exists)
    }

    /// The settings from the variables `var` gives, with `docker` telling
    /// whether docker is installed, `kvm` whether `/dev/kvm` is usable and
    /// `exists` whether a file of the UEFI firmware exists.
    pub fn from_vars(
        var: impl Fn(&str) -> Option<String>,
        docker: bool,
        kvm: bool,
        exists: impl Fn(&Path) -> bool,
    ) -> Settings {
        let set = |name: &str| var(name).filter(|value| !value.is_empty());
        let default_container = if docker { "docker" } else { "podman" };
        let default_accel = if kvm { "kvm" } else { "tcg" };
        let old_image = set("ZBMTEST_OLD_KERNEL")
            .zip(set("ZBMTEST_OLD_INITRD"))
            .map(|(kernel, initrd)| (PathBuf::from(kernel), PathBuf::from(initrd)));
        Settings {
            work: PathBuf::from(set("ZBMTEST_WORK").unwrap_or("/var/tmp/zbm-luks-test".into())),
            image: set("ZBMTEST_IMAGE").unwrap_or("localhost/zbm-builder".into()),
            container: set("ZBMTEST_RUNTIME").unwrap_or(default_container.into()),
            accel: set("ZBMTEST_ACCEL").unwrap_or(default_accel.into()),
            reuse: var("ZBMTEST_REUSE").as_deref() == Some("1"),
            runtime: if set("ZBMTEST_ZBM_RUNTIME").as_deref() == Some("legacy") {
                ZbmRuntime::Legacy
            } else {
                ZbmRuntime::Rust
            },
            old_image,
            repo: set("ZBMTEST_REPO").map_or_else(default_repo, PathBuf::from),
            firmware: Ovmf::find(set, exists),
        }
    }

    /// The UEFI firmware of the boots that need it.
    ///
    /// # Errors
    ///
    /// [`Error::Setup`] when none was found or its variables are set
    /// wrongly; runs that boot no UEFI machine never ask.
    pub fn ovmf(&self) -> Result<&Ovmf> {
        self.firmware
            .as_ref()
            .map_err(|reason| Error::Setup(reason.clone()))
    }

    /// The directory of the files the tests put into images and boot
    /// environments: `be/`, `hooks/`, `functions.sh`.
    pub fn guest_files(&self) -> PathBuf {
        self.repo.join("tests/luks/vm")
    }

    /// The directory of a disk group's images.
    pub fn disks(&self, group: &str) -> PathBuf {
        self.work.join("disks").join(group)
    }

    /// The kernel and initramfs of the image with an older ZFS, when both
    /// variables are set.
    ///
    /// # Errors
    ///
    /// [`Error::Setup`] when either is not a file, such as a placeholder
    /// copied as it was: QEMU would stop at once, before the test sees why.
    pub fn old_image_files(&self) -> Result<Option<(&Path, &Path)>> {
        let Some((kernel, initrd)) = &self.old_image else {
            return Ok(None);
        };
        for (name, path) in [
            ("ZBMTEST_OLD_KERNEL", kernel),
            ("ZBMTEST_OLD_INITRD", initrd),
        ] {
            if !path.is_file() {
                return Err(Error::Setup(format!(
                    "{name}={} is not a file",
                    path.display()
                )));
            }
        }
        Ok(Some((kernel, initrd)))
    }
}

/// The checkout this crate was built from.
fn default_repo() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = crate_dir.join("../..");
    repo.canonicalize().unwrap_or(repo)
}

/// Whether `program` is in a directory of `PATH`.
pub fn program_exists(program: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|path| env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn defaults_follow_the_host() {
        let settings = Settings::from_vars(|_| None, false, true, |_| false);

        assert_eq!(settings.work, PathBuf::from("/var/tmp/zbm-luks-test"));
        assert_eq!(settings.container, "podman");
        assert_eq!(settings.accel, "kvm");
        assert_eq!(settings.runtime, ZbmRuntime::Rust);
        assert!(!settings.reuse);
        assert_eq!(settings.old_image, None);
        assert_eq!(settings.old_image_files().unwrap(), None);
        assert!(settings.guest_files().ends_with("tests/luks/vm"));
    }

    #[test]
    fn variables_override_the_defaults() {
        let var = |name: &str| {
            let value = match name {
                "ZBMTEST_WORK" => "/w",
                "ZBMTEST_RUNTIME" => "docker",
                "ZBMTEST_ACCEL" => "tcg",
                "ZBMTEST_REUSE" => "1",
                "ZBMTEST_ZBM_RUNTIME" => "legacy",
                "ZBMTEST_OLD_KERNEL" => "/k",
                "ZBMTEST_OLD_INITRD" => "/i",
                "ZBMTEST_REPO" => "/r",
                _ => return None,
            };
            Some(value.to_owned())
        };

        let settings = Settings::from_vars(var, false, true, |_| true);

        assert_eq!(settings.disks("keys"), PathBuf::from("/w/disks/keys"));
        assert_eq!(
            (settings.container.as_str(), settings.accel.as_str()),
            ("docker", "tcg")
        );
        assert!(settings.reuse);
        assert_eq!(settings.runtime.as_str(), "legacy");
        assert_eq!(settings.old_image, Some(("/k".into(), "/i".into())));
        let error = settings.old_image_files().unwrap_err().to_string();
        assert_eq!(error, "ZBMTEST_OLD_KERNEL=/k is not a file");
        assert_eq!(settings.repo, PathBuf::from("/r"));
        let file = PathBuf::from(env!("CARGO_MANIFEST_PATH"));
        let found = Settings {
            old_image: Some((file.clone(), file.clone())),
            ..settings
        };
        assert_eq!(
            found.old_image_files().unwrap(),
            Some((file.as_path(), file.as_path()))
        );
    }

    /// The firmware found with the variables `vars` and the files `files`.
    fn ovmf_with(vars: &[(&str, &str)], files: &[&str]) -> Result<Ovmf> {
        let var = |name: &str| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        };
        let exists = |path: &Path| files.iter().any(|file| Path::new(file) == path);
        Settings::from_vars(var, false, true, exists)
            .ovmf()
            .cloned()
    }

    fn pair(index: usize) -> Ovmf {
        let (code, vars) = OVMF_PAIRS[index];
        Ovmf {
            code: code.into(),
            vars: vars.into(),
        }
    }

    #[test]
    fn the_variables_name_the_firmware_whatever_is_installed() {
        let vars = [
            ("ZBMTEST_OVMF_CODE", "/fw/CODE.fd"),
            ("ZBMTEST_OVMF_VARS", "/fw/VARS.fd"),
        ];

        let found = ovmf_with(&vars, &[OVMF_PAIRS[0].0, OVMF_PAIRS[0].1]).unwrap();

        assert_eq!(found.code, PathBuf::from("/fw/CODE.fd"));
        assert_eq!(found.vars, PathBuf::from("/fw/VARS.fd"));
    }

    #[test]
    fn one_firmware_variable_alone_is_an_error_naming_both() {
        for name in ["ZBMTEST_OVMF_CODE", "ZBMTEST_OVMF_VARS"] {
            let files = [OVMF_PAIRS[0].0, OVMF_PAIRS[0].1];

            let error = ovmf_with(&[(name, "/fw/one.fd")], &files).unwrap_err();

            assert_eq!(
                error.to_string(),
                "ZBMTEST_OVMF_CODE and ZBMTEST_OVMF_VARS must be set together; \
                 only one of them is set"
            );
        }
    }

    #[test]
    fn each_distribution_s_firmware_is_found() {
        for (index, (code, vars)) in OVMF_PAIRS.iter().enumerate() {
            assert_eq!(ovmf_with(&[], &[code, vars]).unwrap(), pair(index));
        }
    }

    #[test]
    fn debian_s_firmware_is_preferred_and_pairs_are_never_mixed() {
        let all: Vec<&str> = OVMF_PAIRS
            .iter()
            .flat_map(|(code, vars)| [*code, *vars])
            .collect();
        let mixed = [
            OVMF_PAIRS[0].0,
            OVMF_PAIRS[1].1,
            OVMF_PAIRS[2].0,
            OVMF_PAIRS[2].1,
        ];

        assert_eq!(ovmf_with(&[], &all).unwrap(), pair(0));
        assert_eq!(ovmf_with(&[], &mixed).unwrap(), pair(2));
    }

    #[test]
    fn without_firmware_a_uefi_boot_fails_naming_the_paths_and_variables() {
        let settings = Settings::from_vars(|_| None, false, true, |_| false);

        let error = settings.ovmf().unwrap_err().to_string();

        assert_eq!(
            error,
            "no UEFI firmware found (tried /usr/share/OVMF/OVMF_CODE_4M.fd and \
             /usr/share/OVMF/OVMF_VARS_4M.fd, /usr/share/edk2/ovmf/OVMF_CODE.fd and \
             /usr/share/edk2/ovmf/OVMF_VARS.fd, /usr/share/edk2/x64/OVMF_CODE.4m.fd and \
             /usr/share/edk2/x64/OVMF_VARS.4m.fd); install OVMF or set \
             ZBMTEST_OVMF_CODE and ZBMTEST_OVMF_VARS"
        );
    }
}
