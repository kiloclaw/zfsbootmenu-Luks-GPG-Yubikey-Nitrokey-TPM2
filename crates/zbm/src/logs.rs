//! `logs [--level LEVELS] [--trace] [--prompt]`, also `zlogtail`, and
//! `ztrace` as `logs --trace`: ZFSBootMenu's messages in the kernel log,
//! for the recovery shell (ADR 0018, points 12 and 13). The kernel log, not
//! the journal: hooks and the library's shell functions write to it alone;
//! the journal is `zbm debug journal`'s.
//!
//! - `logs` prints the errors and warnings since the boot, or the levels
//!   `--level` names, as `dmesg -l` takes them; then, as the legacy
//!   `zlogtail` did, it removes `have_errors` when it printed the errors,
//!   and writes the number of the last record it read (`errors_since`),
//!   and `have_warnings` when it printed the warnings.
//! - `logs --prompt`, the shell's `PROMPT_COMMAND`, reads nothing without
//!   `have_errors`. With it, it prints the errors after that number,
//!   between blank lines, as the legacy `recovery-error-printer` did,
//!   removes `have_errors` alone and writes the number.
//! - `logs --trace` prints the debug lines ([`trace`]).
//!
//! Reading the log back undoes the kernel's escapes, so each line is
//! printed escaped ([`dmesg::escaped`]), as the recovery shell's banner
//! prints the errors. It needs no terminal (`logs | cat`, the prompt): it
//! runs on the real system without a console. Where `/dev/kmsg` cannot be
//! read, `dmesg` reads the messages since the boot, and no number is
//! written; `--trace` then says why and exits 1. An unknown level or
//! option exits 2.

mod trace;

use std::io::IsTerminal as _;
use std::path::Path;

use zbm_config::conf::{self, Conf};
use zbm_platform::dmesg::{self, Record};
use zbm_platform::{NoConsole, RealSystem, System};
use zbm_startup::preinit;
use zbm_state::State;
use zbm_types::Printable;

use crate::table::LOGS;
use crate::{Io, USAGE};

/// The levels `logs` prints without `--level`.
const ERRORS_AND_WARNINGS: &str = "err,warn";

/// What `logs` was asked to print.
#[derive(Debug, Eq, PartialEq)]
enum Asked {
    /// The messages of levels: their names as given, for `dmesg`, and
    /// their numbers.
    Levels(String, Vec<u8>),
    /// The debug lines (`--trace`).
    Trace,
    /// The errors since the last prompt (`--prompt`).
    Prompt,
}

/// `logs` with `args` on this machine; colour for `--trace` when standard
/// output is a terminal.
pub(crate) fn run(args: &[String], io: &mut Io) -> i32 {
    let real = RealSystem::new();
    let colour = std::io::stdout().is_terminal();
    run_in(&real.with_console(&NoConsole), args, colour, io)
}

/// `logs` with `args` on `system`, in colour or not, with the state
/// directory the image's configuration names.
fn run_in(system: &System<'_>, args: &[String], colour: bool, io: &mut Io) -> i32 {
    let asked = match asked(args) {
        Ok(asked) => asked,
        Err(said) => {
            let _ = io.stderr.write_all(said.as_bytes());
            return USAGE;
        }
    };
    let conf = system
        .files
        .read_to_string(Path::new(conf::PATH))
        .map(|text| Conf::parse(&text))
        .unwrap_or_default();
    let state = State::new(system.files, preinit::base(&conf));
    match asked {
        Asked::Levels(names, levels) => show(system, &state, &names, &levels, io),
        Asked::Prompt => prompt(system, &state, io),
        Asked::Trace => trace::show(system.files, colour, io),
    }
}

/// What `args` ask for.
///
/// # Errors
///
/// What to say on standard error: the usage for an unknown option, a
/// missing list of levels or two of `--level`, `--trace` and `--prompt`;
/// why for a list that names no level or one that is not a level.
fn asked(args: &[String]) -> Result<Asked, String> {
    let usage = || LOGS.usage.to_owned();
    let (mut names, mut is_trace, mut is_prompt) = (None, false, false);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--level" => names = Some(args.next().ok_or_else(usage)?),
            "--trace" => is_trace = true,
            "--prompt" => is_prompt = true,
            _ => return Err(usage()),
        }
    }
    match (names, is_trace, is_prompt) {
        (names, false, false) => {
            let names = names.map_or(ERRORS_AND_WARNINGS, String::as_str);
            dmesg::levels(names)
                .filter(|levels| !levels.is_empty())
                .map(|levels| Asked::Levels(names.to_owned(), levels))
                .ok_or_else(|| {
                    format!(
                        "logs: {} is no list of levels: emerg, alert, crit, err, \
                         warn, notice, info, debug or 0 to 7, comma-separated\n",
                        Printable(names)
                    )
                })
        }
        (None, true, false) => Ok(Asked::Trace),
        (None, false, true) => Ok(Asked::Prompt),
        _ => Err(usage()),
    }
}

/// Prints ZFSBootMenu's messages of `levels` (named `names`). When they
/// include the errors, removes `have_errors` and writes the number of the
/// last record read; when they include the warnings, removes
/// `have_warnings`: the markers say what was logged and not yet shown.
fn show(system: &System<'_>, state: &State<'_>, names: &str, levels: &[u8], io: &mut Io) -> i32 {
    let read = dmesg::read_after(system.files, levels, None);
    let text = match &read {
        Ok((records, _)) => dmesg::texts(records),
        Err(_) => dmesg::read_logs(system, names).unwrap_or_default(),
    };
    if !text.trim_end_matches('\n').is_empty() {
        let _ = io.stdout.write_all(dmesg::escaped(&text).as_bytes());
    }
    if levels.contains(&dmesg::ERR) {
        state.clear_errors();
        stamp(state, &read);
    }
    if levels.contains(&dmesg::WARN) {
        state.clear_warnings();
    }
    0
}

/// `PROMPT_COMMAND`: with `have_errors`, prints the errors after the
/// number of the last record shown, between blank lines, removes
/// `have_errors` and writes the number of the last record read. Without
/// it, nothing is read or written.
fn prompt(system: &System<'_>, state: &State<'_>, io: &mut Io) -> i32 {
    if !state.has_errors() {
        return 0;
    }
    let read = dmesg::read_after(system.files, &[dmesg::ERR], state.errors_since());
    let text = match &read {
        Ok((records, _)) => dmesg::texts(records),
        Err(_) => dmesg::read_logs(system, "err").unwrap_or_default(),
    };
    let _ = io
        .stdout
        .write_all(format!("\n{}\n", dmesg::escaped(&text)).as_bytes());
    state.clear_errors();
    stamp(state, &read);
    0
}

/// Writes the number of the last record `read` read, if it read any.
fn stamp(state: &State<'_>, read: &std::io::Result<(Vec<Record>, Option<u64>)>) {
    if let Ok((_, Some(last))) = read {
        let _ = state.set_errors_since(*last);
    }
}

#[cfg(test)]
mod tests;
