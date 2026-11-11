//! The markers of what was logged: whether errors and warnings were logged
//! since they were last shown (`have_errors`, `have_warnings`), which the
//! logging writes, and where the recovery shell last showed the errors
//! from (`errors_since`).
//!
//! The legacy prompt keeps a time in `errors_since`, which `dmesg --since`
//! takes. The stamp read and written here is the number of the last record
//! of the kernel log read (`zbm_platform::dmesg::Record::sequence`) instead:
//! records written in the same microsecond have the same time, never the
//! same number. A time in the file reads as no stamp. The legacy prompt's
//! `dmesg --since` refuses a number, so nothing writes one while a Rust
//! image still runs that prompt.

use zbm_platform::error::Result;

use super::State;

/// The file of the stamp.
const ERRORS_SINCE: &str = "errors_since";

impl State<'_> {
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

    /// The number of the last record of the kernel log that was read when
    /// errors were last shown (`errors_since`): the errors to show next
    /// come after it. `None` without one, or when the file holds no number,
    /// such as the legacy runtime's time.
    pub fn errors_since(&self) -> Option<u64> {
        self.read_line(ERRORS_SINCE)?.parse().ok()
    }

    /// Records that the errors up to the record numbered `sequence` were
    /// shown. A shell's prompt may read it meanwhile, so it is replaced.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_errors_since(&self, sequence: u64) -> Result<()> {
        self.replace(ERRORS_SINCE, format!("{sequence}\n").as_bytes())
    }
}
