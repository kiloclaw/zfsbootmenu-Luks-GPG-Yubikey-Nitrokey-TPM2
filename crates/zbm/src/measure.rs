//! `zbm measure`: measured boot seen from the booted system (ROADMAP.md
//! SE10, ADR 0008).
//!
//! - `zbm measure log [FILE]` reads the event log ZFSBootMenu left
//!   (`/run/log/zfsbootmenu/tpm-measure.json` by default), prints its
//!   events, and replays them into the values PCRs 8 and 9 must hold;
//! - `zbm measure predict DATASET [KERNEL]` prints the values a boot of the
//!   boot environment `DATASET` would leave, with the kernel ZFSBootMenu
//!   picks (or `KERNEL`, a path inside it), from the files on the dataset,
//!   which must be mounted (or given with `--root`), and the command line
//!   of this boot's log (or `--command-line`), from what the PCRs held
//!   before ZFSBootMenu extended them in this boot, as its log says: what
//!   the firmware and ZFSBootMenu's own kernel put there, which a boot of
//!   the same ZFSBootMenu image on the same firmware puts there again.
//!
//! Values print as `PCR:BANK=HEX`, which `systemd-cryptenroll --tpm2-pcrs`
//! takes.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use zbm_boot::boot::measured::{
    Bank, COMMAND_LINE_PCR, Event, FILES_PCR, Measured, hex, parse_log, predicted, replay,
};
use zbm_boot::kernels::{self, Kernel};
use zbm_platform::{NoConsole, RealSystem, System};
use zbm_types::{FilesystemName, KernelSelector};

use crate::{Io, USAGE};

/// Where the booted system finds the log: the boot environment's
/// initramfs copies it there from `/.extra`.
pub const LOG: &str = "/run/log/zfsbootmenu/tpm-measure.json";

/// The mount table.
const MOUNTS: &str = "/proc/self/mounts";

/// The usage text.
const USAGE_TEXT: &str = "\
Usage: zbm measure log [FILE]       replay the event log of this boot
       zbm measure predict [OPTIONS] DATASET [KERNEL]
                                    the PCR values a boot of DATASET leaves
Options of predict:
       --root DIR           where DATASET is mounted, if not in the mount table
       --command-line TEXT  the command line, if not that of this boot's log
       --bank NAME          sha1, sha256 (the default), sha384 or sha512
";

/// `zbm measure`.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let real = RealSystem::new();
    run_in(&real.with_console(&NoConsole), args, io)
}

/// `zbm measure` on `system`.
pub fn run_in(system: &System<'_>, args: &[String], io: &mut Io) -> i32 {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["log"] => log(system, Path::new(LOG)),
        ["log", file] if !file.starts_with('-') => log(system, Path::new(file)),
        ["predict", rest @ ..] => Predict::parse(rest).map_or_else(
            || {
                let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
                Err(String::new())
            },
            |predict| predict.run(system),
        ),
        ["-h" | "--help"] => Ok(USAGE_TEXT.to_owned()),
        _ => {
            let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
            return USAGE;
        }
    };
    match result {
        Ok(text) => {
            let _ = io.stdout.write_all(text.as_bytes());
            0
        }
        Err(message) if message.is_empty() => USAGE,
        Err(message) => {
            let _ = writeln!(io.stderr, "zbm measure: {message}");
            1
        }
    }
}

/// The values of PCRs 8 and 9 in `bank` after `events`.
fn values(events: &[Event], bank: Bank) -> String {
    let mut text = String::new();
    for pcr in [COMMAND_LINE_PCR, FILES_PCR] {
        let value = hex(&replay(events, pcr, bank));
        let _ = writeln!(text, "{pcr}:{}={value}", bank.name());
    }
    text
}

/// `zbm measure log FILE`: the events of the log, then the values of the
/// PCRs in each bank every event has a digest for.
fn log(system: &System<'_>, file: &Path) -> Result<String, String> {
    let text = system
        .files
        .read_to_string(file)
        .map_err(|error| format!("unable to read {}: {error}", file.display()))?;
    let events = parse_log(&text)
        .map_err(|index| format!("record {index} of {} is none of ours", file.display()))?;
    let mut out = String::new();
    for (index, event) in events.iter().enumerate() {
        let (kind, value) = (event.measured.kind(), event.measured.value());
        let _ = writeln!(out, "{index:>3}  PCR {}  {kind:<12}  {value}", event.pcr());
    }
    out.push('\n');
    for bank in Bank::ALL {
        if !events.is_empty() && events.iter().all(|event| event.digests.get(bank).is_some()) {
            out.push_str(&values(&events, bank));
        }
    }
    Ok(out)
}

/// What `zbm measure predict` predicts.
#[derive(Debug, Default, Eq, PartialEq)]
struct Predict {
    dataset: String,
    kernel: Option<String>,
    root: Option<PathBuf>,
    command_line: Option<String>,
    bank: Option<Bank>,
}

impl Predict {
    /// Its arguments, or `None` when they are not that.
    fn parse(args: &[&str]) -> Option<Predict> {
        let mut predict = Predict::default();
        let mut positional = Vec::new();
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--root" => predict.root = Some(PathBuf::from(args.next()?)),
                "--command-line" => predict.command_line = Some((*args.next()?).to_owned()),
                "--bank" => predict.bank = Some(Bank::named(args.next()?)?),
                _ if arg.starts_with('-') => return None,
                _ => positional.push(arg),
            }
        }
        let (dataset, kernel) = match positional.as_slice() {
            [dataset] => (dataset, None),
            [dataset, kernel] => (dataset, Some((*kernel).to_owned())),
            _ => return None,
        };
        Some(Predict {
            dataset: (*dataset).to_owned(),
            kernel,
            ..predict
        })
    }

    /// The prediction: the files, the command line and the values.
    fn run(&self, system: &System<'_>) -> Result<String, String> {
        let dataset: FilesystemName = self
            .dataset
            .parse()
            .map_err(|_| format!("{} is no dataset", self.dataset))?;
        let root = match &self.root {
            Some(root) => root.clone(),
            None => mount_point(system, &self.dataset).ok_or_else(|| {
                format!("{} is not mounted: mount it, or give --root", self.dataset)
            })?,
        };
        let kernel = self.kernel(system, &dataset, &root)?;
        let booted = booted(system);
        let command_line = match &self.command_line {
            Some(line) => line.clone(),
            None => booted_command_line(&booted)?,
        };
        let mut events: Vec<Event> = booted
            .into_iter()
            .filter(|event| matches!(event.measured, Measured::Start(_)))
            .collect();
        let start = if events.is_empty() {
            "zeros (no start in this boot's log)"
        } else {
            "the values in this boot's log"
        };
        let boot = predicted(
            system.files,
            &root,
            &kernel,
            Event::command_line(&command_line),
        )
        .map_err(|error| format!("unable to read the kernel or its initramfs: {error}"))?;
        events.extend(boot);
        Ok(format!(
            "kernel        {}\ninitramfs     {}\ncommand line  {command_line}\nstart         \
             {start}\n\n{}",
            kernel.kernel,
            kernel.initramfs,
            values(&events, self.bank.unwrap_or(Bank::Sha256)),
        ))
    }

    /// The kernel ZFSBootMenu boots from the boot environment at `root`:
    /// the one given, or the one `org.zfsbootmenu:kernel` or the version
    /// order picks.
    fn kernel(
        &self,
        system: &System<'_>,
        dataset: &FilesystemName,
        root: &Path,
    ) -> Result<Kernel, String> {
        let found = kernels::discover(system.files, &self.dataset, root).kernels;
        let chosen = if let Some(path) = &self.kernel {
            found.iter().find(|kernel| kernel.kernel == *path)
        } else {
            let property = zbm_boot::boot::kernel_property(system, dataset);
            let selector = property.and_then(|text| text.parse::<KernelSelector>().ok());
            kernels::select(&found, selector.as_ref())
        };
        chosen.cloned().ok_or_else(|| match &self.kernel {
            Some(path) => format!("{path} is no kernel with an initramfs on {}", self.dataset),
            None => format!("{} has no kernel with an initramfs", self.dataset),
        })
    }
}

/// Where the mount table says `dataset` is mounted.
fn mount_point(system: &System<'_>, dataset: &str) -> Option<PathBuf> {
    let table = system.files.read_to_string(Path::new(MOUNTS)).ok()?;
    table.lines().find_map(|line| {
        let mut fields = line.split(' ');
        let (source, target, fstype) = (fields.next()?, fields.next()?, fields.next()?);
        (source == dataset && fstype == "zfs").then(|| PathBuf::from(target.replace("\\040", " ")))
    })
}

/// The events of this boot's log, if it has one ZFSBootMenu wrote.
fn booted(system: &System<'_>) -> Vec<Event> {
    let text = system
        .files
        .read_to_string(Path::new(LOG))
        .unwrap_or_default();
    parse_log(&text).unwrap_or_default()
}

/// The command line of this boot's log `events`: the last one measured.
fn booted_command_line(events: &[Event]) -> Result<String, String> {
    events
        .iter()
        .rev()
        .find_map(|event| match &event.measured {
            Measured::CommandLine(line) => Some(line.clone()),
            _ => None,
        })
        .ok_or_else(|| format!("{LOG} names no command line: give --command-line"))
}

#[cfg(test)]
mod tests;
