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

use zbm_core::error::{Error, Result};
use zbm_system::files::Files;

/// The state directory of the boot image (`BASE`).
pub const BASE: &str = "/zfsbootmenu";

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

    /// Whether `pool` is restricted: a line of `degraded` is exactly its name,
    /// as `is_degraded` in `zfsbootmenu-core.sh` checks.
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

    /// The directory of a boot environment's state, created if needed, as
    /// `be_location` creates it.
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

    /// The cached command line of a kernel of a boot environment, as
    /// `kernel_kcl_cache` names it: the kernel's path without its leading `/`,
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

    /// The value of ZFSBootMenu's command line option `key`, as `get_zbm_arg`
    /// reads it from `cmdline.d/<key>`: the first line without the spaces and
    /// tabs around it, `1` for an option without a value, and `None` when the
    /// command line lacks the option.
    pub fn zbm_arg(&self, key: &str) -> Option<String> {
        if key.is_empty() || key.contains('/') {
            return None;
        }
        self.read_line(&format!("cmdline.d/{key}"))
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
    /// by process `pid` while it is being cached, as `cache_key` names it
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
mod tests {
    use super::*;
    use std::io;
    use zbm_testutil::fakes::{FakeFiles, FileOperation};
    use zbm_testutil::legacy::{Bash, LOG_STUBS};
    use zbm_testutil::{Rng, TempDir};

    fn path(text: &str) -> PathBuf {
        PathBuf::from(text)
    }

    #[test]
    fn markers_and_locks() {
        let files = FakeFiles::new();
        let state = State::new(&files, BASE);
        state.create().unwrap();
        assert!(!state.is_initialized());
        state.mark_initialized().unwrap();
        assert!(state.is_initialized());

        assert!(!state.is_active());
        assert_eq!(state.active_pid(), None);
        state.set_active(1234).unwrap();
        assert_eq!(files.text("/zfsbootmenu/active").unwrap(), "1234\n");
        assert_eq!(state.active_pid(), Some(1234));
        files.file("/zfsbootmenu/active", "  77  \nextra\n");
        assert_eq!(state.active_pid(), Some(77));
        files.file("/zfsbootmenu/active", "garbage\n");
        assert!(state.is_active());
        assert_eq!(state.active_pid(), None);
        state.clear_active();
        state.clear_active();
        assert!(!state.is_active());

        files.file("/zfsbootmenu/have_errors", "");
        assert!(state.has_errors() && !state.has_warnings());
        state.clear_errors();
        assert!(!state.has_errors());
        files.file("/zfsbootmenu/have_warnings", "");
        assert!(state.has_warnings());
        state.clear_warnings();
        assert!(!state.has_warnings());
    }

    #[test]
    fn bootfs_and_host_id() {
        let files = FakeFiles::new();
        files.dir(BASE);
        let state = State::new(&files, BASE);
        assert_eq!(state.bootfs(), None);
        state.set_bootfs("tank/ROOT/void").unwrap();
        assert_eq!(
            files.text("/zfsbootmenu/bootfs").unwrap(),
            "tank/ROOT/void\n"
        );
        assert_eq!(state.bootfs().as_deref(), Some("tank/ROOT/void"));
        // Replaced atomically through a temporary file.
        assert!(matches!(
            files.operations().last(),
            Some(FileOperation::Rename(_, to)) if *to == path("/zfsbootmenu/bootfs")
        ));
        files.file("/zfsbootmenu/bootfs", "\n");
        assert_eq!(state.bootfs(), None);

        assert_eq!(state.spl_hostid(), None);
        state.set_spl_hostid("00bab10c").unwrap();
        assert_eq!(files.text("/zfsbootmenu/spl_hostid").unwrap(), "00bab10c");
        assert_eq!(state.spl_hostid().as_deref(), Some("00bab10c"));
    }

    #[test]
    fn finished_stages_are_listed_in_order() {
        let files = FakeFiles::new();
        files.dir(BASE);
        let state = State::new(&files, BASE);
        assert!(state.finished_stages().is_empty());
        state.finish_stage("10-kmods").unwrap();
        state.finish_stage("20-hostid").unwrap();
        assert_eq!(state.finished_stages(), ["10-kmods", "20-hostid"]);
        assert_eq!(
            files.text("/zfsbootmenu/.stages").as_deref(),
            Some("10-kmods\n20-hostid\n")
        );
    }

    #[test]
    fn degraded_pools_match_whole_names() {
        let files = FakeFiles::new();
        files.dir(BASE);
        let state = State::new(&files, BASE);
        assert!(!state.is_degraded("tank"));
        assert!(state.add_degraded("data2").unwrap());
        assert!(state.add_degraded("a.b").unwrap());
        assert!(!state.add_degraded("data2").unwrap());
        assert_eq!(state.degraded_pools(), ["data2", "a.b"]);
        assert!(state.is_degraded("data2") && state.is_degraded("a.b"));
        // Not a substring or a pattern: the legacy bug fixed with this port.
        assert!(!state.is_degraded("data") && !state.is_degraded("ata2"));
        assert!(!state.is_degraded("axb") && !state.is_degraded(""));
        // The start-up restricts unhealthy pools without checking.
        state.restrict("data2").unwrap();
        assert_eq!(state.degraded_pools(), ["data2", "a.b", "data2"]);
    }

    #[test]
    fn resume_and_command_line_override() {
        let files = FakeFiles::new();
        files.dir(BASE);
        let state = State::new(&files, BASE);
        assert_eq!(state.noresume(), None);
        state.set_noresume().unwrap();
        let first = state.noresume().unwrap();
        state.set_noresume().unwrap();
        assert!(state.noresume().unwrap() > first);

        assert_eq!(state.cmdline_override(), None);
        state.set_cmdline_override("quiet\nro\n").unwrap();
        assert_eq!(state.cmdline_override().as_deref(), Some("quiet\nro\n"));
        state.clear_cmdline_override();
        assert_eq!(state.cmdline_override(), None);
    }

    #[test]
    fn command_line_options_are_read_as_get_zbm_arg_reads_them() {
        let files = FakeFiles::new();
        let state = State::new(&files, BASE);
        files.file("/zfsbootmenu/cmdline.d/zbm.luks", "\tUUID=a, b \nmore\n");
        files.file("/zfsbootmenu/cmdline.d/zbm.luks_key", "");
        assert_eq!(state.zbm_arg("zbm.luks").as_deref(), Some("UUID=a, b"));
        // An empty file is a value, as `read` leaves it empty and succeeds.
        assert_eq!(state.zbm_arg("zbm.luks_key").as_deref(), Some(""));
        assert_eq!(state.zbm_arg("zbm.luks_tokens"), None);
        assert_eq!(state.zbm_arg(""), None);
        assert_eq!(state.zbm_arg("../cmdline.d/zbm.luks"), None);
    }

    #[test]
    fn layout() {
        let files = FakeFiles::new();
        let state = State::new(&files, BASE);
        assert_eq!(state.base(), Path::new("/zfsbootmenu"));
        assert_eq!(state.boot_environments(), path("/zfsbootmenu/bootenvs"));
        assert_eq!(
            state.mount_point("tank/ROOT/void"),
            path("/zfsbootmenu/environments/tank/ROOT/void/mnt")
        );
        assert_eq!(
            state.kernels("tank/ROOT/void"),
            path("/zfsbootmenu/environments/tank/ROOT/void/kernels")
        );
        assert_eq!(
            state.kcl_cache("tank/ROOT/void", "/boot/vmlinuz-6.1:x"),
            Some(path(
                "/zfsbootmenu/environments/tank/ROOT/void/kcl/boot:vmlinuz-6.1::x.kcl"
            ))
        );
        assert_eq!(state.kcl_cache("tank/ROOT/void", ""), None);
        assert_eq!(
            state.key_cache("tank/keys"),
            path("/zfsbootmenu/.keys/tank/keys")
        );
        assert_eq!(state.external_hooks(), path("/zfsbootmenu/.external_hooks"));
        assert_eq!(state.errors_since(), path("/zfsbootmenu/errors_since"));
        assert_eq!(state.luks_mappings(), path("/zfsbootmenu/luks-mappings"));
        assert_eq!(state.luks_skipped(), path("/zfsbootmenu/luks-skipped"));
        assert_eq!(
            state.luks_mappings_lock(),
            path("/zfsbootmenu/luks-mappings.lock")
        );
        assert_eq!(
            state.key_cache_mutex("tank/keys", 42),
            path("/zfsbootmenu/.keys/tank/keys/.cachemutex.42")
        );
        assert_eq!(state.ready(), path("/zfsbootmenu/ready"));

        assert!(!files.exists(&path("/zfsbootmenu/environments/tank")));
        let environment = state.environment("tank/ROOT/void").unwrap();
        assert!(files.is_dir(&environment));
    }

    #[test]
    fn write_failures_name_the_file() {
        let files = FakeFiles::new();
        files.dir(BASE);
        files.fail("/zfsbootmenu/initialized", io::ErrorKind::StorageFull);
        let state = State::new(&files, BASE);
        assert_eq!(
            state.mark_initialized().unwrap_err().to_string(),
            "unable to write /zfsbootmenu/initialized: No space left on device"
        );
        files.fail("/zfsbootmenu/environments", io::ErrorKind::PermissionDenied);
        assert!(state.environment("tank/ROOT/void").is_err());
        let missing = State::new(&files, "/missing/base");
        assert!(missing.set_bootfs("x").is_err());
        assert!(missing.add_degraded("x").is_err());
        files.fail("/missing", io::ErrorKind::PermissionDenied);
        assert!(missing.create().is_err());
    }

    /// The legacy library, loadable outside the image: `zfsbootmenu-core.sh`
    /// sources `/lib/zfsbootmenu-kcl.sh`, which the shim finds in `legacy/`.
    fn legacy_core() -> Bash {
        Bash::new()
            .code(LOG_STUBS)
            .code(
                r#"source() {
                    case "$1" in
                      /lib/*) builtin source "$LEGACY/zfsbootmenu/lib/${1#/lib/}" ;;
                      *) builtin source "$@" ;;
                    esac
                  }"#,
            )
            .source("zfsbootmenu/lib/zfsbootmenu-core.sh")
    }

    #[test]
    fn kcl_cache_names_match_the_legacy_function() {
        let dir = TempDir::new("zbm-state");
        let base = dir.path().to_string_lossy().into_owned();
        let files = FakeFiles::new();
        let state = State::new(&files, base.as_str());
        let bash = legacy_core().env("BASE", &base);
        let mut rng = Rng::from_env(4);
        let alphabet = ['a', 'b', '/', ':', '.', '-', '1'];
        let mut kernels = vec![
            "/boot/vmlinuz-6.1".to_owned(),
            "boot/vmlinuz".to_owned(),
            "/a:b/c::d".to_owned(),
            "//x".to_owned(),
        ];
        for _ in 0..40 {
            kernels.push(format!("/{}", rng.string(&alphabet, 12)));
        }
        for kernel in &kernels {
            let output = bash.call("kernel_kcl_cache", &["tank/ROOT/void", kernel], b"");
            let expected = state
                .kcl_cache("tank/ROOT/void", kernel)
                .map(|path| format!("{}\n", path.display()));
            assert_eq!(
                output.status.success(),
                expected.is_some(),
                "status for {kernel:?}"
            );
            assert_eq!(Some(output.stdout_text()), expected, "kernel {kernel:?}");
        }
        let output = bash.call("kernel_kcl_cache", &["tank/ROOT/void", ""], b"");
        assert!(!output.status.success());
        assert_eq!(state.kcl_cache("tank/ROOT/void", ""), None);
    }

    #[test]
    fn degraded_checks_match_the_legacy_function() {
        let dir = TempDir::new("zbm-state");
        let base = dir.path().to_string_lossy().into_owned();
        let files = FakeFiles::new();
        let state = State::new(&files, base.as_str());
        files.dir(&base);
        let recorded = ["data2", "a.b", "rpool", "tank-1"];
        for pool in recorded {
            state.add_degraded(pool).unwrap();
        }
        dir.write(
            "degraded",
            files.contents(state.base().join("degraded")).unwrap(),
        );
        let bash = legacy_core().env("BASE", &base);
        let mut pools = vec![
            "data", "data2", "ata2", "a.b", "axb", "rpool", "pool", "tank", "",
        ];
        let mut rng = Rng::from_env(5);
        let generated: Vec<String> = (0..30)
            .map(|_| rng.string(&['a', 'b', '.', '2', 'd', 't'], 6))
            .collect();
        pools.extend(generated.iter().map(String::as_str));
        for pool in pools {
            let legacy = bash.call("is_degraded", &[pool], b"").status.success();
            assert_eq!(state.is_degraded(pool), legacy, "pool {pool:?}");
        }
    }
}
