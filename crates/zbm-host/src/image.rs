//! The plan of an image (ROADMAP.md SB8, ADR 0006): what goes into a
//! ZFSBootMenu image, computed from the builder's settings and the build
//! host, as a [`Plan`] that the dracut module and the mkinitcpio hook
//! install (`zbm image plan`), for the legacy runtime and the Rust runtime.
//!
//! The sections do what the Bash install helpers (`install-helpers.sh`)
//! did before them; `builders` puts them in the order of each builder. The
//! crate sits beside `generate-zbm`, which runs the builders, until DM10
//! joins them in `zbm-host`.

mod builders;
mod config;
mod core;
mod fonts;
mod graphics;
mod host;
pub mod keymaps;
pub mod library;
mod libs;
pub mod lists;
mod luks;
mod plan;
mod prune;
pub mod psf;
mod shell;

use std::path::{Path, PathBuf};

pub use config::{Builder, Config, Runtime};
pub use host::{Found, Host};
pub use plan::{Entry, Need, Plan};
pub use prune::{Pruned, prune_firmware};

/// Why an image cannot be built: what the build says before it stops.
pub type Fatal = String;

/// An image as its plan grows.
pub struct Image<'a> {
    /// The builder.
    pub builder: Builder,
    /// The runtime.
    pub runtime: Runtime,
    /// The settings.
    pub config: &'a Config,
    /// The build host.
    pub host: &'a Host<'a>,
    /// The plan so far.
    pub plan: Plan,
    /// uutils' multi-call binary and its programs, when the host has it.
    uutils: Option<(PathBuf, Vec<String>)>,
}

impl<'a> Image<'a> {
    /// An empty image for `builder`.
    ///
    /// # Errors
    ///
    /// For a runtime that is not one.
    pub fn new(
        builder: Builder,
        config: &'a Config,
        host: &'a Host<'a>,
    ) -> Result<Image<'a>, Fatal> {
        let runtime = config.runtime()?;
        let uutils = host.which("coreutils").and_then(|path| {
            let programs = zbm_tools::host::coreutils_programs(host.runner)?;
            Some((path, programs))
        });
        Ok(Image {
            builder,
            runtime,
            config,
            host,
            plan: Plan::default(),
            uutils,
        })
    }

    /// Whether the image has the Rust runtime.
    pub fn is_rust(&self) -> bool {
        self.runtime == Runtime::Rust
    }

    /// The directory of ZFSBootMenu's files on the build host.
    pub fn module_root(&self) -> PathBuf {
        PathBuf::from(self.config.module_root())
    }

    /// Installs the program `name`, as `command -v` finds it, at its path:
    /// that path, or `None` when the host has no such program.
    pub fn executable(&mut self, name: &str) -> Option<PathBuf> {
        let path = self.host.which(name)?;
        self.plan.binary(path.clone(), path.clone());
        Some(path)
    }

    /// Installs each of the programs `names`: those the host does not have.
    pub fn executables<'n>(&mut self, names: &[&'n str]) -> Vec<&'n str> {
        names
            .iter()
            .copied()
            .filter(|name| self.executable(name).is_none())
            .collect()
    }

    /// Installs the program `name`: a hard link to uutils' multi-call
    /// binary when it provides `name` (uutils refuses to run under a name
    /// that is not the file's, so not a symbolic link; GNU's multi-call
    /// binary has no list, so it is not taken), the host's program
    /// otherwise: the file installed, or `None` when there is none.
    ///
    /// The multi-call binary is planned once, before its first link: an
    /// entry planned again moves to the end of the plan, after the links
    /// made to it, which the shims could then not make.
    pub fn program(&mut self, name: &str) -> Option<PathBuf> {
        if let Some((path, programs)) = &self.uutils
            && programs.iter().any(|program| program == name)
        {
            let path = path.clone();
            let multi_call = Path::new("/usr/bin/coreutils");
            if !self.plan.has(multi_call) {
                self.plan.binary(path.clone(), multi_call);
            }
            self.plan.add(Entry::HardLink {
                target: "/usr/bin/coreutils".into(),
                dest: Path::new("/usr/bin").join(name),
            });
            return Some(path);
        }
        self.executable(name)
    }

    /// Installs the build host's file `src` at `dest`; a relative symbolic
    /// link of the module root stays a link, with the file it points to
    /// beside it.
    pub fn install(&mut self, src: &Path, dest: &Path) {
        let Some(target) = self.relative_link(src) else {
            self.plan.file(src, dest);
            return;
        };
        if let Ok(real) = self.host.files.canonicalize(src)
            && self.host.is_file(&real)
            && !self.host.is_link(&real)
        {
            let beside = host::normalized(&dest.parent().unwrap_or(Path::new("/")).join(&target));
            self.install(&real, &beside);
        }
        self.plan.link(target, dest);
    }

    /// What `src` points to, when it is a relative link of the module root,
    /// which images keep as links.
    fn relative_link(&self, src: &Path) -> Option<PathBuf> {
        if !src.starts_with(self.module_root()) {
            return None;
        }
        let target = self.host.files.read_link(src).ok()?;
        (!target.as_os_str().is_empty() && target.is_relative()).then_some(target)
    }

    /// Installs the tree of the build host at `dir`, as `tar` copies it.
    pub fn tree(&mut self, dir: &Path) {
        for (path, found) in self.host.walk(dir) {
            match found {
                Found::Dir(mode) => self.plan.add(Entry::Dir { dest: path, mode }),
                Found::File => self.plan.file(path.clone(), path),
                Found::Link(target) => self.plan.link(target, path),
            }
        }
    }
}

/// The plan of the image that `builder` builds with `config` on `host`.
///
/// # Errors
///
/// Why the image cannot be built.
pub fn plan(builder: Builder, config: &Config, host: &Host) -> Result<Plan, Fatal> {
    let mut image = Image::new(builder, config, host)?;
    match builder {
        Builder::Dracut => builders::dracut(&mut image)?,
        Builder::Mkinitcpio => builders::mkinitcpio(&mut image)?,
    }
    Ok(image.plan)
}

#[cfg(test)]
mod tests;
