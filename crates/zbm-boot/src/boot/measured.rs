//! Measured boot (ROADMAP.md SE10, ADR 0008): before `kexec`, the kernel,
//! the initramfs and the command line of the boot go into PCRs 8 and 9 of
//! the TPM, in every active bank, then the separator `zbm-unsealed` into
//! PCR 9; each extension is recorded in the state directory's event log
//! ([`LOG`]). The hooks of the hook root go into PCR 9 before them, at
//! start-up ([`extend_events`], ADR 0009).
//!
//! The kernel loads copies of the files, in the state directory: the bytes
//! measured are the bytes loaded, whatever happens to the boot
//! environment's files after; and the initramfs's copy has the log in
//! front, in a cpio archive (`zbm_crypt::measure::cpio`), for the booted
//! system. Without a TPM nothing is measured, and the boot goes on;
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
use zbm_platform::{Context, Files};
use zbm_state::State;

use zbm_bootenv::kernels::{Kernel, boot_image};
use zbm_types::Printable;

use crate::load::KexecLoad;

/// The event log in the state directory, which the saved logs keep.
pub(crate) const LOG: &str = "tpm-measure.json";

/// The directory of the copies the kernel loads, in the state directory.
const COPIES: &str = "measured";

/// The TPM devices, the resource manager's first.
const DEVICES: [&str; 2] = ["/dev/tpmrm0", "/dev/tpm0"];

/// The files the kernel loads in place of the boot environment's: their
/// measured copies.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Copies {
    /// The kernel's copy.
    pub kernel: PathBuf,
    /// The initramfs's copy, behind the event log's archive.
    pub initramfs: PathBuf,
}

impl Copies {
    /// Removes the copies, once the kernel has loaded them or failed to.
    pub fn remove(&self, context: &Context<'_>) {
        let _ = context.files().remove_file(&self.kernel);
        let _ = context.files().remove_file(&self.initramfs);
    }
}

/// Whether the machine has a TPM that measured boot would extend.
pub fn has_tpm(context: &Context<'_>) -> bool {
    tpm_device(context).is_some()
}

/// The TPM of the machine, the resource manager's device first.
fn tpm_device(context: &Context<'_>) -> Option<&'static str> {
    DEVICES
        .into_iter()
        .find(|device| context.files().exists(Path::new(device)))
}

/// What became of events for the TPM.
#[non_exhaustive]
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
pub fn extend_events(context: &Context<'_>, state: &State<'_>, events: &[Event]) -> Extended {
    let Some(device) = tpm_device(context) else {
        return Extended::NoTpm;
    };
    let tcti = format!("device:{device}");
    let tpm = Tpm2::new(context.runner(), &tcti);
    let banks = tpm.active_banks().filter(|banks| !banks.is_empty());
    let Some(banks) = banks else {
        zbm_trace::warn!(
            "the PCR banks of the TPM {device} could not be read: nothing is measured";
            %device
        );
        return Extended::NoBanks;
    };
    let log = state.base().join(LOG);
    if !context.files().exists(&log) {
        record_start(context, &tpm, &banks, &log);
    }
    for event in events {
        extend(context, &tpm, (event, &banks), &log);
    }
    Extended::Extended
}

/// Measures what `load` loads into the TPM, if there is one, and records
/// it in the event log of `state`; the copies the kernel loads instead of
/// the boot environment's files, or `None` when nothing was measured, as
/// without a TPM. The files are read as they are opened (`load.files`),
/// and only when they fit in the memory available: otherwise they load
/// unmeasured, after the separator.
pub fn measure(context: &Context<'_>, state: &State<'_>, load: &KexecLoad<'_>) -> Option<Copies> {
    if tpm_device(context).is_none() {
        zbm_trace::debug!("no TPM: the boot is not measured");
        return None;
    }
    let files = read_files(context, load);
    let mut events = Vec::new();
    if let Some((kernel, initramfs)) = &files {
        events.push(kernel.0.clone());
        events.push(initramfs.0.clone());
        events.push(Event::command_line(&load.command_line()));
    }
    // What ZFSBootMenu unsealed, it unsealed: the separator, even when the
    // files could not be measured.
    events.push(Event::unsealed());
    if extend_events(context, state, &events) != Extended::Extended {
        return None;
    }
    let (kernel, initramfs) = files?;
    write_copies(
        context,
        state,
        &state.base().join(LOG),
        (kernel.1, initramfs.1),
    )
}

/// A measured file: its event, and its bytes.
type File = (Event, Vec<u8>);

/// The kernel's estimate of the memory available, in `/proc/meminfo`.
const MEMINFO: &str = "/proc/meminfo";

/// How many times the size of the kernel and the initramfs a measured boot
/// holds at its peak, in this process and the state's tmpfs: the two
/// files read, the initramfs again behind the log, and the two copies.
const PEAK: u64 = 3;

/// The digests and the bytes of the regular file `path`, of at most `len`
/// bytes, read once.
fn measured_file(files: &dyn Files, path: &str, len: u64) -> io::Result<(Digests, Vec<u8>)> {
    let bytes = files.read_regular(Path::new(path), len)?;
    Ok((Digests::of_bytes(&bytes), bytes))
}

/// The size of the regular file `path`, in bytes.
fn size(files: &dyn Files, path: &str) -> io::Result<u64> {
    Ok(files.metadata(Path::new(path))?.len)
}

/// `MemAvailable` of `/proc/meminfo`, in bytes, if it can be read.
fn available_memory(files: &dyn Files) -> Option<u64> {
    let text = files.read_to_string(Path::new(MEMINFO)).ok()?;
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))?;
    let kib = line.trim().strip_suffix("kB")?.trim_end();
    kib.parse::<u64>().ok()?.checked_mul(1024)
}

/// Whether `len` bytes of files can be measured: [`PEAK`] times them fit
/// in the memory available, or that memory is not known. Warns when they
/// do not fit.
fn fits_in_memory(files: &dyn Files, len: u64) -> bool {
    let Some(available) = available_memory(files) else {
        return true;
    };
    let needed = len.saturating_mul(PEAK);
    if needed <= available {
        return true;
    }
    zbm_trace::warn!(
        "the kernel and initramfs ({len} bytes) are too large to measure in the \
         {available} bytes of memory available: they load unmeasured";
        %len, %available
    );
    false
}

/// The kernel and the initramfs of `load`, as they are opened, read once
/// when they fit in memory, and their events.
fn read_files(context: &Context<'_>, load: &KexecLoad<'_>) -> Option<(File, File)> {
    let files = context.files();
    let (kernel, initramfs) = load.opened();
    let unread = |inside: &str, error: io::Error| {
        let path = format!("{}{inside}", load.mnt);
        zbm_trace::warn!(
            "unable to read {path} to measure it: {error}";
            path = %Printable(&path), %error
        );
    };
    let kernel_len = size(files, &kernel)
        .map_err(|error| unread(load.kernel, error))
        .ok()?;
    let initramfs_len = size(files, &initramfs)
        .map_err(|error| unread(load.initramfs, error))
        .ok()?;
    if !fits_in_memory(files, kernel_len.saturating_add(initramfs_len)) {
        return None;
    }
    let read = |(path, len): (&str, u64), inside: &str, measured: fn(String) -> Measured| {
        match measured_file(files, path, len) {
            Ok((digests, bytes)) => {
                let measured = measured(inside.to_owned());
                Some((Event { measured, digests }, bytes))
            }
            Err(error) => {
                unread(inside, error);
                None
            }
        }
    };
    let kernel = read((&kernel, kernel_len), load.kernel, Measured::Kernel)?;
    let initramfs = read(
        (&initramfs, initramfs_len),
        load.initramfs,
        Measured::Initramfs,
    )?;
    Some((kernel, initramfs))
}

/// The digests of the file `inside` the boot environment mounted at
/// `root`, found as the boot finds it, inside the boot environment.
fn predicted_file(files: &dyn Files, root: &Path, inside: &str) -> io::Result<Digests> {
    let path = boot_image(files, root, Path::new(inside)).map_err(|why| {
        let why = Printable(&why.to_string()).to_string();
        io::Error::new(io::ErrorKind::InvalidInput, why)
    })?;
    let (digests, _) = measured_file(files, &path, size(files, &path)?)?;
    Ok(digests)
}

/// The events a boot of `kernel`, from the boot environment mounted at
/// `root`, with the event of its command line, extends, in order: what
/// `zbm measure predict` replays. The files are found and read as the boot
/// finds and reads them.
///
/// # Errors
///
/// When the kernel or the initramfs cannot be found or read.
pub fn predicted(
    files: &dyn Files,
    root: &Path,
    kernel: &Kernel,
    command_line: Event,
) -> io::Result<Vec<Event>> {
    let kernel_digests = predicted_file(files, root, &kernel.kernel)?;
    let initramfs_digests = predicted_file(files, root, &kernel.initramfs)?;
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
fn record_start(context: &Context<'_>, tpm: &Tpm2<'_>, banks: &[Bank], log: &Path) {
    let Some(values) = tpm.read_pcrs(&[COMMAND_LINE_PCR, FILES_PCR], banks) else {
        zbm_trace::warn!("the PCRs could not be read: the log replays from zeros");
        return;
    };
    for (pcr, digests) in values {
        append(context, (&Event::start(pcr, digests), banks), log);
    }
}

/// Extends the PCR of `event` in `banks`, and appends its record to the
/// event log `log` when the TPM took it.
fn extend(context: &Context<'_>, tpm: &Tpm2<'_>, (event, banks): (&Event, &[Bank]), log: &Path) {
    let pcr = event.pcr();
    if let Err(error) = tpm.extend(pcr, &event.digests, banks) {
        zbm_trace::warn!(
            "PCR {pcr} was not extended ({error}): the log leaves it out";
            %pcr, %error
        );
        return;
    }
    append(context, (event, banks), log);
}

/// Appends the record of `event`, with its digests in `banks`, to the
/// event log `log`.
fn append(context: &Context<'_>, (event, banks): (&Event, &[Bank]), log: &Path) {
    let recorded = context
        .files()
        .read_to_string(log)
        .map_or(0, |text| text.matches('\u{1e}').count());
    let recnum = u64::try_from(recorded).unwrap_or(u64::MAX);
    let record = measure::record(recnum, event, banks);
    if let Err(error) = context.files().append(log, record.as_bytes()) {
        zbm_trace::warn!("unable to write the TPM event log: {error}"; %error);
    }
}

/// Writes the copies of the kernel and the initramfs in `state`, the
/// initramfs's behind an archive of the event log `log`: an uncompressed
/// archive before a compressed one is the layout every kernel unpacks
/// (that of early microcode), where one after it is lost when the kernel
/// cannot tell where the compressed archive ends.
fn write_copies(
    context: &Context<'_>,
    state: &State<'_>,
    log: &Path,
    (kernel, initramfs): (Vec<u8>, Vec<u8>),
) -> Option<Copies> {
    let dir = state.base().join(COPIES);
    let copies = Copies {
        kernel: dir.join("kernel"),
        initramfs: dir.join("initramfs"),
    };
    let text = context.files().read(log).unwrap_or_default();
    let mut loaded = measure::cpio(measure::LOG_PATH, &text);
    loaded.extend_from_slice(&initramfs);
    let written = context
        .files()
        .create_dir_all(&dir)
        .and_then(|()| context.files().write(&copies.kernel, &kernel))
        .and_then(|()| context.files().write(&copies.initramfs, &loaded));
    match written {
        Ok(()) => Some(copies),
        Err(error) => {
            zbm_trace::warn!(
                "unable to write the measured copies: {error}; the boot environment's files load";
                %error
            );
            copies.remove(context);
            None
        }
    }
}

#[cfg(test)]
mod tests;
