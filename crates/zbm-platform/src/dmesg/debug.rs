//! The debug lines of the kernel log, which `ztrace` shows:
//! `ZBM:[PID]|TRACE|TEXT`, each with the calls that wrote it. Shell code
//! writes TRACE as `FUNCTION,FILE,LINE` calls joined by `;`, the outermost
//! first; Rust writes one `FILE,LINE`. A message of several lines carries
//! its TRACE on its last line alone.
//!
//! The kernel log is untrusted text (P13): any text is read without a
//! panic, and the parts are slices of it, still to be escaped.

/// The level of debug lines (`debug`).
pub const LEVEL: u8 = 7;

/// A debug line: `PREFIX|TRACE|TEXT`.
#[derive(Debug, Eq, PartialEq)]
pub struct Line<'t> {
    /// `ZBM:[PID]`.
    pub prefix: &'t str,
    /// The calls of TRACE, in its order.
    pub calls: Vec<Call<'t>>,
    /// The message.
    pub text: &'t str,
}

/// A call of a debug line's TRACE.
#[derive(Debug, Eq, PartialEq)]
pub struct Call<'t> {
    /// The function, which shell code names and Rust does not.
    pub function: Option<&'t str>,
    /// The file.
    pub file: &'t str,
    /// The line in the file.
    pub line: &'t str,
}

/// The debug line of a record's `text`: one that starts with `ZBM`, its
/// fields split at the first two `|`, as the legacy `ztrace` read them;
/// `None` for any other text.
pub fn line(text: &str) -> Option<Line<'_>> {
    if !text.starts_with("ZBM") {
        return None;
    }
    let mut fields = text.splitn(3, '|');
    let prefix = fields.next().unwrap_or_default();
    let trace = fields.next().unwrap_or_default();
    let text = fields.next().unwrap_or_default();
    let calls = trace
        .split(';')
        .filter(|call| !call.is_empty())
        .map(call)
        .collect();
    Some(Line {
        prefix,
        calls,
        text,
    })
}

/// The call of `token`: `FUNCTION,FILE,LINE` from shell code, the rest
/// of it after the second comma being the line; `FILE,LINE` from Rust.
fn call(token: &str) -> Call<'_> {
    let mut fields = token.splitn(3, ',');
    let first = fields.next().unwrap_or_default();
    match (fields.next(), fields.next()) {
        (Some(file), Some(line)) => Call {
            function: Some(first),
            file,
            line,
        },
        (file, _) => Call {
            function: None,
            file: first,
            line: file.unwrap_or_default(),
        },
    }
}
