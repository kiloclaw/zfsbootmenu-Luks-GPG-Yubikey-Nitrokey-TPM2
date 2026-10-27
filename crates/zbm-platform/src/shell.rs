//! The shell the runtime runs its scripts, the hooks and the recovery shell
//! with. Images with the Rust runtime have brush ([`BRUSH`]), a
//! Bash-compatible shell in Rust; GNU Bash ([`BASH`]) stays in every image,
//! as dracut's `/bin/sh`. `zfsbootmenu-preinit` chooses the shell once, from
//! `zbm.shell` and what the image has ([`choose`]), and passes it on to every
//! program of the runtime in the environment ([`ENV`]), which [`chosen`]
//! reads.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Where images with the Rust runtime install brush.
pub const BRUSH: &str = "/usr/bin/brush";

/// GNU Bash.
pub const BASH: &str = "/bin/bash";

/// The environment variable that names the shell for the runtime's programs.
pub const ENV: &str = "ZBM_SHELL";

/// The script the shell runs for the regular expression match that
/// `org.zfsbootmenu:kernel` needs (`zbm_boot::kernels::bash_regex_match`):
/// `$1` matched against the pattern `$2`.
pub const BASH_MATCH: &str = "[[ $1 =~ $2 ]]";

/// The shell `zfsbootmenu-preinit` chooses: brush when the image has it
/// (`has_brush`) and `wanted`, the value of `zbm.shell`, is not `bash`;
/// GNU Bash otherwise.
pub fn choose(wanted: Option<&str>, has_brush: bool) -> &'static str {
    if has_brush && wanted != Some("bash") {
        BRUSH
    } else {
        BASH
    }
}

/// The shell named by `value`, the environment's [`ENV`], when it is an
/// absolute path.
pub fn from_env(value: Option<&OsStr>) -> Option<PathBuf> {
    value
        .map(Path::new)
        .filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
}

/// The shell `zfsbootmenu-preinit` chose for this process and the programs
/// it starts; `None` where it chose none (legacy images, tests), and each
/// caller keeps the Bash it ran before.
pub fn chosen() -> Option<PathBuf> {
    use crate::env::Environment as _;
    from_env(crate::env::ProcessEnvironment.var_os(ENV).as_deref())
}

/// Whether `program` is one of the shells: brush or GNU Bash, by name.
pub fn is_shell(program: &Path) -> bool {
    program
        .file_name()
        .is_some_and(|name| name == "bash" || name == "brush")
}

/// Whether the script whose first line is `first_line` is written for
/// the shell: it names Bash or `sh` after `#!` (through `env` or not), or
/// names no interpreter at all. A script for another interpreter, such as
/// `#!/usr/bin/python3`, is not.
pub fn is_shell_script(first_line: &str) -> bool {
    let Some(interpreter) = first_line.strip_prefix("#!") else {
        return true;
    };
    let mut words = interpreter.split_whitespace();
    let mut program = words.next().unwrap_or_default();
    if Path::new(program).file_name() == Some(OsStr::new("env")) {
        program = words
            .find(|word| !word.starts_with('-'))
            .unwrap_or_default();
    }
    matches!(
        Path::new(program).file_name().and_then(OsStr::to_str),
        Some("bash" | "sh")
    )
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn brush_unless_the_image_lacks_it_or_bash_is_asked_for() {
        assert_eq!(choose(None, true), BRUSH);
        assert_eq!(choose(Some("brush"), true), BRUSH);
        assert_eq!(choose(Some("bash"), true), BASH);
        assert_eq!(choose(None, false), BASH);
    }

    #[test]
    fn the_environment_names_the_shell_with_an_absolute_path() {
        assert_eq!(
            from_env(Some(OsStr::new(BRUSH))),
            Some(PathBuf::from(BRUSH))
        );
        assert_eq!(from_env(Some(OsStr::new("brush"))), None);
        assert_eq!(from_env(None), None);
        assert!(is_shell(Path::new("/usr/bin/brush")) && is_shell(Path::new("bash")));
        assert!(!is_shell(Path::new("/bin/zsh")));
    }

    #[test]
    fn shell_scripts_are_told_by_their_first_line() {
        for script in [
            "#!/bin/bash",
            "#!/bin/sh -e",
            "#! /usr/bin/env bash",
            "echo hi",
            "",
        ] {
            assert!(is_shell_script(script), "{script:?}");
        }
        for other in [
            "#!/usr/bin/python3",
            "#!/usr/bin/env -S perl -w",
            "#!/bin/zsh",
            "#!",
        ] {
            assert!(!is_shell_script(other), "{other:?}");
        }
    }
}
