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
use zbm_platform::files::Files;

/// The state directory of the boot image (`BASE`).
pub const BASE: &str = "/zfsbootmenu";

/// The file of the state directory that the console on the display saves
/// its scrollback in, for `zbm report`.
pub const SCROLLBACK: &str = "console-scrollback.txt";

/// The value `read -r` gives a variable from the first line of `text`: the
/// line without the spaces and tabs around it.
fn read_first_line(text: &str) -> &str {
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
        Some(read_first_line(&text).to_owned())
    }

    /// Creates the directory (`mkdir -p "${BASE}"`).
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
    pub fn finish_stage(&self, stage: &str) -> Result<()> {
        self.append(".stages", format!("{stage}\n").as_bytes())
    }

    /// Whether a menu holds the lock (`active`).
    pub fn is_active(&self) -> bool {
        self.files.exists(&self.path("active"))
    }

    /// The process ID of the menu that holds the lock, if it can be read.
    pub fn active_pid(&self) -> Option<u32> {
        self.read_line("active")?.parse().ok()
    }

    /// Takes the lock for the menu with process ID `pid` (`echo "$$" > active`).
    pub fn set_active(&self, pid: u32) -> Result<()> {
        self.write("active", format!("{pid}\n").as_bytes())
    }

    /// Releases the lock (`rm -f active`).
    pub fn clear_active(&self) {
        self.remove("active");
    }

    /// The process ID of the console on the display (`zbm console`,
    /// ROADMAP.md #19), while it holds the display: the console writes it
    /// once it can take requests, and removes it once it handed the
    /// display back.
    pub fn console_pid(&self) -> Option<u32> {
        self.read_line("console")?.parse().ok()
    }

    /// Records the console with process ID `pid`.
    pub fn set_console(&self, pid: u32) -> Result<()> {
        self.write("console", format!("{pid}\n").as_bytes())
    }

    /// Records that no console holds the display, nor reads the keyboards.
    pub fn clear_console(&self) {
        self.remove("console");
        self.remove("console-layout");
        self.remove("console-font");
    }

    /// The process ID of the console that handed the display back before
    /// `kexec` and relays on the kernel console: when `kexec` fails, it is
    /// asked to take the display again (`zbm console reattach`).
    pub fn released_console_pid(&self) -> Option<u32> {
        self.read_line("console-released")?.parse().ok()
    }

    /// Records the console with process ID `pid` as one that handed the
    /// display back.
    pub fn set_released_console(&self, pid: u32) -> Result<()> {
        self.write("console-released", format!("{pid}\n").as_bytes())
    }

    /// Records that no console waits to take the display again.
    pub fn clear_released_console(&self) {
        self.remove("console-released");
    }

    /// The keyboard layout the console on the display types with.
    pub fn console_layout(&self) -> Option<String> {
        self.read_line("console-layout")
            .filter(|name| !name.is_empty())
    }

    /// Records the keyboard layout the console types with.
    pub fn set_console_layout(&self, name: &str) -> Result<()> {
        self.write("console-layout", format!("{name}\n").as_bytes())
    }

    /// Asks the console for the keyboard layout `name`
    /// (`zbm console layout`); the console takes the request.
    pub fn request_console_layout(&self, name: &str) -> Result<()> {
        self.write("console-layout-request", format!("{name}\n").as_bytes())
    }

    /// Whether a request for a keyboard layout waits for the console.
    pub fn has_console_layout_request(&self) -> bool {
        self.files.exists(&self.path("console-layout-request"))
    }

    /// The keyboard layout asked for, while the request waits.
    pub fn console_layout_request(&self) -> Option<String> {
        self.read_line("console-layout-request")
            .filter(|name| !name.is_empty())
    }

    /// Removes the request for a keyboard layout: the console took it, or
    /// it came too late.
    pub fn clear_console_layout_request(&self) {
        self.remove("console-layout-request");
    }

    /// The font the console on the display draws with: its name, its
    /// cells' size and how many.
    pub fn console_font(&self) -> Option<String> {
        self.read_line("console-font")
            .filter(|font| !font.is_empty())
    }

    /// Records the font the console draws with.
    pub fn set_console_font(&self, font: &str) -> Result<()> {
        self.write("console-font", format!("{font}\n").as_bytes())
    }

    /// Asks the console for the font's size `size` (`zbm console font`);
    /// the console takes the request.
    pub fn request_console_font(&self, size: &str) -> Result<()> {
        self.write("console-font-request", format!("{size}\n").as_bytes())
    }

    /// Whether a request for a font's size waits for the console.
    pub fn has_console_font_request(&self) -> bool {
        self.files.exists(&self.path("console-font-request"))
    }

    /// The font's size asked for, while the request waits.
    pub fn console_font_request(&self) -> Option<String> {
        self.read_line("console-font-request")
            .filter(|size| !size.is_empty())
    }

    /// Removes the request for a font's size: the console took it, or it
    /// came too late.
    pub fn clear_console_font_request(&self) {
        self.remove("console-font-request");
    }

    /// The process ID of the program that shows a secret prompt now (a
    /// passphrase, a PIN): the console on the display keeps no scrollback
    /// and shows nothing typed while it is there.
    pub fn secret_prompt_pid(&self) -> Option<u32> {
        self.read_line("secret-prompt")?.parse().ok()
    }

    /// Records that process `pid` shows a secret prompt.
    pub fn set_secret_prompt(&self, pid: u32) -> Result<()> {
        self.write("secret-prompt", format!("{pid}\n").as_bytes())
    }

    /// Records that no secret prompt is shown.
    pub fn clear_secret_prompt(&self) {
        self.remove("secret-prompt");
    }

    /// Saves `text`, the console's scrollback, in [`SCROLLBACK`]; returns
    /// its path.
    pub fn save_scrollback(&self, text: &[u8]) -> Result<PathBuf> {
        self.replace(SCROLLBACK, text)?;
        Ok(self.path(SCROLLBACK))
    }

    /// The default boot environment (`bootfs`), if one is recorded.
    pub fn bootfs(&self) -> Option<String> {
        self.read_line("bootfs").filter(|bootfs| !bootfs.is_empty())
    }

    /// Records the default boot environment.
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
    pub fn restrict(&self, pool: &str) -> Result<()> {
        self.append("degraded", format!("{pool}\n").as_bytes())
    }

    /// The host ID found by matching pools (`spl_hostid`), if any.
    pub fn spl_hostid(&self) -> Option<String> {
        self.read_line("spl_hostid")
            .filter(|hostid| !hostid.is_empty())
    }

    /// Records the host ID found by matching pools (`echo -n`).
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
    pub fn set_noresume(&self) -> Result<()> {
        self.write("noresume", b"")
    }

    /// The command line that replaces those of all boot environments
    /// (`cmdline`), tokenized one argument per line, if one is set.
    pub fn cmdline_override(&self) -> Option<String> {
        self.files.read_to_string(&self.path("cmdline")).ok()
    }

    /// Sets the command line that replaces those of all boot environments.
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
        self.path(".keys").join(dataset)
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
