//! Kernel modules (ROADMAP.md CD4): loaded by the runtime itself, with the
//! kernel's index of its modules and `modprobe.d` (`zbm_platform::modules`)
//! and `finit_module`, which decompresses them too; `modprobe`, `modinfo`
//! and `insmod` only where that cannot be: an image without the index, a
//! module the configuration loads with a command (`install`), or a kernel
//! that cannot decompress the module's file (before Linux 5.17).

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use zbm_platform::modules::index::{Index, KernelFiles, Plan, Step};
use zbm_platform::modules::is_compressed;
use zbm_platform::{Command, Files, Input, ModuleLoader, Output, Runner, Stream, System};

/// The longest loading a module may take: a driver waits for its device,
/// and up to a minute for each firmware file it asks for.
const LOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// Where `modprobe` reads its configuration, the first a file's name is
/// found in winning.
const CONFIG_DIRS: [&str; 5] = [
    "/etc/modprobe.d",
    "/run/modprobe.d",
    "/usr/local/lib/modprobe.d",
    "/usr/lib/modprobe.d",
    "/lib/modprobe.d",
];

/// `EINVAL`, `ENOSYS` and `EOPNOTSUPP`: what a kernel answers for a
/// compressed module it cannot decompress, or a call it has not.
const CANNOT: [i32; 3] = [22, 38, 95];

/// Why a module did not load: what the kernel or `modprobe` said.
pub type Failure = String;

/// What the runtime did itself.
enum Native {
    /// Loaded, or it was already.
    Done,
    /// It could not: `modprobe` is asked.
    Fallback,
}

/// Loading kernel modules.
#[derive(Clone, Copy)]
pub struct Modules<'a> {
    runner: &'a dyn Runner,
    files: &'a dyn Files,
    loader: &'a dyn ModuleLoader,
}

impl<'a> Modules<'a> {
    /// The modules, through `files` and `loader`, and `runner` for the
    /// programs where they cannot.
    pub fn new(
        runner: &'a dyn Runner,
        files: &'a dyn Files,
        loader: &'a dyn ModuleLoader,
    ) -> Modules<'a> {
        Modules {
            runner,
            files,
            loader,
        }
    }

    /// The modules of `system`.
    pub fn of(system: &System<'a>) -> Modules<'a> {
        Modules::new(system.runner, system.files, system.modules)
    }

    /// The index of the running kernel's modules, and the configuration.
    fn index(&self) -> Index {
        let read = |path: &Path| self.files.read_to_string(path).unwrap_or_default();
        let release = read(Path::new("/proc/sys/kernel/osrelease"));
        let root = Path::new("/lib/modules").join(release.trim());
        let texts = [
            "modules.dep",
            "modules.alias",
            "modules.softdep",
            "modules.builtin",
        ]
        .map(|name| read(&root.join(name)));
        let kernel = KernelFiles {
            dep: &texts[0],
            alias: &texts[1],
            softdep: &texts[2],
            builtin: &texts[3],
        };
        let config: Vec<String> = self.config_files().iter().map(|path| read(path)).collect();
        let config: Vec<&str> = config.iter().map(String::as_str).collect();
        Index::parse(&root, &kernel, &config, &read(Path::new("/proc/cmdline")))
    }

    /// The files of `modprobe.d`, in the order `modprobe` reads them: by
    /// name, a name found in an earlier directory hiding the later ones.
    fn config_files(&self) -> Vec<PathBuf> {
        let mut found: std::collections::BTreeMap<String, PathBuf> =
            std::collections::BTreeMap::new();
        for dir in CONFIG_DIRS {
            for name in self.files.read_dir(Path::new(dir)).unwrap_or_default() {
                let Some(name) = name.to_str() else { continue };
                let is_conf = Path::new(name)
                    .extension()
                    .is_some_and(|extension| extension == "conf");
                if is_conf && !found.contains_key(name) {
                    found.insert(name.to_owned(), Path::new(dir).join(name));
                }
            }
        }
        found.into_values().collect()
    }

    /// Whether `name` is loaded, or built in.
    fn is_loaded(&self, name: &str) -> bool {
        self.files.exists(&Path::new("/sys/module").join(name))
    }

    /// Inserts `steps` in order, but what is loaded already.
    fn insert(&self, steps: &[Step]) -> Result<Native, Failure> {
        for step in steps {
            if self.is_loaded(&step.name) {
                continue;
            }
            match self.loader.insert(&step.path, &step.parameters) {
                Ok(()) => zbm_trace::debug!("loaded {name}"; name = %step.name),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) if is_beyond_kernel(&error, &step.path) => {
                    zbm_trace::debug!(
                        "{name}: {error}; asking modprobe";
                        name = %step.name, %error
                    );
                    return Ok(Native::Fallback);
                }
                Err(error) if step.optional => {
                    zbm_trace::debug!("{name}: {error}"; name = %step.name, %error);
                }
                Err(error) => return Err(format!("{}: {error}", step.name)),
            }
        }
        Ok(Native::Done)
    }

    /// Loads `module` itself; whether it must ask `modprobe`.
    fn load_native(&self, module: &str) -> Result<Native, Failure> {
        let index = self.index();
        if index.is_empty() {
            return Ok(Native::Fallback);
        }
        match index.plan(module, &[]) {
            Plan::Insert(steps) => self.insert(&steps),
            Plan::Command => Ok(Native::Fallback),
            Plan::Unknown => Err(format!("module {module} not found")),
        }
    }

    /// Loads `module` if the kernel has it, quietly; whether it loaded is
    /// for the caller to find out.
    pub fn probe_quietly(&self, module: &str) {
        if matches!(self.load_native(module), Ok(Native::Done)) {
            return;
        }
        let command = Command::new("modprobe")
            .args(["-q", module])
            .timeout(LOAD_TIMEOUT)
            .stdin(Input::Null)
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        let _ = self.runner.run(&command);
    }

    /// Loads `module` and what it needs.
    ///
    /// # Errors
    ///
    /// Why it did not load.
    pub fn load(&self, module: &str) -> Result<(), Failure> {
        match self.load_native(module) {
            Ok(Native::Done) => Ok(()),
            Err(failure) => Err(failure),
            Ok(Native::Fallback) => outcome(
                self.runner.run(
                    &Command::new("modprobe")
                        .arg(module)
                        .timeout(LOAD_TIMEOUT)
                        .merge_stderr(),
                ),
            ),
        }
    }

    /// Inserts the file of `module` alone with `parameters`, none of the
    /// configuration's, as `insmod` does.
    ///
    /// # Errors
    ///
    /// Why it did not load, or that it has no file.
    pub fn insert_alone(&self, module: &str, parameters: &[&str]) -> Result<(), Failure> {
        let index = self.index();
        let file = match index.file(module) {
            Some(file) => file,
            None if index.is_empty() => return self.insmod(module, parameters),
            None => return Err(format!("module {module} not found")),
        };
        match self.loader.insert(&file, &parameters.join(" ")) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) if is_beyond_kernel(&error, &file) => self.insmod(module, parameters),
            Err(error) => Err(format!("{module}: {error}")),
        }
    }

    /// `insmod $(modinfo -F filename MODULE) PARAMETERS`.
    fn insmod(&self, module: &str, parameters: &[&str]) -> Result<(), Failure> {
        let found = self
            .runner
            .run(&Command::new("modinfo").args(["-F", "filename", module]))
            .map(|output| output.stdout_text().trim_end_matches('\n').to_owned())
            .unwrap_or_default();
        if found.is_empty() {
            return Err(format!("module {module} not found"));
        }
        let command = Command::new("insmod")
            .arg(found)
            .timeout(LOAD_TIMEOUT)
            .args(parameters.iter().copied())
            .merge_stderr();
        outcome(self.runner.run(&command))
    }
}

/// Whether `error`, for the module file `path`, says the kernel cannot
/// load it itself: no `finit_module` here, or a compressed file it cannot
/// decompress.
fn is_beyond_kernel(error: &io::Error, path: &Path) -> bool {
    error.kind() == io::ErrorKind::Unsupported
        || error
            .raw_os_error()
            .is_some_and(|errno| CANNOT.contains(&errno) && (errno != 22 || is_compressed(path)))
}

/// The outcome of a program run: nothing when it succeeded, else what it
/// said.
fn outcome(run: io::Result<Output>) -> Result<(), Failure> {
    match run {
        Ok(output) if output.status.is_success() => Ok(()),
        Ok(output) => Err(output.stdout_text().trim_end_matches('\n').to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests;
