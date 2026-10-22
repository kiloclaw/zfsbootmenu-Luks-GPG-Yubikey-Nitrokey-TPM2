//! Writing to the kernel log.
//!
//! Code logs with the macros of `zbm_trace` (`zbm_trace::info!` and the
//! others); [`KernelLog`] decides where those messages go. Installed for a
//! thread, it writes them as `kmsg-log-lib.sh` does: to `/dev/kmsg`, filtered
//! by `loglevel`, with the marker files `have_warnings` and `have_errors` in
//! the state directory. It also writes every `tracing` event, whatever its
//! level, to the journal `journal.jsonl` in the state directory. It writes
//! through [`Files`], so tests see every message in their fake files.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing_subscriber::layer::SubscriberExt as _;
use zbm_trace::{JournalLayer, KmsgLayer, KmsgSink};

use crate::files::Files;

/// The kernel log device.
pub const KMSG: &str = "/dev/kmsg";

/// The journal's name in the state directory.
pub const JOURNAL: &str = "journal.jsonl";

/// The most a process writes to the journal, so that a menu left open for
/// days cannot fill the memory file system.
pub const JOURNAL_LIMIT: u64 = 4 << 20;

/// Appends what is written to a file, through [`Files`]: each write is one
/// append, so the lines of several processes do not mix.
pub struct FilesAppender {
    /// The files.
    files: Arc<dyn Files + Send + Sync>,
    /// The file appended to.
    path: PathBuf,
}

impl FilesAppender {
    /// Appends to `path` through `files`.
    pub fn new(files: &dyn Files, path: &Path) -> FilesAppender {
        FilesAppender {
            files: files.shared(),
            path: path.to_owned(),
        }
    }
}

impl io::Write for FilesAppender {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.files.append(&self.path, bytes)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Where the kernel log of a [`KernelLog`] goes: a device and a state
/// directory, through [`Files`].
pub struct FilesSink {
    /// The files.
    files: Arc<dyn Files + Send + Sync>,
    /// The kernel log device.
    device: PathBuf,
    /// The state directory, which holds the marker files.
    base: PathBuf,
}

impl KmsgSink for FilesSink {
    fn record(&self, line: &str) {
        let _ = self.files.append(&self.device, line.as_bytes());
    }

    fn mark(&self, marker: &str) {
        // `: > "${BASE}/have_errors"`
        let _ = self.files.write(&self.base.join(marker), b"");
    }
}

/// The kernel log of a process: where `zbm_trace`'s messages go once it is
/// installed.
///
/// ```
/// use std::path::Path;
/// use zbm_system::{KernelLog, RealFiles};
///
/// let id = std::process::id();
/// let dir = std::env::temp_dir().join(format!("zbm-doc-{id}"));
/// std::fs::create_dir_all(&dir).unwrap();
/// let device = dir.join("kmsg");
/// let log = KernelLog::new(&RealFiles, "6", &dir, 1).with_device(&device);
/// {
///     let _installed = log.install();
///     zbm_trace::info!("importing {}", "tank");
/// }
/// let text = std::fs::read_to_string(dir.join("kmsg")).unwrap();
/// assert_eq!(text, "<6>ZFSBootMenu: importing tank\n");
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub struct KernelLog {
    /// The files, the device and the state directory.
    sink: FilesSink,
    /// The log level, as `zbm_trace::kmsg::parse_level` reads it.
    loglevel: String,
    /// The process ID that debugging output names.
    pid: u32,
    /// Whether messages are written as `LEVEL: text` lines.
    labelled: bool,
    /// The journal, if any.
    journal: Option<PathBuf>,
}

impl KernelLog {
    /// A log with the given `loglevel` that writes to `/dev/kmsg` through
    /// `files`, leaves marker files and the journal in `base`, and names
    /// process `pid` in debugging output.
    pub fn new(files: &dyn Files, loglevel: &str, base: &Path, pid: u32) -> KernelLog {
        KernelLog {
            journal: Some(base.join(JOURNAL)),
            sink: FilesSink {
                files: files.shared(),
                device: PathBuf::from(KMSG),
                base: base.to_owned(),
            },
            loglevel: loglevel.to_owned(),
            pid,
            labelled: false,
        }
    }

    /// The same log, writing each message of any level as one `LEVEL: text`
    /// line, as the logging stand-ins of the LUKS unit suites do; empty
    /// debugging output is left out.
    pub fn labelled(mut self) -> KernelLog {
        self.labelled = true;
        self
    }

    /// The same log, writing to another device.
    pub fn with_device(mut self, device: &Path) -> KernelLog {
        self.sink.device = device.to_owned();
        self
    }

    /// The same log, without a journal.
    pub fn without_journal(mut self) -> KernelLog {
        self.journal = None;
        self
    }

    /// Sends this thread's messages to the log until the returned guard is
    /// dropped; then the log installed before, if any, takes them again.
    pub fn install(self) -> Installed {
        let journal = self.journal.map(|path| {
            JournalLayer::new(FilesAppender::new(&*self.sink.files, &path))
                .with_limit(JOURNAL_LIMIT)
        });
        let mut layer = KmsgLayer::new(self.sink, &self.loglevel, self.pid);
        if self.labelled {
            layer = layer.labelled();
        }
        let subscriber = tracing_subscriber::registry()
            .with(layer)
            .with(journal)
            .with(zbm_trace::crash::SpanStack);
        Installed {
            _guard: tracing::subscriber::set_default(subscriber),
        }
    }
}

/// A [`KernelLog`] that takes this thread's messages while it lives.
#[must_use = "the log is uninstalled when this is dropped"]
pub struct Installed {
    /// Restores the previous subscriber when dropped.
    _guard: tracing::subscriber::DefaultGuard,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::RealFiles;
    use zbm_testkit::TempDir;
    use zbm_trace::Level;

    #[test]
    fn writes_records_and_markers() {
        let dir = TempDir::new("zbm-system-log");
        let kmsg = dir.join("kmsg");
        let log = KernelLog::new(&RealFiles, "6", dir.path(), 42).with_device(&kmsg);

        {
            let _installed = log.install();
            zbm_trace::info!("importing tank");
            zbm_trace::debug!("not shown");
            zbm_trace::warn!("canmount=on set for 'tank/ROOT'");
            zbm_trace::error!("unable to import a pool");
            zbm_trace::notice!("note");
            assert!(zbm_trace::enabled(Level::Info) && !zbm_trace::enabled(Level::Debug));
            assert!(zbm_trace::enabled(Level::Error) && zbm_trace::enabled(Level::Warning));
            assert!(zbm_trace::enabled(Level::Notice));
        }
        zbm_trace::error!("not written: the log is uninstalled");

        assert_eq!(
            std::fs::read_to_string(&kmsg).unwrap(),
            "<6>ZFSBootMenu: importing tank\n\
             <4>ZFSBootMenu: canmount=on set for 'tank/ROOT'\n\
             <3>ZFSBootMenu: unable to import a pool\n\
             <5>ZFSBootMenu: note\n"
        );
        assert!(dir.join("have_warnings").exists());
        assert!(dir.join("have_errors").exists());
    }

    #[test]
    fn the_journal_takes_every_event_the_kernel_log_leaves_out() {
        let dir = TempDir::new("zbm-system-log");
        let kmsg = dir.join("kmsg");
        let log = KernelLog::new(&RealFiles, "3", dir.path(), 42).with_device(&kmsg);

        {
            let _installed = log.install();
            let _span = tracing::info_span!("import", pool = "tank").entered();
            zbm_trace::debug!("debugging output");
            tracing::info!(passphrase = "hunter2", "a command ran");
            assert!(!zbm_trace::enabled(Level::Debug));
        }

        assert_eq!(std::fs::read_to_string(&kmsg).unwrap_or_default(), "");
        let journal = std::fs::read_to_string(dir.join(JOURNAL)).unwrap();
        let lines: Vec<&str> = journal.lines().collect();
        assert_eq!(lines.len(), 2, "{journal}");
        assert!(lines[0].contains(r#""message":"debugging output""#));
        assert!(lines[0].contains(r#""spans":[{"pool":"tank","span":"import"}]"#));
        assert!(lines[1].contains(r#""passphrase":"[redacted]""#));
        assert!(!journal.contains("hunter2"));
    }

    #[test]
    fn a_log_without_a_journal_writes_none() {
        let dir = TempDir::new("zbm-system-log");
        let log = KernelLog::new(&RealFiles, "7", dir.path(), 1)
            .with_device(&dir.join("kmsg"))
            .without_journal();

        let _installed = log.install();
        zbm_trace::info!("only for the kernel log");

        assert!(!dir.join(JOURNAL).exists());
    }

    #[test]
    fn labelled_lines_are_written_as_the_suites_expect() {
        let dir = TempDir::new("zbm-system-log");
        let log_file = dir.join("log");
        let log = KernelLog::new(&RealFiles, "0", dir.path(), 42)
            .with_device(&log_file)
            .labelled();

        let _installed = log.install();
        zbm_trace::info!("one");
        zbm_trace::warn!("two\nlines");

        assert_eq!(
            std::fs::read_to_string(&log_file).unwrap(),
            "INFO: one\nWARN: two\nlines\n"
        );
        assert!(dir.join("have_warnings").exists());
    }

    #[test]
    fn an_unwritable_device_is_not_an_error() {
        let dir = TempDir::new("zbm-system-log");
        let log = KernelLog::new(&RealFiles, "7", dir.path(), 1)
            .with_device(Path::new("/nonexistent/kmsg"));

        let _installed = log.install();
        zbm_trace::info!("dropped");
    }
}
