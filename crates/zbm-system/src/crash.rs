//! The runtime's panic hook: a crash report in the state directory.
//!
//! [`CrashReporter::install`] makes every panic of the process leave a
//! report (`zbm_trace::crash::Report`) in `/zfsbootmenu/crash-TIME-PID.txt`,
//! log an error to the kernel log that names it, and write the report to the
//! journal, before the standard hook prints the panic on standard error. The
//! supervisor finds the report and points the recovery shell at it, and
//! `zbm debug crash` shows it. The binary is stripped in the boot image, so
//! the report holds offsets that `cargo xtask symbolize` resolves with the
//! debug file of the same build ID.

use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use zbm_trace::crash::{Reason, Report, build_id, locate, open_spans, parse_maps};

use crate::clock::{Clock as _, SystemClock};
use crate::files::Files;

/// Where the kernel shows the process itself.
const PROC_SELF: &str = "/proc/self";

/// Writes crash reports of one program to a state directory.
#[derive(Clone)]
pub struct CrashReporter {
    /// The files: the state directory and `/proc`.
    files: Arc<dyn Files + Send + Sync>,
    /// The state directory.
    base: PathBuf,
    /// The program (applet) the reports name.
    program: String,
    /// The process's directory in `/proc`, which holds `maps` and `exe`.
    proc: PathBuf,
}

impl CrashReporter {
    /// A reporter for `program` that writes to `base` through `files`.
    pub fn new(files: &dyn Files, base: &Path, program: &str) -> CrashReporter {
        CrashReporter {
            files: files.shared(),
            base: base.to_owned(),
            program: program.to_owned(),
            proc: PathBuf::from(PROC_SELF),
        }
    }

    /// The same reporter, reading `maps` and `exe` from `dir` instead of
    /// `/proc/self`.
    #[must_use]
    pub fn with_proc(mut self, dir: &Path) -> CrashReporter {
        dir.clone_into(&mut self.proc);
        self
    }

    /// The report of this process ending for `reason`, at `time` (seconds
    /// since 1970), with the stack `addresses` (innermost first), the spans
    /// open on this thread and the build ID of this process's binary.
    pub fn report(&self, reason: Reason, addresses: &[usize], time: u64) -> Report {
        let maps = self
            .files
            .read_to_string(&self.proc.join("maps"))
            .map(|text| parse_maps(&text))
            .unwrap_or_default();
        let frames = addresses
            .iter()
            .map(|address| locate(u64::try_from(*address).unwrap_or(u64::MAX), &maps))
            .collect();
        let binary = self
            .files
            .canonicalize(&self.proc.join("exe"))
            .map_or_else(|_| "?".to_owned(), |path| path.display().to_string());
        Report {
            program: self.program.clone(),
            binary,
            pid: std::process::id(),
            time,
            reason,
            build_id: self
                .files
                .read(&self.proc.join("exe"))
                .ok()
                .and_then(|exe| build_id(&exe)),
            spans: open_spans(),
            frames,
        }
    }

    /// Writes `report` to the state directory and returns its path, or
    /// `None` if it could not be written. A report of the same process and
    /// second that is there already is kept: the new one is
    /// `crash-TIME-PID.N.txt`.
    pub fn write(&self, report: &Report) -> Option<PathBuf> {
        let name = report.file_name();
        let stem = name.strip_suffix(".txt").unwrap_or(&name);
        let path = std::iter::once(name.clone())
            .chain((1..100).map(|number| format!("{stem}.{number}.txt")))
            .map(|name| self.base.join(name))
            .find(|path| !self.files.exists(path))?;
        self.files
            .write(&path, report.render().as_bytes())
            .ok()
            .map(|()| path)
    }

    /// Records a panic: writes its report, logs an error naming it, and
    /// writes the report to the journal. Returns the report's path.
    pub fn record_panic(&self, info: &PanicHookInfo<'_>, addresses: &[usize]) -> Option<PathBuf> {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "Box<dyn Any>".to_owned());
        let location = info.location().map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        });
        let time = SystemClock::new()
            .wall()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let report = self.report(
            Reason::Panic {
                message: message.clone(),
                location,
            },
            addresses,
            time,
        );
        let path = self.write(&report);
        let place = path.as_deref().map_or_else(
            || "it could not be saved".to_owned(),
            |path| format!("see {}", path.display()),
        );
        zbm_trace::error!("{} crashed: {message}; {place}", self.program);
        tracing::error!(target: "zbm::crash", report = %report.render(), "crash report");
        path
    }

    /// Makes every panic of this process record its crash report, then run
    /// the hook that was installed before (normally the standard one, which
    /// prints the panic on standard error).
    pub fn install(self) {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let addresses = zbm_sys::return_addresses();
            let _ = self.record_panic(info, &addresses);
            previous(info);
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::RealFiles;
    use zbm_testkit::TempDir;

    const MAPS_TEXT: &str = "\
5600aa000000-5600aa040000 r--p 00000000 00:11 12 /usr/libexec/zfsbootmenu/zbm
5600aa040000-5600aa200000 r-xp 00040000 00:11 12 /usr/libexec/zfsbootmenu/zbm
";

    #[test]
    fn a_report_locates_the_stack_in_the_binary_and_names_the_open_spans() {
        let dir = TempDir::new("zbm-crash");
        dir.write("proc/maps", MAPS_TEXT);
        dir.write("proc/exe", "not an ELF file");
        let reporter = CrashReporter::new(&RealFiles, dir.path(), "zfsbootmenu-init")
            .with_proc(&dir.join("proc"));
        let subscriber = {
            use tracing_subscriber::layer::SubscriberExt as _;
            tracing_subscriber::registry().with(zbm_trace::crash::SpanStack)
        };

        let report = tracing::subscriber::with_default(subscriber, || {
            let _span = tracing::info_span!("import", pool = "tank").entered();
            let segfault = Reason::Signal {
                number: 11,
                core: false,
            };
            reporter.report(segfault, &[0x5600_aa04_1234, 0x10], 1_700_000_000)
        });

        assert_eq!(report.program, "zfsbootmenu-init");
        assert_eq!(report.build_id, None);
        assert_eq!(report.spans, ["import{pool=tank}"]);
        assert_eq!(report.frames[0].offset, 0x41234);
        assert_eq!(report.frames[0].module, "/usr/libexec/zfsbootmenu/zbm");
        assert_eq!(report.frames[1].module, "?");
    }

    #[test]
    fn reports_are_written_to_the_state_directory() {
        let dir = TempDir::new("zbm-crash");
        let reporter = CrashReporter::new(&RealFiles, dir.path(), "zluks");
        let abort = Reason::Signal {
            number: 6,
            core: false,
        };
        let report = reporter.report(abort, &[], 7);

        let path = reporter.write(&report).unwrap();

        assert_eq!(
            path,
            dir.join(format!("crash-7-{}.txt", std::process::id()))
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), report.render());
        let nowhere = CrashReporter::new(&RealFiles, &dir.join("missing"), "zluks");
        assert_eq!(nowhere.write(&report), None);
    }

    #[test]
    fn the_installed_hook_reports_every_panic_then_runs_the_one_before() {
        let dir = TempDir::new("zbm-crash");
        let before = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = std::sync::Arc::clone(&before);
        let original = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |_| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
        CrashReporter::new(&RealFiles, dir.path(), "zbm-test").install();

        let detail = String::from("formatted");
        let first = std::panic::catch_unwind(|| panic!("a literal"));
        let second = std::panic::catch_unwind(|| panic!("a {detail} message"));
        std::panic::set_hook(original);

        assert!(first.is_err() && second.is_err());
        assert_eq!(before.load(std::sync::atomic::Ordering::SeqCst), 2);
        let texts: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
            .collect();
        assert_eq!(texts.len(), 2, "{texts:?}");
        for message in ["a literal", "a formatted message"] {
            let ending = format!(": {message}");
            let reported = |text: &String| {
                text.lines()
                    .any(|line| line.starts_with("reason: panicked at ") && line.ends_with(&ending))
            };
            assert!(texts.iter().any(reported), "{texts:?}");
        }
    }

    #[test]
    fn a_report_that_cannot_be_saved_is_still_logged() {
        let reporter = CrashReporter::new(&RealFiles, Path::new("/nonexistent/zbm"), "zbm-test");
        let previous = std::panic::take_hook();
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(Some(PathBuf::new())));
        let sink = std::sync::Arc::clone(&recorded);
        std::panic::set_hook(Box::new(move |info| {
            *sink.lock().unwrap() = reporter.record_panic(info, &[]);
        }));

        let result = std::panic::catch_unwind(|| panic!("nowhere to write"));
        std::panic::set_hook(previous);

        assert!(result.is_err());
        assert_eq!(*recorded.lock().unwrap(), None);
    }

    #[test]
    fn a_panic_leaves_a_report_with_its_message_place_and_build_id() {
        let dir = TempDir::new("zbm-crash");
        let reporter = CrashReporter::new(&RealFiles, dir.path(), "zbm-test");
        let previous = std::panic::take_hook();
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(None));
        let sink = std::sync::Arc::clone(&recorded);
        std::panic::set_hook(Box::new(move |info| {
            let addresses = zbm_sys::return_addresses();
            *sink.lock().unwrap() = reporter.record_panic(info, &addresses);
        }));

        let result = std::panic::catch_unwind(|| panic!("the pool is gone"));
        std::panic::set_hook(previous);

        assert!(result.is_err());
        let path = recorded.lock().unwrap().clone().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let report = Report::parse(&text).unwrap();
        assert!(
            text.lines().any(|line| line
                .starts_with("reason: panicked at crates/zbm-system/src/crash.rs:")
                && line.ends_with(": the pool is gone")),
            "{text}"
        );
        assert!(report.build_id.is_some(), "{text}");
        assert!(report.frames.len() > 3, "{text}");
        let exe = std::fs::canonicalize("/proc/self/exe").unwrap();
        assert!(
            report
                .frames
                .iter()
                .any(|frame| Path::new(&frame.module) == exe),
            "{text}"
        );
    }
}
