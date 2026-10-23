//! The settings of a run, from the environment, and the constants the
//! scenarios share.
//!
//! - `ZBMTEST_WORK`: the work directory for disks, images and logs
//!   (`/var/tmp/zbm-luks-test`);
//! - `ZBMTEST_IMAGE`: the build container image (`localhost/zbm-builder`);
//! - `ZBMTEST_RUNTIME`: `docker` or `podman` (`docker` when installed);
//! - `ZBMTEST_ACCEL`: `kvm` or `tcg` (`kvm` when `/dev/kvm` is usable);
//! - `ZBMTEST_REUSE`: `1` reuses images built from the same inputs;
//! - `ZBMTEST_ZBM_RUNTIME`: `legacy` (the default) or `rust`, the runtime
//!   of the images;
//! - `ZBMTEST_SHADOW`: `0` builds legacy images without shadow mode;
//! - `ZBMTEST_OLD_KERNEL`, `ZBMTEST_OLD_INITRD`: an image with an older
//!   ZFS, for the `oldzfs` tests, which are skipped without it;
//! - `ZBMTEST_REPO`: the checkout to build images from (this crate's).

use std::env;
use std::path::{Path, PathBuf};

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

/// The firmware code of UEFI boots.
pub const OVMF_CODE: &str = "/usr/share/OVMF/OVMF_CODE_4M.fd";

/// The template of the firmware variables of UEFI boots.
pub const OVMF_VARS: &str = "/usr/share/OVMF/OVMF_VARS_4M.fd";

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
    /// Whether legacy images run shadow mode.
    pub shadow: bool,
    /// The kernel and initramfs of an image with an older ZFS.
    pub old_image: Option<(PathBuf, PathBuf)>,
    /// The checkout images are built from.
    pub repo: PathBuf,
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
        Settings::from_vars(|name| env::var(name).ok(), docker, kvm)
    }

    /// The settings from the variables `var` gives, with `docker` telling
    /// whether docker is installed and `kvm` whether `/dev/kvm` is usable.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>, docker: bool, kvm: bool) -> Settings {
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
            runtime: if set("ZBMTEST_ZBM_RUNTIME").as_deref() == Some("rust") {
                ZbmRuntime::Rust
            } else {
                ZbmRuntime::Legacy
            },
            shadow: var("ZBMTEST_SHADOW").as_deref() != Some("0"),
            old_image,
            repo: set("ZBMTEST_REPO").map_or_else(default_repo, PathBuf::from),
        }
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
        let settings = Settings::from_vars(|_| None, false, true);

        assert_eq!(settings.work, PathBuf::from("/var/tmp/zbm-luks-test"));
        assert_eq!(settings.container, "podman");
        assert_eq!(settings.accel, "kvm");
        assert_eq!(settings.runtime, ZbmRuntime::Legacy);
        assert!(settings.shadow && !settings.reuse);
        assert_eq!(settings.old_image, None);
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
                "ZBMTEST_ZBM_RUNTIME" => "rust",
                "ZBMTEST_SHADOW" => "0",
                "ZBMTEST_OLD_KERNEL" => "/k",
                "ZBMTEST_OLD_INITRD" => "/i",
                "ZBMTEST_REPO" => "/r",
                _ => return None,
            };
            Some(value.to_owned())
        };

        let settings = Settings::from_vars(var, false, true);

        assert_eq!(settings.disks("keys"), PathBuf::from("/w/disks/keys"));
        assert_eq!(
            (settings.container.as_str(), settings.accel.as_str()),
            ("docker", "tcg")
        );
        assert!(settings.reuse && !settings.shadow);
        assert_eq!(settings.runtime.as_str(), "rust");
        assert_eq!(settings.old_image, Some(("/k".into(), "/i".into())));
        assert_eq!(settings.repo, PathBuf::from("/r"));
    }
}
