//! `runtime-record-zfs`: not a test. Runs the commands of `cargo xtask
//! record-zfs --manifest` on a fresh disk and saves their output as the
//! fixtures of `zbm-zfs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::expect::regex;
use crate::qemu::{Disk, Machine};
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::{Sh, Vm};

/// The commands `cargo xtask record-zfs --manifest` lists.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Manifest {
    /// The commands that build the pool.
    setup: Vec<String>,
    /// The recorded commands: (file name, command).
    records: Vec<(String, String)>,
    /// The commands that clean up.
    teardown: Vec<String>,
}

impl Manifest {
    /// The manifest in its JSON text.
    fn parse(text: &str) -> Result<Manifest> {
        let bad = || Error::Setup(format!("bad record-zfs manifest: {}", text.trim()));
        let value: serde_json::Value = serde_json::from_str(text).map_err(|_| bad())?;
        let strings = |key: &str| -> Option<Vec<String>> {
            value[key]
                .as_array()?
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect()
        };
        let records = value["records"]
            .as_array()
            .ok_or_else(bad)?
            .iter()
            .map(|record| {
                let file = record["file"].as_str()?.to_owned();
                Some((file, record["command"].as_str()?.to_owned()))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(bad)?;
        Ok(Manifest {
            setup: strings("setup").ok_or_else(bad)?,
            records,
            teardown: strings("teardown").ok_or_else(bad)?,
        })
    }
}

/// Records the fixtures.
///
/// # Errors
///
/// When a step cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let json = crate::disks::run(
        Command::new("cargo")
            .args(["xtask", "record-zfs", "--manifest"])
            .current_dir(&settings.repo),
    )?;
    let manifest = Manifest::parse(&json)?;
    let image = super::image(settings, "auto", ZbmRuntime::Legacy, None)?;
    let disk = crate::disks::disk(settings, "record-zfs", 0, "384M", true)?;
    let machine = Machine::boot(&image, super::SHELL_APPEND).with_disks([Disk::new(disk, "d0")]);
    let mut vm = Vm::start(test, "record", &machine)?;
    vm.recovery_shell()?;
    for command in &manifest.setup {
        vm.must(command)?;
    }
    let outputs = record(&mut vm, &manifest.records)?;
    for command in &manifest.teardown {
        vm.sh(command)?;
    }
    vm.poweroff()?;
    write_fixtures(test, &settings.repo, &outputs)
}

/// Runs each recorded command; returns what it printed, by file name.
fn record(vm: &mut Vm<'_>, records: &[(String, String)]) -> Result<Vec<(String, Vec<u8>)>> {
    let mut outputs: Vec<(String, Vec<u8>)> = Vec::new();
    for (file, command) in records {
        // Hexadecimal survives the serial console byte for byte.
        let (_, out) = vm.sh_with(
            &format!("( {command} ) | od -An -v -tx1"),
            Sh::default().quiet(),
        )?;
        let data = from_hex(&out)
            .ok_or_else(|| Error::Setup(format!("{file}: not hexadecimal: {out}")))?;
        vm.test()
            .check(!data.is_empty(), &format!("recorded {file}"));
        match outputs.iter_mut().find(|(known, _)| known == file) {
            Some(entry) => entry.1 = data,
            None => outputs.push((file.clone(), data)),
        }
    }
    Ok(outputs)
}

/// The bytes written in hexadecimal in `text`, ignoring white space.
pub(super) fn from_hex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_digit(16).and_then(|digit| u8::try_from(digit).ok()))
        .collect::<Option<_>>()?;
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    Some(
        digits
            .chunks(2)
            .map(|pair| pair[0] << 4 | pair[1])
            .collect(),
    )
}

/// The fixture directory of the ZFS release `zfs version` names.
fn fixture_dir(repo: &Path, outputs: &[(String, Vec<u8>)]) -> Result<Option<PathBuf>> {
    let version = outputs
        .iter()
        .find(|(file, _)| file == "version.txt")
        .map(|(_, data)| String::from_utf8_lossy(data).into_owned())
        .unwrap_or_default();
    let release = regex(r"zfs-(\d+\.\d+)")?
        .captures(&version)
        .map(|captures| captures[1].to_owned());
    Ok(release.map(|release| repo.join("crates/zbm-zfs/tests/fixtures").join(release)))
}

/// Writes the fixtures into the directory of the release.
fn write_fixtures(test: &Test, repo: &Path, outputs: &[(String, Vec<u8>)]) -> Result<()> {
    let target = fixture_dir(repo, outputs)?;
    test.check(target.is_some(), "zfs version names the release");
    let Some(target) = target else {
        return Ok(());
    };
    let io = |what: &Path| Error::io(format!("writing {}", what.display()));
    fs::create_dir_all(&target).map_err(io(&target))?;
    for (name, data) in outputs {
        let path = target.join(name);
        fs::write(&path, data).map_err(io(&path))?;
    }
    test.note(&format!("fixtures written to {}", target.display()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_manifest_lists_setup_records_and_teardown() {
        let text = r#"{"setup":["zpool create x"],"records":[{"file":"version.txt",
            "command":"zfs version"}],"teardown":["zpool export x"]}"#;

        let manifest = Manifest::parse(text).unwrap();

        assert_eq!(manifest.setup, ["zpool create x"]);
        assert_eq!(
            manifest.records,
            [("version.txt".to_owned(), "zfs version".to_owned())]
        );
        assert_eq!(manifest.teardown, ["zpool export x"]);
        assert!(Manifest::parse(r#"{"setup":[],"records":[{}],"teardown":[]}"#).is_err());
        assert!(Manifest::parse("not json").is_err());
    }

    #[test]
    fn hexadecimal_output_becomes_bytes() {
        assert_eq!(from_hex(" 7a 66\n 73 0a\n"), Some(b"zfs\n".to_vec()));
        assert_eq!(from_hex(""), Some(Vec::new()));
        assert_eq!(from_hex("7a 6"), None);
        assert_eq!(from_hex("zz"), None);
    }

    #[test]
    fn the_fixtures_go_to_the_directory_of_the_release() {
        let outputs = vec![(
            "version.txt".to_owned(),
            b"zfs-2.4.4-1\nzfs-kmod-2.4.4-1\n".to_vec(),
        )];

        let dir = fixture_dir(Path::new("/r"), &outputs).unwrap();

        assert_eq!(
            dir,
            Some(PathBuf::from("/r/crates/zbm-zfs/tests/fixtures/2.4"))
        );
        assert_eq!(fixture_dir(Path::new("/r"), &[]).unwrap(), None);
    }
}
