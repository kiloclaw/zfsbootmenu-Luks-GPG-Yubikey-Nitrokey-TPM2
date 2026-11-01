//! What the image plan asks the build host's programs (ROADMAP.md SB8, ADR
//! 0006): which options `fzf` takes, ZFS's version and the host ID, the
//! dynamic linker's cache and `zpool`'s libraries, GCC's runtime, GnuPG's
//! components and packets and a keyring of public keys, and what uutils and
//! busybox provide. Each answer is the program's, or nothing when it cannot
//! run.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::path::Path;

use zbm_platform::{Command, Files, Input, Runner, Stream};

/// The standard output of `command`, when it ran and succeeded.
fn output(runner: &dyn Runner, command: &Command) -> Option<String> {
    let output = runner.run(command).ok()?;
    output.status.is_success().then(|| output.stdout_text())
}

/// The quiet form of `program args`.
fn quiet(program: &str, args: &[&str]) -> Command {
    Command::new(program)
        .args(args.iter().copied())
        .stdin(Input::Null)
        .stderr(Stream::Null)
}

/// Whether `fzf` takes `options`: it filters `abc` with them and succeeds.
pub fn fzf_supports(runner: &dyn Runner, options: &[&str]) -> bool {
    let command = Command::new("fzf")
        .args(["-f", "abc"])
        .args(options.iter().copied())
        .arg("--exit-0")
        .stdin(Input::Bytes(b"abc\n".to_vec()))
        .stdout(Stream::Null)
        .stderr(Stream::Null);
    runner
        .run(&command)
        .is_ok_and(|output| output.status.is_success())
}

/// ZFS's version, as the first line of `zfs version` gives it, without
/// `zfs-` and `kmod-`: `2.2.6-1`.
pub fn zfs_version(runner: &dyn Runner) -> Option<String> {
    let text = output(runner, &quiet("zfs", &["version"]))?;
    let line = text.lines().next()?;
    let version = line.strip_prefix("zfs-")?;
    Some(version.strip_prefix("kmod-").unwrap_or(version).to_owned())
}

/// The host ID `hostid` prints, eight hexadecimal digits.
pub fn hostid(runner: &dyn Runner) -> Option<String> {
    let text = output(runner, &quiet("hostid", &[]))?;
    let id = text.trim();
    (id.len() == 8 && id.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| id.to_owned())
}

/// GCC's current runtime profile (`gcc-config -c`), such as
/// `x86_64-pc-linux-gnu-14`.
pub fn gcc_profile(runner: &dyn Runner) -> Option<String> {
    let text = output(runner, &quiet("gcc-config", &["-c"]))?;
    Some(text.trim().to_owned()).filter(|profile| !profile.is_empty())
}

/// The dynamic linker's cache, as `ldconfig -p` prints it.
pub fn linker_cache(runner: &dyn Runner) -> String {
    output(runner, &quiet("ldconfig", &["-p"])).unwrap_or_default()
}

/// The libraries `program` links, as `ldd` prints them.
pub fn libraries_of(runner: &dyn Runner, program: &Path) -> String {
    let command = Command::new("ldd")
        .arg(program.as_os_str())
        .stdin(Input::Null)
        .stderr(Stream::Null);
    output(runner, &command).unwrap_or_default()
}

/// GnuPG's components and their programs (`gpgconf --list-components`):
/// `name:description:path` a line.
pub fn gpg_components(runner: &dyn Runner) -> String {
    output(runner, &quiet("gpgconf", &["--list-components"])).unwrap_or_default()
}

/// The packets of the GnuPG file `file`, without decrypting it
/// (`gpg --list-only --list-packets`).
pub fn gpg_packets(runner: &dyn Runner, file: &Path) -> String {
    let command = Command::new("gpg")
        .args(["--batch", "--list-only", "--list-packets"])
        .arg(file.as_os_str())
        .stdin(Input::Null)
        .stderr(Stream::Null);
    // `gpg` fails without the secret key, after it listed the packets.
    runner
        .run(&command)
        .map(|output| output.stdout_text())
        .unwrap_or_default()
}

/// The programs the multi-call binary `coreutils` provides, when it is
/// uutils' (`coreutils --list`); GNU's has no list.
pub fn coreutils_programs(runner: &dyn Runner) -> Option<Vec<String>> {
    let text = output(runner, &quiet("coreutils", &["--list"]))?;
    Some(
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

/// The programs mkinitcpio's busybox provides.
pub fn busybox_programs(runner: &dyn Runner) -> Vec<String> {
    output(runner, &quiet("/usr/lib/initcpio/busybox", &["--list"]))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A keyring of the public keys `ids` of the build host's keyring and of
/// the key files `files`, built in `home`, an empty directory of a short
/// path, which the caller removes: the keyring's files, by name, and what
/// could not be added.
pub fn gpg_keyring(
    runner: &dyn Runner,
    files: &dyn Files,
    home: &Path,
    (ids, keyfiles): (&[String], &[String]),
) -> (Vec<(String, Vec<u8>)>, Vec<String>) {
    let mut failed = Vec::new();
    // An empty common.conf keeps gpg from switching to keyboxd.
    let _ = files.write(&home.join("common.conf"), b"");
    let import = |input: Input, file: Option<&str>| {
        let mut command = Command::new("gpg")
            .env("GNUPGHOME", home.as_os_str())
            .args(["--batch", "--import"])
            .stdin(input)
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        if let Some(file) = file {
            command = command.arg(file);
        }
        runner
            .run(&command)
            .is_ok_and(|output| output.status.is_success())
    };
    for id in ids {
        let exported = output(runner, &quiet("gpg", &["--batch", "--export", id]));
        let imported = exported
            .is_some_and(|key| !key.is_empty() && import(Input::Bytes(key.into_bytes()), None));
        if !imported {
            failed.push(format!(
                "unable to export GnuPG public key {id} from the build host"
            ));
        }
    }
    for keyfile in keyfiles {
        if !import(Input::Null, Some(keyfile)) {
            failed.push(format!("unable to import GnuPG public key file {keyfile}"));
        }
    }
    let kept = ["pubring.kbx", "pubring.gpg", "trustdb.gpg", "common.conf"]
        .into_iter()
        .filter_map(|name| Some((name.to_owned(), files.read(&home.join(name)).ok()?)))
        .collect();
    let _ = runner.run(&quiet("gpgconf", &["--kill", "all"]).env("GNUPGHOME", home.as_os_str()));
    (kept, failed)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_testkit::FakeRunner;
    use zbm_testkit::fakes::FakeFiles;

    use super::*;

    #[test]
    fn the_programs_answers_are_read_and_failures_are_nothing() {
        let runner = FakeRunner::new();
        runner.respond("fzf -f abc --raw --exit-0", 0, "abc\n");
        runner.respond("fzf -f abc --disabled --exit-0", 2, "");
        runner.respond("zfs version", 0, "zfs-2.2.6-1\nzfs-kmod-2.2.6-1\n");
        runner.respond("hostid", 0, "0a1b2c3d\n");
        runner.respond("gcc-config -c", 0, "x86_64-pc-linux-gnu-14\n");
        runner.respond("ldconfig -p", 0, "libc.so.6 => /lib/libc.so.6\n");
        runner.respond("ldd /sbin/zpool", 0, "libzfs.so.6 => /lib/libzfs.so.6\n");
        runner.respond(
            "gpgconf --list-components",
            0,
            "gpg-agent:Agent:/usr/bin/gpg-agent\n",
        );
        runner.respond(
            "gpg --batch --list-only --list-packets /k.gpg",
            2,
            ":pubkey enc packet:\n",
        );
        runner.respond("coreutils --list", 0, "cat\nls\n\n");
        runner.respond("/usr/lib/initcpio/busybox --list", 0, "sh\nls\n");

        assert!(fzf_supports(&runner, &["--raw"]));
        assert!(!fzf_supports(&runner, &["--disabled"]));
        assert_eq!(zfs_version(&runner).as_deref(), Some("2.2.6-1"));
        assert_eq!(hostid(&runner).as_deref(), Some("0a1b2c3d"));
        assert_eq!(
            gcc_profile(&runner).as_deref(),
            Some("x86_64-pc-linux-gnu-14")
        );
        assert!(linker_cache(&runner).contains("libc.so.6"));
        assert!(libraries_of(&runner, Path::new("/sbin/zpool")).contains("libzfs"));
        assert!(gpg_components(&runner).starts_with("gpg-agent"));
        assert!(gpg_packets(&runner, Path::new("/k.gpg")).contains("pubkey enc"));
        assert_eq!(
            coreutils_programs(&runner),
            Some(vec!["cat".to_owned(), "ls".to_owned()])
        );
        assert_eq!(busybox_programs(&runner), ["sh", "ls"]);

        let none = FakeRunner::new();
        none.respond_program("zfs", 1, "");
        none.respond_program("hostid", 0, "zero\n");
        none.respond_program("gcc-config", 0, "\n");
        none.respond_program("coreutils", 1, "");
        assert_eq!(zfs_version(&none), None);
        assert_eq!(hostid(&none), None);
        assert_eq!(gcc_profile(&none), None);
        assert_eq!(coreutils_programs(&none), None);
    }

    #[test]
    fn a_keyring_is_built_of_the_keys_that_could_be_had() {
        let runner = FakeRunner::new();
        runner.respond("gpg --batch --export ABCD", 0, "KEY");
        runner.respond("gpg --batch --export EF01", 0, "");
        runner.respond("gpg --batch --import /k.asc", 2, "");
        let files = FakeFiles::new();
        files.dir("/tmp/zbmgpg");
        files.file("/tmp/zbmgpg/pubring.kbx", "RING");

        let (kept, failed) = gpg_keyring(
            &runner,
            &files,
            Path::new("/tmp/zbmgpg"),
            (
                &["ABCD".to_owned(), "EF01".to_owned()],
                &["/k.asc".to_owned()],
            ),
        );

        let names: Vec<&str> = kept.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["pubring.kbx", "common.conf"]);
        assert_eq!(failed.len(), 2, "{failed:?}");
        assert!(failed[0].contains("EF01") && failed[1].contains("/k.asc"));
        assert!(
            runner
                .lines()
                .iter()
                .any(|line| line == "gpgconf --kill all")
        );
    }
}
