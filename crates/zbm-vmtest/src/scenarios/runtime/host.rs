//! Commands on the host for the user who started the tests with `sudo`.

use std::env;
use std::fs;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

use crate::error::{Error, Result};

/// A command and the `PATH` it runs with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HostCommand {
    /// The program and its arguments.
    pub args: Vec<String>,
    /// The `PATH` its programs are found in.
    pub path: String,
}

/// `args` as a command for the user who started the tests with `sudo`, so
/// that their Rust toolchain (`~/.cargo/bin`) is found and the build files
/// in the checkout stay theirs; as root, with root's `PATH`, otherwise.
///
/// # Errors
///
/// [`Error::Setup`] when `SUDO_USER` names a user `/etc/passwd` lacks.
pub(super) fn host_user_command(args: &[String]) -> Result<HostCommand> {
    let path = env::var("PATH").unwrap_or_default();
    let root = fs::metadata("/proc/self").is_ok_and(|meta| meta.uid() == 0);
    let user = env::var("SUDO_USER")
        .ok()
        .filter(|user| root && !user.is_empty() && user != "root");
    let Some(user) = user else {
        return Ok(user_command(args, None, &path));
    };
    let passwd = fs::read_to_string("/etc/passwd").map_err(Error::io("reading /etc/passwd"))?;
    let home = home_in_passwd(&passwd, &user)
        .ok_or_else(|| Error::Setup(format!("getpwnam(): name not found: {user}")))?;
    Ok(user_command(args, Some((&user, &home)), &path))
}

/// `args` as a command for `sudo_user` (name, home directory), or as is
/// with `path` when there is none.
fn user_command(args: &[String], sudo_user: Option<(&str, &str)>, path: &str) -> HostCommand {
    let Some((user, home)) = sudo_user else {
        return HostCommand {
            args: args.to_vec(),
            path: path.to_owned(),
        };
    };
    let path =
        format!("{home}/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
    let mut command: Vec<String> = ["runuser", "-u", user, "--", "env"]
        .iter()
        .map(|&arg| arg.to_owned())
        .collect();
    command.push(format!("HOME={home}"));
    command.push(format!("PATH={path}"));
    command.extend_from_slice(args);
    HostCommand {
        args: command,
        path,
    }
}

/// The home directory of `user` in the text of `/etc/passwd`.
fn home_in_passwd(passwd: &str, user: &str) -> Option<String> {
    passwd.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        (fields.len() >= 7 && fields[0] == user).then(|| fields[5].to_owned())
    })
}

/// Whether `tool` is an executable file in a directory of `path`, as
/// Python's `shutil.which(tool, path=path)` finds it.
pub(super) fn is_on_path(tool: &str, path: &str) -> bool {
    path.split(':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(dir).join(tool))
        .any(|file| {
            fs::metadata(file)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn args() -> Vec<String> {
        ["cargo", "xtask", "symbolize", "/w/report.txt"]
            .iter()
            .map(|&arg| arg.to_owned())
            .collect()
    }

    #[test]
    fn without_a_sudo_user_the_command_runs_as_is() {
        let command = user_command(&args(), None, "/usr/bin:/bin");

        assert_eq!(command.args, args());
        assert_eq!(command.path, "/usr/bin:/bin");
    }

    #[test]
    fn a_sudo_user_runs_the_command_with_their_cargo() {
        let command = user_command(&args(), Some(("ann", "/home/ann")), "/root/bin");

        let path = "/home/ann/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:\
                    /sbin:/bin";
        assert_eq!(
            command.args,
            [
                "runuser",
                "-u",
                "ann",
                "--",
                "env",
                "HOME=/home/ann",
                &format!("PATH={path}"),
                "cargo",
                "xtask",
                "symbolize",
                "/w/report.txt"
            ]
        );
        assert_eq!(command.path, path);
    }

    #[test]
    fn the_home_directory_is_read_from_passwd() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\n\
                      ann:x:1000:1000:Ann,,,:/home/ann:/bin/bash\nbroken:x\n";

        assert_eq!(home_in_passwd(passwd, "ann").as_deref(), Some("/home/ann"));
        assert_eq!(home_in_passwd(passwd, "an"), None);
        assert_eq!(home_in_passwd(passwd, "broken"), None);
    }

    #[test]
    fn tools_are_found_in_the_path_when_executable() {
        let dir = std::env::temp_dir().join(format!("zbm-vmtest-which-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tool"), "").unwrap();
        fs::write(dir.join("data"), "").unwrap();
        fs::set_permissions(dir.join("tool"), fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("/nonexistent::{}", dir.display());

        let found = (is_on_path("tool", &path), is_on_path("data", &path));

        assert_eq!(found, (true, false));
        assert!(!is_on_path("tool", "/nonexistent"));
        fs::remove_dir_all(&dir).ok();
    }
}
