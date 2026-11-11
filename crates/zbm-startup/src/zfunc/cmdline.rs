//! ZFSBootMenu's command line as the library prints it: zbmcmdline, and
//! the report's command line, which is the same.

use std::io::Write;
use std::path::Path;

use zbm_platform::{Files, System};
use zbm_state::State;
use zbm_types::Printable;

use crate::preinit::{zbm_cmdline, zbm_sources};

/// ZFSBootMenu's command line, its arguments joined by one space:
/// `zbm.cmdline` of the state directory `base`, one argument per line, as
/// it is, or, when it cannot be read, the command line [`built`] in its
/// place.
pub(crate) fn assembled(files: &dyn Files, base: &Path) -> String {
    let saved = base.join("zbm.cmdline");
    let tokenized = files.read(&saved).map_or_else(
        |_| built(files, &saved),
        |bytes| String::from_utf8_lossy(&bytes).into_owned(),
    );
    tokenized
        .split_terminator('\n')
        .collect::<Vec<_>>()
        .join(" ")
}

/// The command line built from the image's and the kernel's, as start-up
/// builds it, one argument per line; saved as `saved` with a newline, as
/// the legacy function saved it, an empty one too.
fn built(files: &dyn Files, saved: &Path) -> String {
    let (cmdline, _) = zbm_cmdline(&zbm_sources(files, Path::new("/")));
    let tokenized = cmdline.one_per_line();
    let written = if tokenized.is_empty() {
        "\n"
    } else {
        &tokenized
    };
    if let Err(error) = files.write(saved, written.as_bytes()) {
        zbm_trace::debug!(
            "unable to save the command line in {saved}: {error}";
            saved = %saved.display(), %error
        );
    }
    tokenized
}

/// zbmcmdline: prints ZFSBootMenu's command line on `out`, with a
/// newline, its control characters escaped.
pub(super) fn zbmcmdline(
    system: &System<'_>,
    state: &State<'_>,
    _args: &[String],
    out: &mut dyn Write,
) -> i32 {
    let line = assembled(system.files, state.base());
    super::printed(writeln!(out, "{}", Printable(&line)))
}

#[cfg(test)]
mod tests;
