//! Crash reports: what a panic or a fatal signal leaves behind for a
//! developer.
//!
//! A report names the program, why it ended, where (for a panic), the build
//! ID of the binary, the `tracing` spans that were open ([`SpanStack`]) and
//! the stack as offsets from the load address of each binary
//! ([`locate`]). Offsets, unlike addresses, do not change with address space
//! randomisation, so `cargo xtask symbolize` can turn them into function
//! names, files and lines with the debug file of that build ID, which is kept
//! out of the boot image.
//!
//! ```
//! use zbm_trace::crash::{Frame, Reason, Report};
//!
//! let report = Report {
//!     program: "zfsbootmenu-init".to_owned(),
//!     binary: "/libexec/zbm".to_owned(),
//!     pid: 412,
//!     time: 1_700_000_000,
//!     reason: Reason::Panic {
//!         message: "no pools".to_owned(),
//!         location: Some("crates/zbm-boot/src/import.rs:40:9".to_owned()),
//!     },
//!     build_id: Some("5b60ff28".to_owned()),
//!     spans: vec!["startup{stage=import}".to_owned()],
//!     frames: vec![Frame {
//!         offset: 0x1234,
//!         module: "/libexec/zbm".to_owned(),
//!     }],
//! };
//! let text = report.render();
//! let reason = "panicked at crates/zbm-boot/src/import.rs:40:9: no pools";
//! assert!(text.contains(&format!("reason: {reason}\n")));
//! assert_eq!(Report::parse(&text), Some(report));
//! ```

mod elf;
mod maps;
mod spans;

pub use elf::build_id;
pub use maps::{Mapping, locate, parse_maps};
pub use spans::{SpanStack, open_spans};

use std::fmt::Write as _;

/// The first line of every report, by which tools recognise one.
pub const HEADER: &str = "ZFSBootMenu crash report";

/// What the `spans` line says when no span was open.
const NO_SPANS: &str = "(none)";

/// Why the program ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reason {
    /// It panicked, with this message, at this place in the source.
    Panic {
        /// The panic message.
        message: String,
        /// `file:line:column`, if known.
        location: Option<String>,
    },
    /// The kernel ended it with a signal.
    Signal {
        /// The signal number.
        number: i32,
        /// Whether it left a core dump.
        core: bool,
    },
}

impl Reason {
    /// One line saying why.
    fn line(&self) -> String {
        match self {
            Reason::Panic {
                message,
                location: Some(location),
            } => format!("panicked at {location}: {message}"),
            Reason::Panic {
                message,
                location: None,
            } => format!("panicked: {message}"),
            Reason::Signal { number, core } => {
                let core = if *core { " (core dumped)" } else { "" };
                format!("killed by signal {number} ({}){core}", signal_name(*number))
            }
        }
    }

    /// The reason from its [`line`](Reason::line).
    fn parse(line: &str) -> Option<Reason> {
        if let Some(rest) = line.strip_prefix("killed by signal ") {
            let (number, rest) = rest.split_once(' ')?;
            return Some(Reason::Signal {
                number: number.parse().ok()?,
                core: rest.ends_with(" (core dumped)"),
            });
        }
        if let Some(message) = line.strip_prefix("panicked: ") {
            return Some(Reason::Panic {
                message: message.to_owned(),
                location: None,
            });
        }
        let rest = line.strip_prefix("panicked at ")?;
        let (location, message) = split_location(rest)?;
        Some(Reason::Panic {
            message: message.to_owned(),
            location: Some(location.to_owned()),
        })
    }
}

/// Splits `file:line:column: message` after the column.
fn split_location(text: &str) -> Option<(&str, &str)> {
    text.match_indices(": ").find_map(|(index, _)| {
        let location = text.get(..index)?;
        let mut parts = location.rsplitn(3, ':');
        let column = parts.next()?;
        let line = parts.next()?;
        let number =
            |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
        (number(column) && number(line) && parts.next().is_some())
            .then(|| Some((location, text.get(index + 2..)?)))
            .flatten()
    })
}

/// The usual name of signal `number` on Linux.
pub fn signal_name(number: i32) -> &'static str {
    match number {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        31 => "SIGSYS",
        _ => "unknown signal",
    }
}

/// One frame of the stack: an offset from the load address of a binary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    /// The return address minus the address the binary was loaded at.
    pub offset: u64,
    /// The binary's path, or `?` when no binary holds the address.
    pub module: String,
}

/// A crash report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Report {
    /// The program (applet) that crashed.
    pub program: String,
    /// The binary it ran, whose frames the debug file resolves.
    pub binary: String,
    /// Its process ID.
    pub pid: u32,
    /// When, in seconds since 1970.
    pub time: u64,
    /// Why it ended.
    pub reason: Reason,
    /// The build ID of the binary, in hexadecimal.
    pub build_id: Option<String>,
    /// The spans that were open, outermost first.
    pub spans: Vec<String>,
    /// The stack, innermost first.
    pub frames: Vec<Frame>,
}

impl Report {
    /// The report as text, which [`Report::parse`] reads back.
    pub fn render(&self) -> String {
        let mut text = format!("{HEADER}\n");
        let _ = writeln!(text, "program: {}", self.program);
        let _ = writeln!(text, "binary: {}", self.binary);
        let _ = writeln!(text, "pid: {}", self.pid);
        let _ = writeln!(text, "time: {}", self.time);
        let _ = writeln!(text, "reason: {}", self.reason.line());
        let build_id = self.build_id.as_deref().unwrap_or("unknown");
        let _ = writeln!(text, "build-id: {build_id}");
        let spans = if self.spans.is_empty() {
            NO_SPANS.to_owned()
        } else {
            self.spans.join(" > ")
        };
        let _ = writeln!(text, "spans: {spans}");
        text.push_str("frames:\n");
        for (number, frame) in self.frames.iter().enumerate() {
            let _ = writeln!(text, "  #{number} {:#018x} {}", frame.offset, frame.module);
        }
        text
    }

    /// The name of the file a report goes to: `crash-TIME-PID.txt`, which
    /// sorts by time for reports of the same century.
    pub fn file_name(&self) -> String {
        format!("crash-{}-{}.txt", self.time, self.pid)
    }

    /// Reads a report that [`Report::render`] wrote, or `None` if `text` is
    /// not one.
    pub fn parse(text: &str) -> Option<Report> {
        let mut lines = text.lines();
        if lines.next()? != HEADER {
            return None;
        }
        let mut field = |name: &str| -> Option<String> {
            let value = lines.next()?.strip_prefix(name)?.strip_prefix(':')?;
            Some(value.strip_prefix(' ').unwrap_or(value).to_owned())
        };
        let program = field("program")?;
        let binary = field("binary")?;
        let pid = field("pid")?.parse().ok()?;
        let time = field("time")?.parse().ok()?;
        let reason = Reason::parse(&field("reason")?)?;
        let build_id = Some(field("build-id")?).filter(|id| id != "unknown");
        let spans = field("spans")?;
        let spans = if spans == NO_SPANS {
            Vec::new()
        } else {
            spans.split(" > ").map(str::to_owned).collect()
        };
        field("frames")?;
        let frames = lines.map_while(parse_frame).collect();
        Some(Report {
            program,
            binary,
            pid,
            time,
            reason,
            build_id,
            spans,
            frames,
        })
    }
}

/// A frame line: `  #N 0xOFFSET MODULE`.
fn parse_frame(line: &str) -> Option<Frame> {
    let rest = line.trim_start().strip_prefix('#')?;
    let (_, rest) = rest.split_once(' ')?;
    let (offset, module) = rest.split_once(' ')?;
    let offset = u64::from_str_radix(offset.strip_prefix("0x")?, 16).ok()?;
    Some(Frame {
        offset,
        module: module.to_owned(),
    })
}

#[cfg(test)]
mod tests;
