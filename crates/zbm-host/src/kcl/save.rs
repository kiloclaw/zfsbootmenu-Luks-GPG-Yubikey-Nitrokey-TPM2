//! Saving a command line: into a boot environment's property, or into an EFI
//! image's `.cmdline` section.

use std::path::Path;

use zbm_config::spec::properties::COMMANDLINE;
use zbm_types::DatasetName;
use zbm_zfs::ZfsText;

use crate::kcl::{Failure, Io};
use crate::pe::{Image, WriteOptions};

/// Sets the property of `dataset` to the command line `assembled`, or removes
/// it when the command line is empty.
///
/// # Errors
///
/// The message to print when `zfs` fails.
pub(crate) fn to_dataset(
    zfs: &ZfsText,
    assembled: &str,
    dataset: &DatasetName,
) -> Result<(), Failure> {
    // `kcl="$(cat)"` drops trailing newlines and NUL bytes.
    let kcl = assembled.trim_end_matches('\n').replace('\0', "");
    if kcl.is_empty() {
        if !zfs.inherit(dataset, COMMANDLINE.name).unwrap_or(false) {
            return Err(Failure::of(format!(
                "failed to clear KCL property on {dataset}"
            )));
        }
    } else if !zfs.set(dataset, COMMANDLINE.name, &kcl).unwrap_or(false) {
        return Err(Failure::of(format!(
            "failed to set KCL property on {dataset}"
        )));
    }
    Ok(())
}

/// The sections a new `.cmdline` goes before, in the order of a bundle.
const AFTER_CMDLINE: &[&str] = &[".splash", ".dtb", ".uname", ".initrd", ".linux"];

/// Gives `image` the command line `assembled` and a NUL byte as its
/// `.cmdline` section, or removes the section when the line is empty
/// (ADR 0014, point 6): at its address when it fits before the next
/// section, else with the sections after it laid out again; before the
/// kernel and the initramfs for an image without one. The image keeps its
/// time stamp.
///
/// # Errors
///
/// The messages to print, in order, when the image cannot be read, changed
/// or written.
pub(crate) fn to_image(io: &mut Io, assembled: &str, image: &Path) -> Result<(), Failure> {
    let name = image.display();
    let bytes = std::fs::read(image)
        .map_err(|_| Failure::of(format!("unable to read object file '{name}'")))?;
    let mut read = Image::read(bytes).map_err(|failure| {
        Failure::of(format!("unable to read EFI executable '{name}': {failure}"))
    })?;
    let changed = if assembled.is_empty() {
        read.remove(".cmdline");
        Ok(())
    } else {
        let mut contents = assembled.as_bytes().to_vec();
        contents.push(0);
        match read.replace(".cmdline", &contents) {
            Ok(true) => Ok(()),
            Ok(false) => read.insert_before(".cmdline", &contents, AFTER_CMDLINE),
            Err(failure) => Err(failure),
        }
    };
    let written = changed
        .and_then(|()| read.write(&WriteOptions::default()))
        .map_err(|failure| {
            Failure::of(format!(
                "unable to change EFI executable '{name}': {failure}"
            ))
        })?;
    for warning in &written.warnings {
        let _ = writeln!(io.stderr, "WARNING: {warning}");
    }
    std::fs::write(image, &written.bytes)
        .map_err(|_| Failure::of("failed to write new KCL to EFI executable".to_owned()))
}
