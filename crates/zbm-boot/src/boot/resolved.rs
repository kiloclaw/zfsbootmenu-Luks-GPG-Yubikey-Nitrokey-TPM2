//! The files the boot loads, found as the kernel search finds them: inside
//! the boot environment's root, with no link left on their paths, and only
//! regular files of bounded size with UTF-8 paths (threat model I5). A
//! kernel or initramfs that is not found that way is not loaded: the boot
//! fails as a load that fails does, after PCR 9's separator.

use std::path::{Path, PathBuf};

use zbm_config::spec::properties::DEVICETREE;
use zbm_platform::Context;
use zbm_platform::interaction::Interaction;
use zbm_state::State;
use zbm_types::{FilesystemName, Printable};
use zbm_zfs::{Stderr, ZfsText};

use zbm_bootenv::commandline;
use zbm_bootenv::kernels::{Refused, boot_image};

use super::measured::{self, Event};
use super::sequence::failed_load;
use crate::load::KexecLoad;

/// The kernel and initramfs of `load`, of the boot environment mounted at
/// `mnt`, as they are opened: full paths with no link on them. The name of
/// the first that is refused, and why, otherwise.
pub(super) fn resolved<'n>(
    context: &Context<'_>,
    load: &KexecLoad<'n>,
    mnt: &Path,
) -> Result<(String, String), (&'n str, Refused)> {
    let image = |inside: &'n str| {
        boot_image(context.files(), mnt, Path::new(inside)).map_err(|why| (inside, why))
    };
    Ok((image(load.kernel)?, image(load.initramfs)?))
}

/// Fails the boot of `load`, whose file `inside` of the boot environment
/// mounted at `mnt` is refused for `why`: logs why, extends PCR 9's
/// separator, as after any boot attempt that failed (ADR 0008, point 4),
/// then unmounts the boot environment and tells the user through
/// `interaction`, as a load that fails does.
pub(super) fn refused(
    context: &Context<'_>,
    (interaction, state): (&dyn Interaction, &State<'_>),
    load: &KexecLoad<'_>,
    mnt: &Path,
    (inside, why): (&str, &Refused),
) {
    let path = format!("{}{inside}", load.mnt);
    zbm_trace::error!(
        "unable to load {path}: {why}";
        path = %Printable(&path), why = %Printable(&why.to_string())
    );
    let _ = measured::extend_events(context, state, &[Event::unsealed()]);
    failed_load(context, interaction, load, mnt);
}

/// The device tree that `org.zfsbootmenu:devicetree` of `fs` names for
/// `kernel` in the boot environment mounted at `mnt`, as it is loaded: a
/// full path with no link on it, if the property is set and the file is
/// found inside the boot environment. The log names it as the boot
/// environment does, below `mnt`.
pub(super) fn find_devicetree(
    context: &Context<'_>,
    fs: &FilesystemName,
    kernel: &str,
    mnt: &Path,
) -> Option<PathBuf> {
    let dtb_property = ZfsText::new(context.runner()).value(fs, DEVICETREE.name, Stderr::Hidden);
    let Some(property) = dtb_property.filter(|property| !property.is_empty() && property != "-")
    else {
        zbm_trace::debug!("no devicetree property set");
        return None;
    };
    zbm_trace::debug!(
        "devicetree property set to '{property}'"; property = %Printable(&property)
    );
    let inside = commandline::devicetree_candidate(&property, kernel)?;
    let candidate = format!("{}{}", mnt.display(), inside.display());
    match boot_image(context.files(), mnt, &inside) {
        Ok(path) => {
            zbm_trace::debug!(
                "devicetree file exists: '{candidate}'"; candidate = %Printable(&candidate)
            );
            Some(PathBuf::from(path))
        }
        Err(why) => {
            if !why.is_absent() {
                zbm_trace::warn!(
                    "the device tree {candidate} is not read: {why}";
                    candidate = %Printable(&candidate), why = %Printable(&why.to_string())
                );
            }
            zbm_trace::debug!(
                "devicetree file missing: '{candidate}'"; candidate = %Printable(&candidate)
            );
            None
        }
    }
}
