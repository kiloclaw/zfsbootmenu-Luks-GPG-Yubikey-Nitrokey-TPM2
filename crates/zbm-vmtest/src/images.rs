//! Images built by the build container: the ZFS test boot environment and
//! ZFSBootMenu images from this checkout.

use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::error::{Error, Result};
use crate::settings::{HOSTID, Settings, ZbmRuntime};

/// Runs the build container with `args`; the mounts are (host, guest)
/// directories, `env` variables to set.
///
/// # Errors
///
/// [`Error::Command`] when the container runtime cannot be started.
pub fn container(
    settings: &Settings,
    args: &[&str],
    mounts: &[(&Path, &str)],
    entrypoint: &str,
    env: &[(&str, &str)],
) -> Result<Output> {
    let mut command = Command::new(&settings.container);
    command.args(["run", "--rm"]);
    for (host, guest) in mounts {
        command.arg("-v").arg(format!("{}:{guest}", host.display()));
    }
    for (key, value) in env {
        command.arg("-e").arg(format!("{key}={value}"));
    }
    command
        .args(["--entrypoint", entrypoint])
        .arg(&settings.image)
        .args(args);
    command.output().map_err(|error| Error::Command {
        command: format!("{command:?}"),
        status: "not run".into(),
        output: error.to_string(),
    })
}

/// The host ID file of the test images and boot environment.
///
/// # Errors
///
/// [`Error::Io`] when it cannot be written.
pub fn hostid_file(settings: &Settings) -> Result<PathBuf> {
    let path = settings.work.join("hostid");
    fs::create_dir_all(&settings.work).map_err(Error::io("creating the work directory"))?;
    if !path.exists() {
        fs::write(&path, HOSTID).map_err(Error::io("writing the host ID file"))?;
    }
    Ok(path)
}

/// `spl_hostid=` with the host ID of the test images, for an image built
/// elsewhere: the boot environment's root pool stays imported at power-off,
/// so another host ID sees it as in use by another system.
///
/// # Errors
///
/// [`Error::Io`] when the host ID file cannot be read.
pub fn hostid_arg(settings: &Settings) -> Result<String> {
    let bytes = fs::read(hostid_file(settings)?).map_err(Error::io("reading the host ID"))?;
    let mut id = [0_u8; 4];
    for (target, byte) in id.iter_mut().zip(&bytes) {
        *target = *byte;
    }
    Ok(format!("spl_hostid=0x{:08x}", u32::from_le_bytes(id)))
}

/// Builds the ZFS test boot environment (kernel, initramfs, root file
/// system) unless it exists, or when `force`.
///
/// # Errors
///
/// [`Error::Build`] when the build fails.
pub fn build_be(settings: &Settings, force: bool) -> Result<()> {
    hostid_file(settings)?;
    if settings.work.join("payload/kver").exists() && !force {
        return Ok(());
    }
    let be = settings.guest_files().join("be");
    let output = container(
        settings,
        &["/out"],
        &[(&settings.work, "/out"), (&be, "/be:ro")],
        "/be/build-be.sh",
        &[],
    )?;
    let log = settings.work.join("build-be.log");
    fs::write(&log, [output.stdout, output.stderr].concat()).ok();
    if !output.status.success() {
        return Err(Error::Build {
            what: "the boot environment".into(),
            log,
        });
    }
    Ok(())
}

/// The kernel version of the test boot environment.
///
/// # Errors
///
/// When the boot environment cannot be built.
pub fn be_kver(settings: &Settings) -> Result<String> {
    build_be(settings, false)?;
    let kver = fs::read_to_string(settings.work.join("payload/kver")).map_err(Error::io(
        "reading the kernel version of the boot environment",
    ))?;
    Ok(kver.trim().to_owned())
}

/// What a ZFSBootMenu test image is built from.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImageSpec {
    /// The name of its build directory.
    pub variant: String,
    /// The text of `/etc/zfsbootmenu/crypttab`, if it has one.
    pub crypttab: Option<String>,
    /// Files copied into the build directory (path, contents); those below
    /// `hooks/` are executable.
    pub files: Vec<(String, Vec<u8>)>,
    /// Extra dracut configuration lines.
    pub conf: String,
    /// `Kernel.CommandLine` of the EFI image.
    pub cmdline: Option<String>,
    /// Whether to build an EFI image too.
    pub efi: bool,
    /// A shell script run in the container before the build (`rc.d`).
    pub rc: Option<String>,
    /// The runtime; `None`: the run's.
    pub runtime: Option<ZbmRuntime>,
    /// Cargo features of the build, such as `zbm/crash-test`.
    pub features: Option<String>,
    /// `Global.ConsoleFont` of the `generate-zbm` configuration.
    pub console_font: Option<String>,
}

impl ImageSpec {
    /// An image named `variant`, without anything else.
    pub fn new(variant: &str) -> ImageSpec {
        ImageSpec {
            variant: variant.to_owned(),
            ..ImageSpec::default()
        }
    }

    /// The inputs as text: an image is reused only when it was built from
    /// the same inputs. A setup test can change a file the image embeds,
    /// such as the GnuPG key file of the keys tests.
    fn stamp(&self) -> String {
        let hex = |bytes: &[u8]| {
            bytes.iter().fold(String::new(), |mut out, byte| {
                write!(out, "{byte:02x}").ok();
                out
            })
        };
        let optional =
            |text: Option<&String>| text.map_or("none".to_owned(), |t| hex(t.as_bytes()));
        let mut stamp = format!(
            "crypttab {}\nconf {}\ncmdline {}\nefi {}\nrc {}\nfeatures {}\n",
            optional(self.crypttab.as_ref()),
            hex(self.conf.as_bytes()),
            optional(self.cmdline.as_ref()),
            self.efi,
            optional(self.rc.as_ref()),
            optional(self.features.as_ref()),
        );
        if let Some(font) = &self.console_font {
            writeln!(stamp, "console-font {}", hex(font.as_bytes())).ok();
        }
        let mut files: Vec<&(String, Vec<u8>)> = self.files.iter().collect();
        files.sort();
        for (name, data) in files {
            writeln!(stamp, "file {} {}", hex(name.as_bytes()), hex(data)).ok();
        }
        stamp
    }
}

/// The files of a built image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Image {
    /// The kernel.
    pub kernel: PathBuf,
    /// The initramfs.
    pub initrd: PathBuf,
    /// The EFI image, when one was built.
    pub efi: PathBuf,
    /// The directory of `zbm`'s debug file (`cargo xtask symbolize`).
    pub debug: PathBuf,
}

/// The dracut configuration of an image.
fn dracut_conf(spec: &ImageSpec, conf: &str) -> String {
    let crypttab = if spec.crypttab.is_some() {
        "/build/crypttab"
    } else {
        "/build/no-crypttab"
    };
    format!(
        "zfsbootmenu_hook_root=\"/build/hooks\"\ncompress=\"zstd\"\n\
         zfsbootmenu_luks_crypttab=\"{crypttab}\"\n{conf}\n"
    )
}

/// The `generate-zbm` configuration of an image.
fn generate_config(spec: &ImageSpec) -> String {
    let global = spec.console_font.as_ref().map_or(String::new(), |font| {
        format!("Global:\n  ConsoleFont: {font}\n")
    });
    let mut config = format!(
        "{global}Components:\n  Enabled: true\n  Versions: false\nEFI:\n  Enabled: {}\n  \
         Versions: false\nKernel:\n  Version: 7.3.*\n",
        spec.efi
    );
    if let Some(cmdline) = &spec.cmdline {
        writeln!(config, "  CommandLine: \"{cmdline}\"").ok();
    }
    config
}

/// Builds a ZFSBootMenu test image with the build container from the
/// checkout, unless `ZBMTEST_REUSE=1` and it was built from the same
/// inputs, the checkout's sources that go into images among them. The
/// build runs the checkout's `packaging/container/build-init.sh`, so
/// changes to it need no new build container. `zbm`'s debug file is left
/// in `build/debug/<build ID>.debug`.
///
/// # Errors
///
/// [`Error::Build`] when the build fails.
pub fn zbm_image(settings: &Settings, spec: &ImageSpec) -> Result<Image> {
    let mut variant = spec.variant.clone();
    let mut conf = spec.conf.clone();
    if spec.runtime.unwrap_or(settings.runtime) == ZbmRuntime::Rust {
        variant.push_str("-rust");
        conf.push_str("\nzfsbootmenu_runtime=\"rust\"");
    } else if settings.shadow {
        conf.push_str("\nzfsbootmenu_shadow=\"yes\"");
    }
    let dir = settings.work.join("zbm").join(&variant);
    let build = dir.join("build");
    let image = Image {
        kernel: build.join("vmlinuz-bootmenu"),
        initrd: build.join("initramfs-bootmenu.img"),
        efi: build.join("vmlinuz.EFI"),
        debug: build.join("debug"),
    };
    let stamp_path = dir.join("inputs.txt");
    let stamp = inputs(settings, spec, &conf);
    let built = image.initrd.exists() && (!spec.efi || image.efi.exists());
    if settings.reuse && built && fs::read_to_string(&stamp_path).ok().as_deref() == Some(&stamp) {
        return Ok(image);
    }
    prepare_build(settings, spec, &dir, &conf)?;
    let features = spec.features.as_deref();
    let env: Vec<(&str, &str)> = features
        .map(|f| ("ZBM_CARGO_FEATURES", f))
        .into_iter()
        .collect();
    let output = container(
        settings,
        &["-V"],
        &[(&settings.repo, "/zbm:ro"), (&dir, "/build")],
        "/zbm/packaging/container/build-init.sh",
        &env,
    )?;
    let log = dir.join("build.log");
    fs::write(&log, [output.stdout, output.stderr].concat()).ok();
    if !output.status.success() || !image.initrd.exists() {
        return Err(Error::Build {
            what: format!("ZFSBootMenu image {variant}"),
            log,
        });
    }
    fs::write(&stamp_path, stamp).map_err(Error::io("writing the image's inputs"))?;
    Ok(image)
}

/// What an image with `spec` and the dracut configuration `conf` is built
/// from, as recorded beside it: its inputs and the checkout's sources.
fn inputs(settings: &Settings, spec: &ImageSpec, conf: &str) -> String {
    format!(
        "{}conf+ {conf}\nsources {}\n",
        spec.stamp(),
        source::fingerprint(&settings.repo)
    )
}

/// Fills a fresh build directory.
fn prepare_build(settings: &Settings, spec: &ImageSpec, dir: &Path, conf: &str) -> Result<()> {
    let io = |what: &str| Error::io(format!("{what} in {}", dir.display()));
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(io("removing the old build"))?;
    }
    fs::create_dir_all(dir.join("dracut.conf.d")).map_err(io("creating dracut.conf.d"))?;
    fs::copy(hostid_file(settings)?, dir.join("hostid")).map_err(io("copying the host ID"))?;
    copy_tree(&settings.guest_files().join("hooks"), &dir.join("hooks"))
        .map_err(io("copying the hooks"))?;
    if let Some(crypttab) = &spec.crypttab {
        fs::write(dir.join("crypttab"), crypttab).map_err(io("writing the crypttab"))?;
    }
    fs::write(
        dir.join("dracut.conf.d/zz-test.conf"),
        dracut_conf(spec, conf),
    )
    .map_err(io("writing the dracut configuration"))?;
    for (name, data) in &spec.files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io("creating a directory"))?;
        }
        fs::write(&path, data).map_err(io("writing a file"))?;
        if name.starts_with("hooks/") {
            executable(&path).map_err(io("making a hook executable"))?;
        }
    }
    if let Some(rc) = &spec.rc {
        let script = dir.join("rc.d/10-test");
        fs::create_dir_all(dir.join("rc.d")).map_err(io("creating rc.d"))?;
        fs::write(&script, format!("#!/bin/sh\nset -e\n{rc}\n")).map_err(io("writing rc.d"))?;
        executable(&script).map_err(io("making rc.d executable"))?;
    }
    fs::write(dir.join("config.yaml"), generate_config(spec)).map_err(io("writing config.yaml"))?;
    Ok(())
}

/// Makes `path` executable (mode 755).
fn executable(path: &Path) -> std::io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

/// Copies the directory tree `from` to `to`, with its modes.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

mod source;

#[cfg(test)]
mod tests;
