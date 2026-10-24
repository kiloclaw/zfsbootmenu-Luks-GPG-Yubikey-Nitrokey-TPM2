//! Saving a command line: into a boot environment's property, or into an EFI
//! image's `.cmdline` section.

use std::path::Path;

use zbm_config::spec::properties::COMMANDLINE;
use zbm_core::fs::{create_temp_file, is_readable};
use zbm_platform::exec::Output;
use zbm_tools::efi as efi_tools;
use zbm_zfs::ZfsCommand;

use crate::{Context, Failure, TempFile, efi};

/// Sets the property of `dataset` to the command line `assembled`, or removes
/// it when the command line is empty.
///
/// # Errors
///
/// The message to print when `zfs` fails.
pub(crate) fn to_dataset(zfs: &ZfsCommand, assembled: &str, dataset: &str) -> Result<(), Failure> {
    // `kcl="$(cat)"` drops trailing newlines and NUL bytes.
    let kcl = assembled.trim_end_matches('\n').replace('\0', "");
    if kcl.is_empty() {
        if !zfs.inherit(COMMANDLINE.name, dataset).unwrap_or(false) {
            return Err(Failure::of(format!(
                "failed to clear KCL property on {dataset}"
            )));
        }
    } else if !zfs.set(COMMANDLINE.name, &kcl, dataset).unwrap_or(false) {
        return Err(Failure::of(format!(
            "failed to set KCL property on {dataset}"
        )));
    }
    Ok(())
}

/// Where the `.cmdline` section of `image` is and how much space follows it.
fn gap(context: &Context, image: &Path) -> Result<(String, i64), Failure> {
    let image_name = image.display().to_string();
    let found = if is_readable(image) {
        match efi_tools::section_headers(context.runner, image) {
            Ok(output) if output.status.is_success() => {
                efi::cmdline_gap(&output.stdout_text(), &image_name)
            }
            _ => Err(vec![
                "failed to parse object file; is objdump available?".to_owned(),
            ]),
        }
    } else {
        Err(vec![format!("unable to read object file '{image_name}'")])
    };
    found.map_err(|mut messages| {
        messages.push("failed to determine offset data for KCL".to_owned());
        Failure(messages)
    })
}

/// Replaces the `.cmdline` section of `image` with the command line
/// `assembled` and a NUL byte, at the same address, if it fits in the space
/// before the next section.
///
/// # Errors
///
/// The messages to print, in order, when a step fails.
pub(crate) fn to_image(context: &Context, assembled: &str, image: &Path) -> Result<(), Failure> {
    let (offset, gap) = gap(context, image)?;
    let failed_temp = || Failure::of("failed to save temporary KCL".to_owned());
    let section =
        create_temp_file(&context.temp_dir, "tmp.XXXXXXXXXX").map_err(|_| failed_temp())?;
    let section = TempFile(section);
    let mut contents = assembled.as_bytes().to_vec();
    contents.push(0);
    std::fs::write(&section.0, &contents).map_err(|_| failed_temp())?;
    if !efi::fits(contents.len(), gap) {
        return Err(Failure::of(format!(
            "new KCL size exceeds space available in EFI file '{}'; aborting",
            image.display()
        )));
    }

    let succeeded =
        |run: std::io::Result<Output>| run.is_ok_and(|output| output.status.is_success());
    if !succeeded(efi_tools::remove_cmdline(context.runner, image)) {
        return Err(Failure::of(
            "failed to clear existing KCL from EFI executable".to_owned(),
        ));
    }
    if !succeeded(efi_tools::add_cmdline(
        context.runner,
        image,
        &section.0,
        &offset,
    )) {
        return Err(Failure::of(
            "failed to write new KCL to EFI executable".to_owned(),
        ));
    }
    Ok(())
}
