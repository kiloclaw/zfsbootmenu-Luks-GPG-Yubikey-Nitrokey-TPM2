//! Measured boot (ROADMAP.md SE10, ADR 0008): before `kexec`, the kernel,
//! the initramfs and the command line of the boot go into PCRs 8 and 9 of
//! the TPM, in every active bank, then the separator `zbm-unsealed` into
//! PCR 9; each extension is recorded in the state directory's event log
//! ([`LOG`]). The hooks of the hook root go into PCR 9 before them, at
//! start-up ([`extend_events`], ADR 0009).
//!
//! The kernel loads copies of the files, in the state directory: the bytes
//! measured are the bytes loaded, whatever happens to the boot
//! environment's files after; and the initramfs's copy has the log
//! appended, in a cpio archive (`zbm_crypt::measure::cpio`), for the
//! booted system. Without a TPM nothing is measured, and the boot goes on;
//! with one that refuses an extension, the boot goes on too, and the log
//! leaves the event out (measured boot is evidence for the booted system,
//! not a gate of ZFSBootMenu's).

use std::io;
use std::path::{Path, PathBuf};

pub use zbm_crypt::measure::{
    self, Bank, COMMAND_LINE_PCR, Digests, Event, FILES_PCR, Measured, hex, parse_log, replay,
    unsealed_at,
};
use zbm_crypt::tpm2::Tpm2;
use zbm_platform::{Files, System};
use zbm_state::State;

use crate::commandline::KexecLoad;
use crate::kernels::Kernel;

/// The event log in the state directory, which the saved logs keep.
pub const LOG: &str = "tpm-measure.json";

/// The directory of the copies the kernel loads, in the state directory.
const COPIES: &str = "measured";

/// The TPM devices, the resource manager's first.
const DEVICES: [&str; 2] = ["/dev/tpmrm0", "/dev/tpm0"];

/// The files the kernel loads in place of the boot environment's: their
/// measured copies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Copies {
    /// The kernel's copy.
    pub kernel: PathBuf,
    /// The initramfs's copy, with the event log appended.
    pub initramfs: PathBuf,
}

impl Copies {
    /// Removes the copies, once the kernel has loaded them or failed to.
    pub fn remove(&self, system: &System<'_>) {
        let _ = system.files.remove_file(&self.kernel);
        let _ = system.files.remove_file(&self.initramfs);
    }
}

/// Whether the machine has a TPM that measured boot would extend.
pub fn has_tpm(system: &System<'_>) -> bool {
    tpm_device(system).is_some()
}

/// The TPM of the machine, the resource manager's device first.
fn tpm_device(system: &System<'_>) -> Option<&'static str> {
    DEVICES
        .into_iter()
        .find(|device| system.files.exists(Path::new(device)))
}

/// What became of events for the TPM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Extended {
    /// There is no TPM: nothing was measured.
    NoTpm,
    /// The TPM did not say which banks it has: nothing was measured.
    NoBanks,
    /// The events went into the TPM's active banks, and those the TPM
    /// took into the log.
    Extended,
}

/// Extends `events`, in order, into the TPM, if there is one, in its
/// active banks, and records each that it took in the event log of
/// `state`, which starts with what PCRs 8 and 9 held when it is new: the
/// one way into the TPM of measured boot, for the hook root's hooks
/// (ADR 0009) and the boot's files.
pub fn extend_events(system: &System<'_>, state: &State<'_>, events: &[Event]) -> Extended {
    let Some(device) = tpm_device(system) else {
        return Extended::NoTpm;
    };
    let tcti = format!("device:{device}");
    let tpm = Tpm2::new(system.runner, &tcti);
    let banks = tpm.active_banks().filter(|banks| !banks.is_empty());
    let Some(banks) = banks else {
        zbm_trace::warn!(
            "the PCR banks of the TPM {device} could not be read: nothing is measured";
            %device
        );
        return Extended::NoBanks;
    };
    let log = state.base().join(LOG);
    if !system.files.exists(&log) {
        record_start(system, &tpm, &banks, &log);
    }
    for event in events {
        extend(system, &tpm, (event, &banks), &log);
    }
    Extended::Extended
}

/// Measures what `load` loads into the TPM, if there is one, and records
/// it in the event log of `state`; the copies the kernel loads instead of
/// the boot environment's files, or `None` when nothing was measured, as
/// without a TPM.
pub fn measure(system: &System<'_>, state: &State<'_>, load: &KexecLoad<'_>) -> Option<Copies> {
    if tpm_device(system).is_none() {
        zbm_trace::debug!("no TPM: the boot is not measured");
        return None;
    }
    let files = read_files(system, load);
    let mut events = Vec::new();
    if let Some((kernel, initramfs)) = &files {
        events.push(kernel.0.clone());
        events.push(initramfs.0.clone());
        events.push(Event::command_line(&load.command_line()));
    }
    // What ZFSBootMenu unsealed, it unsealed: the separator, even when the
    // files could not be measured.
    events.push(Event::unsealed());
    if extend_events(system, state, &events) != Extended::Extended {
        return None;
    }
    let (kernel, initramfs) = files?;
    write_copies(
        system,
        state,
        &state.base().join(LOG),
        (kernel.1, initramfs.1),
    )
}

/// A measured file: its event, and its bytes.
type File = (Event, Vec<u8>);

/// The digests and the bytes of the file `inside` the boot environment
/// mounted at `mnt`, read once.
fn measured_file(files: &dyn Files, mnt: &str, inside: &str) -> io::Result<(Digests, Vec<u8>)> {
    let bytes = files.read(&PathBuf::from(format!("{mnt}{inside}")))?;
    Ok((Digests::of_bytes(&bytes), bytes))
}

/// The kernel and the initramfs of `load`, read once, and their events.
fn read_files(system: &System<'_>, load: &KexecLoad<'_>) -> Option<(File, File)> {
    let read = |inside: &str, measured: fn(String) -> Measured| match measured_file(
        system.files,
        load.mnt,
        inside,
    ) {
        Ok((digests, bytes)) => {
            let measured = measured(inside.to_owned());
            Some((Event { measured, digests }, bytes))
        }
        Err(error) => {
            let path = format!("{}{inside}", load.mnt);
            zbm_trace::warn!("unable to read {path} to measure it: {error}"; %path, %error);
            None
        }
    };
    let kernel = read(load.kernel, Measured::Kernel)?;
    let initramfs = read(load.initramfs, Measured::Initramfs)?;
    Some((kernel, initramfs))
}

/// The events a boot of `kernel`, from the boot environment mounted at
/// `root`, with the event of its command line, extends, in order: what
/// `zbm measure predict` replays.
///
/// # Errors
///
/// When the kernel or the initramfs cannot be read.
pub fn predicted(
    files: &dyn Files,
    root: &Path,
    kernel: &Kernel,
    command_line: Event,
) -> io::Result<Vec<Event>> {
    let root = root.to_string_lossy();
    let mnt = root.strip_suffix('/').unwrap_or(&root);
    let (kernel_digests, _) = measured_file(files, mnt, &kernel.kernel)?;
    let (initramfs_digests, _) = measured_file(files, mnt, &kernel.initramfs)?;
    Ok(vec![
        Event {
            measured: Measured::Kernel(kernel.kernel.clone()),
            digests: kernel_digests,
        },
        Event {
            measured: Measured::Initramfs(kernel.initramfs.clone()),
            digests: initramfs_digests,
        },
        command_line,
        Event::unsealed(),
    ])
}

/// Starts the event log `log` with what PCRs 8 and 9 hold in `banks`
/// before the first extension, which may be more than zeros: firmware and
/// the kernel's EFI stub may have extended PCR 9.
fn record_start(system: &System<'_>, tpm: &Tpm2<'_>, banks: &[Bank], log: &Path) {
    let Some(values) = tpm.read_pcrs(&[COMMAND_LINE_PCR, FILES_PCR], banks) else {
        zbm_trace::warn!("the PCRs could not be read: the log replays from zeros");
        return;
    };
    for (pcr, digests) in values {
        append(system, (&Event::start(pcr, digests), banks), log);
    }
}

/// Extends the PCR of `event` in `banks`, and appends its record to the
/// event log `log` when the TPM took it.
fn extend(system: &System<'_>, tpm: &Tpm2<'_>, (event, banks): (&Event, &[Bank]), log: &Path) {
    let pcr = event.pcr();
    if let Err(error) = tpm.extend(pcr, &event.digests, banks) {
        zbm_trace::warn!(
            "PCR {pcr} was not extended ({error}): the log leaves it out";
            %pcr, %error
        );
        return;
    }
    append(system, (event, banks), log);
}

/// Appends the record of `event`, with its digests in `banks`, to the
/// event log `log`.
fn append(system: &System<'_>, (event, banks): (&Event, &[Bank]), log: &Path) {
    let recorded = system
        .files
        .read_to_string(log)
        .map_or(0, |text| text.matches('\u{1e}').count());
    let recnum = u64::try_from(recorded).unwrap_or(u64::MAX);
    let record = measure::record(recnum, event, banks);
    if let Err(error) = system.files.append(log, record.as_bytes()) {
        zbm_trace::warn!("unable to write the TPM event log: {error}"; %error);
    }
}

/// Writes the copies of the kernel and the initramfs in `state`, the
/// event log `log` appended to the initramfs's.
fn write_copies(
    system: &System<'_>,
    state: &State<'_>,
    log: &Path,
    (kernel, mut initramfs): (Vec<u8>, Vec<u8>),
) -> Option<Copies> {
    let dir = state.base().join(COPIES);
    let copies = Copies {
        kernel: dir.join("kernel"),
        initramfs: dir.join("initramfs"),
    };
    let text = system.files.read(log).unwrap_or_default();
    initramfs.extend_from_slice(&measure::cpio(measure::LOG_PATH, &text));
    let written = system
        .files
        .create_dir_all(&dir)
        .and_then(|()| system.files.write(&copies.kernel, &kernel))
        .and_then(|()| system.files.write(&copies.initramfs, &initramfs));
    match written {
        Ok(()) => Some(copies),
        Err(error) => {
            zbm_trace::warn!(
                "unable to write the measured copies: {error}; the boot environment's files load";
                %error
            );
            copies.remove(system);
            None
        }
    }
}

#[cfg(test)]
mod tests;
