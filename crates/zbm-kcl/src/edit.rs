//! Editing a command line in `$EDITOR`.

use std::path::Path;

use crate::tokens as kcl;
use zbm_platform::filetest::{create_temp_file, is_readable};
use zbm_tools::programs;

use crate::{Context, Failure, TempFile, command_exists};

/// The text added below the command line when it is opened for editing.
const EDIT_HELP: &str = "\n\n# KCL processing ends with the first line that contains no text.
# Anything starting with # is considered a comment and will be ignored.
# Multiple lines will be concatenated with spaces to form a single line.
";

/// The editor to run: `$EDITOR`, or `vi` when it is unset or empty.
fn editor(context: &Context) -> String {
    context
        .editor
        .clone()
        .filter(|editor| !editor.is_empty())
        .unwrap_or_else(|| "vi".to_owned())
}

/// Opens the tokenized command line in `temp_kcl` in the editor, with
/// [`EDIT_HELP`] below it, and saves what the editor leaves back into
/// `temp_kcl`, tokenized again.
///
/// # Errors
///
/// The message to print when there is no editor, a file cannot be read or
/// written, or the editor fails.
pub(crate) fn in_editor(context: &Context, temp_kcl: &Path) -> Result<(), Failure> {
    let editor = editor(context);
    if !command_exists(&editor, context.path.as_deref()) {
        return Err(Failure::of("define $EDITOR to edit".to_owned()));
    }
    let tokens = is_readable(temp_kcl)
        .then(|| std::fs::read(temp_kcl).ok())
        .flatten()
        .ok_or_else(|| Failure::of("unable to read KCL".to_owned()))?;
    let failed_temp = || Failure::of("failed to create temporary file".to_owned());
    let file = create_temp_file(&context.temp_dir, "tmp.XXXXXXXXXX").map_err(|_| failed_temp())?;
    let file = TempFile(file);
    let mut text = kcl::assemble(&String::from_utf8_lossy(&tokens));
    text.push_str(EDIT_HELP);
    std::fs::write(&file.0, text).map_err(|_| failed_temp())?;

    let edited = programs::edit(context.runner, &editor, &file.0)
        .is_ok_and(|output| output.status.is_success());
    if !edited {
        return Err(Failure::of("failed to edit KCL".to_owned()));
    }

    let edited = std::fs::read(&file.0)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    std::fs::write(temp_kcl, kcl::tokenize(&strip_kcl(&edited)))
        .map_err(|_| Failure::of("failed to save edited KCL".to_owned()))
}

/// Joins the lines before the first empty line with spaces, without comments
/// (from the first `#` and the blanks before it) and without lines left
/// empty.
pub fn strip_kcl(text: &str) -> String {
    let mut lines = Vec::new();
    for line in kcl::lines(text) {
        if line.is_empty() {
            break;
        }
        let line = match line.find('#') {
            Some(hash) => line[..hash].trim_end_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']),
            None => line,
        };
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines.join(" ")
}
