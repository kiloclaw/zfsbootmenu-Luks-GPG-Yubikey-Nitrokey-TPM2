//! The state directory of the runtime, `/zfsbootmenu`.
//!
//! The processes of the runtime (the start-up program, the menu, the programs
//! `fzf` starts, the recovery commands) share state only through files in this
//! directory, with the layout and formats of the legacy runtime, which users,
//! hooks and the documentation rely on. [`State`] owns that layout: no other
//! code builds these paths. See "Shared state" in `crates/DESIGN.md` for which
//! process writes and reads each file.
//!
//! Files that other processes may read while they change are replaced
//! atomically ([`Files::replace`]); the others are written as the legacy code
//! writes them.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use zbm_platform::error::{Error, Result};
use zbm_platform::files::{FileLock, Files, LockWait};

/// The lock of the menu: one menu runs at a time.
mod console;

const ACTIVE_LOCK: &str = "active.lock";

/// The lock of the menu, held by the menu that runs; when it is dropped, the
/// PID in `active` goes first, then the lock.
pub struct Active<'a> {
    files: &'a dyn Files,
    pid_file: PathBuf,
    _lock: FileLock,
}

impl Drop for Active<'_> {
    fn drop(&mut self) {
        let _ = self.files.remove_file(&self.pid_file);
    }
}

/// The state directory of the boot image (`BASE`).
pub const BASE: &str = "/zfsbootmenu";

/// The file of the state directory that the console on the display saves
/// its scrollback in, for `zbm report`.
pub const SCROLLBACK: &str = "console-scrollback.txt";

/// The most screenshots the state directory keeps.
pub const MAX_SCREENSHOTS: u32 = 99;

/// Whether `name`, a file of the state directory, is a screenshot.
pub fn is_screenshot(name: &str) -> bool {
    name.strip_prefix("screenshot-")
        .and_then(|rest| {
            rest.strip_suffix(".txt")
                .or_else(|| rest.strip_suffix(".png"))
        })
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// The first line of `text`, without the spaces and tabs around it.
fn first_line(text: &str) -> &str {
    let line = text.split('\n').next().unwrap_or_default();
    line.trim_matches([' ', '\t'])
}

/// The state directory.
pub struct State<'a> {
    files: &'a dyn Files,
    base: PathBuf,
}

impl<'a> State<'a> {
    /// The state directory at `base`, normally [`BASE`].
    pub fn new(files: &'a dyn Files, base: impl Into<PathBuf>) -> State<'a> {
        State {
            files,
            base: base.into(),
        }
    }

    /// The directory itself.
    pub fn base(&self) -> &Path {
        &self.base
    }

    fn path(&self, name: &str) -> PathBuf {
        self.base.join(name)
    }

    fn write(&self, name: &str, contents: &[u8]) -> Result<()> {
        let path = self.path(name);
        self.files
            .write(&path, contents)
            .map_err(|error| Error::io(format!("unable to write {}", path.display()), error))
    }

    fn replace(&self, name: &str, contents: &[u8]) -> Result<()> {
        let path = self.path(name);
        self.files
            .replace(&path, contents)
            .map_err(|error| Error::io(format!("unable to write {}", path.display()), error))
    }

    fn append(&self, name: &str, contents: &[u8]) -> Result<()> {
        let path = self.path(name);
        self.files
            .append(&path, contents)
            .map_err(|error| Error::io(format!("unable to write {}", path.display()), error))
    }

    /// `rm -f`: a file that does not exist is not an error.
    fn remove(&self, name: &str) {
        let _ = self.files.remove_file(&self.path(name));
    }

    fn read_line(&self, name: &str) -> Option<String> {
        let text = self.files.read_to_string(&self.path(name)).ok()?;
        Some(first_line(&text).to_owned())
    }

    /// Creates the directory (`mkdir -p "${BASE}"`).
    ///
    /// # Errors
    ///
    /// When the directory cannot be created.
    pub fn create(&self) -> Result<()> {
        self.files
            .create_dir_all(&self.base)
            .map_err(|error| Error::io(format!("unable to create {}", self.base.display()), error))
    }

    /// Whether the start-up sequence has finished (`initialized`).
    pub fn is_initialized(&self) -> bool {
        self.files.exists(&self.path("initialized"))
    }

    /// Records that the start-up sequence has finished.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn mark_initialized(&self) -> Result<()> {
        self.write("initialized", b"")
    }

    /// The start-up stages that have finished (`.stages`), in order.
    pub fn finished_stages(&self) -> Vec<String> {
        self.files
            .read_to_string(&self.path(".stages"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Records that the start-up stage `stage` has finished.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn finish_stage(&self, stage: &str) -> Result<()> {
        self.append(".stages", format!("{stage}\n").as_bytes())
    }

    /// Whether a menu holds the lock of the menu (`active.lock`). The kernel
    /// lets go of it when that menu's process ends, however it ends.
    pub fn is_active(&self) -> bool {
        self.files.is_locked(&self.path(ACTIVE_LOCK))
    }

    /// The process ID of the menu that holds the lock, while one does: it is
    /// written in `active` once the lock is taken, and removed before it is
    /// released, so a PID read without the lock held is never trusted.
    pub fn active_pid(&self) -> Option<u32> {
        if !self.is_active() {
            return None;
        }
        self.read_line("active")?.parse().ok()
    }

    /// Takes the lock of the menu for the process `pid`, unless another
    /// menu holds it (`None`); held until the result is dropped.
    ///
    /// # Errors
    ///
    /// When the lock or `active` cannot be written.
    pub fn take_active(&self, pid: u32) -> Result<Option<Active<'a>>> {
        let path = self.path(ACTIVE_LOCK);
        let lock = self
            .files
            .lock(&path, LockWait::Try)
            .map_err(|error| Error::io(format!("unable to lock {}", path.display()), error))?;
        let Some(lock) = lock else {
            return Ok(None);
        };
        let held = Active {
            files: self.files,
            pid_file: self.path("active"),
            _lock: lock,
        };
        self.write("active", format!("{pid}\n").as_bytes())?;
        Ok(Some(held))
    }

    /// Takes the lock of the record of the LUKS mappings
    /// (`luks-mappings.lock`), waiting for the session (the console or SSH)
    /// that holds it: a holder that dies lets go of it, so the wait ends.
    ///
    /// # Errors
    ///
    /// When the lock cannot be taken.
    pub fn lock_mappings(&self) -> Result<FileLock> {
        let path = self.path("luks-mappings.lock");
        // A wait that returns without the lock is a lock that is held.
        let held = || std::io::Error::from(std::io::ErrorKind::WouldBlock);
        self.files
            .lock(&path, LockWait::Block)
            .and_then(|lock| lock.ok_or_else(held))
            .map_err(|error| Error::io(format!("unable to lock {}", path.display()), error))
    }

    /// Takes the lock of the cache of key files from the key source
    /// `dataset`, unless it is held (`None`): by another process caching the
    /// same key, or by this one, when the key source's own key comes from
    /// it.
    ///
    /// # Errors
    ///
    /// When the lock cannot be taken.
    pub fn try_lock_key_cache(&self, dataset: &str) -> Result<Option<FileLock>> {
        self.lock_path(&self.key_cache(dataset).join(".lock"), LockWait::Try)
    }

    /// The lock of the file `path`.
    fn lock_path(&self, path: &Path, wait: LockWait) -> Result<Option<FileLock>> {
        self.files
            .lock(path, wait)
            .map_err(|error| Error::io(format!("unable to lock {}", path.display()), error))
    }

    /// The default boot environment (`bootfs`), if one is recorded.
    pub fn bootfs(&self) -> Option<String> {
        self.read_line("bootfs").filter(|bootfs| !bootfs.is_empty())
    }

    /// Records the default boot environment.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_bootfs(&self, dataset: &str) -> Result<()> {
        self.replace("bootfs", format!("{dataset}\n").as_bytes())
    }

    /// The pools restricted from read-write imports (`degraded`), in the order
    /// they were recorded.
    pub fn degraded_pools(&self) -> Vec<String> {
        let text = self
            .files
            .read_to_string(&self.path("degraded"))
            .unwrap_or_default();
        text.lines().map(str::to_owned).collect()
    }

    /// Whether `pool` is restricted: a line of `degraded` is exactly its
    /// name.
    pub fn is_degraded(&self, pool: &str) -> bool {
        !pool.is_empty() && self.degraded_pools().iter().any(|line| line == pool)
    }

    /// Restricts `pool` unless it is already restricted, and returns whether it
    /// was added.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn add_degraded(&self, pool: &str) -> Result<bool> {
        if self.is_degraded(pool) {
            return Ok(false);
        }
        self.append("degraded", format!("{pool}\n").as_bytes())?;
        Ok(true)
    }

    /// Restricts `pool` even if it is already restricted, as the start-up does
    /// for unhealthy pools; a second line changes nothing for
    /// [`State::is_degraded`].
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn restrict(&self, pool: &str) -> Result<()> {
        self.append("degraded", format!("{pool}\n").as_bytes())
    }

    /// The host ID found by matching pools (`spl_hostid`), if any.
    pub fn spl_hostid(&self) -> Option<String> {
        self.read_line("spl_hostid")
            .filter(|hostid| !hostid.is_empty())
    }

    /// Records the host ID found by matching pools (`echo -n`).
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_spl_hostid(&self, hostid: &str) -> Result<()> {
        self.write("spl_hostid", hostid.as_bytes())
    }

    /// When the user chose not to resume from hibernation (`noresume`), if they
    /// did; command line caches older than this are stale.
    pub fn noresume(&self) -> Option<SystemTime> {
        let metadata = self.files.metadata(&self.path("noresume")).ok()?;
        Some(metadata.modified)
    }

    /// Records that the user chose not to resume from hibernation.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_noresume(&self) -> Result<()> {
        self.write("noresume", b"")
    }

    /// The command line that replaces those of all boot environments
    /// (`cmdline`), tokenized one argument per line, if one is set.
    pub fn cmdline_override(&self) -> Option<String> {
        self.files.read_to_string(&self.path("cmdline")).ok()
    }

    /// Sets the command line that replaces those of all boot environments.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_cmdline_override(&self, tokenized: &str) -> Result<()> {
        self.write("cmdline", tokenized.as_bytes())
    }

    /// Removes the command line override (`CTRL+T` on the main screen).
    pub fn clear_cmdline_override(&self) {
        self.remove("cmdline");
    }

    /// The file `fzf` reads the boot environments from (`bootenvs`).
    pub fn boot_environments(&self) -> PathBuf {
        self.path("bootenvs")
    }

    /// The directory of a boot environment's state,
    /// `environments/<dataset>`, created if needed.
    ///
    /// # Errors
    ///
    /// When the directory cannot be created.
    pub fn environment(&self, dataset: &str) -> Result<PathBuf> {
        let path = self.path("environments").join(dataset);
        self.files
            .create_dir_all(&path)
            .map_err(|error| Error::io(format!("unable to create {}", path.display()), error))?;
        Ok(path)
    }

    /// Where a boot environment is mounted (`environments/<dataset>/mnt`).
    pub fn mount_point(&self, dataset: &str) -> PathBuf {
        self.path("environments").join(dataset).join("mnt")
    }

    /// The kernel list of a boot environment (`environments/<dataset>/kernels`).
    pub fn kernels(&self, dataset: &str) -> PathBuf {
        self.path("environments").join(dataset).join("kernels")
    }

    /// The cached command line of a kernel of a boot environment, named after
    /// the kernel: the kernel's path without its leading `/`,
    /// with `:` doubled and `/` replaced by `:`, in
    /// `environments/<dataset>/kcl/<name>.kcl`. `None` without a kernel.
    pub fn kcl_cache(&self, dataset: &str, kernel: &str) -> Option<PathBuf> {
        if kernel.is_empty() {
            return None;
        }
        let flat = kernel
            .strip_prefix('/')
            .unwrap_or(kernel)
            .replace(':', "::")
            .replace('/', ":");
        Some(
            self.path("environments")
                .join(dataset)
                .join("kcl")
                .join(format!("{flat}.kcl")),
        )
    }

    /// The cache of key files from the key source `dataset`
    /// (`.keys/<dataset>`).
    pub fn key_cache(&self, dataset: &str) -> PathBuf {
        self.key_caches().join(dataset)
    }

    /// The caches of key files of every key source (`.keys`).
    pub fn key_caches(&self) -> PathBuf {
        self.path(".keys")
    }

    /// Where an external hook root is mounted (`.external_hooks`).
    pub fn external_hooks(&self) -> PathBuf {
        self.path(".external_hooks")
    }

    /// Whether errors were logged since the last time they were shown
    /// (`have_errors`).
    pub fn has_errors(&self) -> bool {
        self.files.exists(&self.path("have_errors"))
    }

    /// Whether warnings were logged since the last time they were shown
    /// (`have_warnings`).
    pub fn has_warnings(&self) -> bool {
        self.files.exists(&self.path("have_warnings"))
    }

    /// Forgets that errors were logged, after showing them.
    pub fn clear_errors(&self) {
        self.remove("have_errors");
    }

    /// Forgets that warnings were logged, after showing them.
    pub fn clear_warnings(&self) {
        self.remove("have_warnings");
    }

    /// The time stamp of the last error report in the recovery shell
    /// (`errors_since`).
    pub fn errors_since(&self) -> PathBuf {
        self.path("errors_since")
    }

    /// The LUKS mappings ZFSBootMenu opened (`luks-mappings`), documented in
    /// `zluks(8)`.
    pub fn luks_mappings(&self) -> PathBuf {
        self.path("luks-mappings")
    }

    /// How each LUKS mapping of this boot was opened, closed since or not
    /// (`luks-unlocked`): a
    /// line `name<TAB>unattended` for a volume a silent key method opened,
    /// a TPM2 token without a PIN or Clevis, and `name<TAB>attended` for
    /// one a person opened, with a passphrase, a PIN, a security key or a
    /// key device ([`State::record_unlocked`]).
    pub fn luks_unlocked(&self) -> PathBuf {
        self.path("luks-unlocked")
    }

    /// Records that this boot opened the LUKS mapping `name`, by a person
    /// when `attended`.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn record_unlocked(&self, name: &str, attended: bool) -> Result<()> {
        let how = if attended { "attended" } else { "unattended" };
        let path = self.luks_unlocked();
        self.files
            .append(&path, format!("{name}\t{how}\n").as_bytes())
            .map_err(|error| Error::io(format!("unable to write {}", path.display()), error))
    }

    /// The LUKS mappings this boot opened when no person opened any of them
    /// (ROADMAP.md, SE10): every volume opened by a silent key method, so
    /// the machine unlocked itself. `None` when a person opened one, or none
    /// was opened.
    pub fn unattended_mappings(&self) -> Option<Vec<String>> {
        let text = self.files.read(&self.luks_unlocked()).ok()?;
        let text = String::from_utf8_lossy(&text);
        let mut names = Vec::new();
        for line in text.lines().filter(|line| !line.is_empty()) {
            match line.rsplit_once('\t') {
                Some((name, "unattended")) if !name.is_empty() => names.push(name.to_owned()),
                _ => return None,
            }
        }
        (!names.is_empty()).then_some(names)
    }

    /// The LUKS volumes the user skipped (`luks-skipped`), documented in
    /// `zluks(8)`.
    pub fn luks_skipped(&self) -> PathBuf {
        self.path("luks-skipped")
    }

    /// The lock directory held while a mapping is added to `luks-mappings`
    /// (`luks-mappings.lock`), so that the console and an SSH session cannot
    /// record the same mapping twice.
    pub fn luks_mappings_lock(&self) -> PathBuf {
        self.path("luks-mappings.lock")
    }

    /// The marker that stops the key source `dataset` from being cached again
    /// by process `pid` while it is being cached
    /// (`.keys/<dataset>/.cachemutex.<pid>`).
    pub fn key_cache_mutex(&self, dataset: &str, pid: u32) -> PathBuf {
        self.key_cache(dataset).join(format!(".cachemutex.{pid}"))
    }

    /// The marker dracut's `initqueue/settled` hook creates and its
    /// `initqueue/finished` hook waits for (`ready`), so that dracut runs its
    /// `pre-mount` hooks, which start ZFSBootMenu.
    pub fn ready(&self) -> PathBuf {
        self.path("ready")
    }
}

#[cfg(test)]
mod tests;
