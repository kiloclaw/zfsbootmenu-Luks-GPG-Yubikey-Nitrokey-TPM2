//! The shell of the Rust runtime: brush, or GNU Bash.

use super::super::{Doc, OptionSpec, Reader, Source, Value};

/// `zbm.shell`: the shell that runs the hooks, the recovery shell and the
/// runtime's own scripts in images with the Rust runtime.
pub static SHELL: OptionSpec = OptionSpec {
    name: "zbm.shell",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Choice(&["brush", "bash"]),
    default: Some("brush"),
    summary: "The shell of the hooks and the recovery shell: brush, or GNU Bash",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.shell=bash**",
        body: SHELL_DOC,
    },
    deprecation: None,
    examples: &["zbm.shell=bash", "zbm.shell=brush"],
};

const SHELL_DOC: &str = r"
In images built with the Rust runtime, the hooks, the recovery shell, the shell of SSH sessions and
the runtime's own scripts run on *brush*, a Bash-compatible shell written in Rust, which completes
commands, suggests them from the history and keeps no history file. A hook whose first line names
*bash* or *sh*, or that names nothing, runs on it; a hook in another language runs as before. On a
serial console, the recovery shell uses brush's basic line editor, which asks the terminal nothing.
With **zbm.shell=bash**, all of them run on GNU Bash, as in images with the legacy runtime: for a
hook that brush runs differently. GNU Bash stays in the image as */bin/sh* and */bin/bash* either
way, and starts when brush cannot.
";
