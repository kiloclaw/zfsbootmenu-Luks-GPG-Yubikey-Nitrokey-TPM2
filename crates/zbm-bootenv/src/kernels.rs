//! Kernels and initramfs images of a boot environment.
//!
//! The kernels are the files `/boot/{vmlinux,vmlinuz,linux,linuz,kernel}` and
//! `/boot/<name>-*` of the mounted boot environment, in the order `sort -V`
//! gives their paths, each with the first initramfs `find_initramfs` finds;
//! kernels without an initramfs are left out. The default kernel is the last
//! one, or the last one whose path matches the regular expression in
//! `org.zfsbootmenu:kernel`.
//!
//! Every file is looked up inside the boot environment ([`boot_image`]):
//! `/boot`, the kernels, their initramfs images and command line files are
//! resolved in its root, never in ZFSBootMenu's namespace, and only regular
//! files of bounded size count. A name with a control character is no
//! kernel, and a `/boot` gives at most the 64 newest. A file that exists and
//! is refused is passed over, as a missing one is, with one warning for its
//! kernel.
//!
//! The order is that of uutils' `sort -V` ([`version_sort`], in
//! `zbm_types::version`, from `uucore`), the coreutils of ZFSBootMenu's
//! images; see "Version order" in `crates/DESIGN.md` for how GNU coreutils
//! and BusyBox differ.

use std::path::{Path, PathBuf};

use zbm_platform::files::{FileKind, Files};
use zbm_tools::mount::inside;
pub(crate) use zbm_types::version::version_sort;
use zbm_types::{KernelSelector, Printable};

mod resolved;

pub(crate) use resolved::{MAX_KERNELS, NAME_MAX, Refusals, SMALL_LIMIT, refused, small_file};
pub use resolved::{Refused, boot_image};

/// The kernel file names looked for in `/boot`, each also followed by `-` and
/// anything.
const KERNEL_NAMES: [&str; 5] = ["vmlinux", "vmlinuz", "linux", "linuz", "kernel"];

/// A kernel of a boot environment, with paths relative to its root, as a line
/// of `environments/<dataset>/kernels` holds it.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Kernel {
    /// The boot environment.
    pub dataset: String,
    /// The kernel, such as `/boot/vmlinuz-6.12.9_1`.
    pub kernel: String,
    /// Its initramfs, such as `/boot/initramfs-6.12.9_1.img`.
    pub initramfs: String,
}

impl Kernel {
    /// The kernel `kernel` of `dataset`, with its initramfs `initramfs`.
    pub fn new(dataset: &str, kernel: &str, initramfs: &str) -> Kernel {
        Kernel {
            dataset: dataset.to_owned(),
            kernel: kernel.to_owned(),
            initramfs: initramfs.to_owned(),
        }
    }

    /// The line of the kernel list: the three fields separated by tabs.
    pub fn line(&self) -> String {
        format!("{}\t{}\t{}\n", self.dataset, self.kernel, self.initramfs)
    }

    /// A kernel from a line of the kernel list, read as `IFS=$'\t' read -r fs
    /// kernel initramfs` reads it: the third field takes the rest of the line.
    pub fn parse(line: &str) -> Kernel {
        let line = line.strip_suffix('\n').unwrap_or(line);
        let mut fields = line.splitn(3, '\t');
        let mut next = || fields.next().unwrap_or_default().to_owned();
        Kernel {
            dataset: next(),
            kernel: next(),
            initramfs: next(),
        }
    }
}

/// The names in `/boot` of a boot environment mounted at `mnt` that name
/// kernels, as the environment names them (`/boot/<name>`), in version
/// order: `vmlinux`, `vmlinuz`, `linux`, `linuz` or `kernel`, alone or
/// followed by `-` and anything, UTF-8 and with no control character.
/// `/boot` is resolved inside the environment; without it, there are
/// none.
fn kernel_names(files: &dyn Files, dataset: &str, mnt: &Path) -> Vec<String> {
    let boot = inside::resolve(files, mnt, Path::new("/boot"))
        .map_err(refused)
        .and_then(|found| {
            let boot = found.path.under(mnt);
            match files.metadata(&boot) {
                Ok(metadata) if metadata.kind != FileKind::Directory => Err(
                    Refused::NotADirectory(found.path.in_chroot(), metadata.kind),
                ),
                _ => Ok(boot),
            }
        });
    let boot = match boot {
        Ok(boot) => boot,
        Err(why) => {
            if !why.is_absent() {
                let why = why.to_string();
                zbm_trace::warn!(
                    "/boot of {dataset} is not read: {why}";
                    dataset = %Printable(dataset), why = %Printable(&why)
                );
            }
            return Vec::new();
        }
    };
    let names = files.read_dir(&boot).unwrap_or_default();
    let (mut found, mut controlled) = (Vec::new(), Vec::new());
    for name in names.iter().filter_map(|name| name.to_str()) {
        let is_kernel = KERNEL_NAMES.iter().any(|base| {
            name == *base
                || name
                    .strip_prefix(base)
                    .is_some_and(|rest| rest.starts_with('-'))
        });
        if is_kernel && name.contains(char::is_control) {
            controlled.push(name);
        } else if is_kernel {
            found.push(format!("/boot/{name}"));
        }
    }
    if let Some(first) = controlled.first() {
        zbm_trace::warn!(
            "kernel names in {dataset} skipped for a control character: {count}, such as '{name}'";
            dataset = %Printable(dataset), count = controlled.len(), name = %Printable(first)
        );
    }
    version_sort(&mut found);
    found
}

/// The kernels of a boot environment mounted at `mnt`, as it names them
/// (`/boot/<name>`), in version order: the [`kernel_names`] that are
/// regular files ([`boot_image`]), at most the newest [`MAX_KERNELS`].
pub(crate) fn kernel_files(files: &dyn Files, dataset: &str, mnt: &Path) -> Vec<String> {
    let mut found = kernel_names(files, dataset, mnt);
    let left_out = found.len().saturating_sub(MAX_KERNELS);
    if left_out > 0 {
        zbm_trace::warn!(
            "{dataset} has {count} kernels in /boot: the {left_out} oldest are left out";
            dataset = %Printable(dataset), count = found.len(), left_out
        );
        found.drain(..left_out);
    }
    found.retain(|kernel| {
        let Err(why) = boot_image(files, mnt, Path::new(kernel)) else {
            return true;
        };
        let mut refusals = Refusals::new(dataset, kernel);
        refusals.note(kernel, &why);
        refusals.report();
        false
    });
    found
}

/// The initramfs of the kernel `kernel` (as the environment names it) of
/// the boot environment mounted at `mnt`, as the environment names it, or
/// `None`: the first regular file ([`boot_image`]) in the kernel's
/// directory among names made of `initramfs` or `initrd`, the kernel's
/// file name or its version (from the first `-` on), and `.img` with or
/// without a compression suffix, tried in a fixed order. A name longer
/// than [`NAME_MAX`] is not looked up; the files that exist and are
/// refused go to `refusals`.
pub(crate) fn find_initramfs(
    files: &dyn Files,
    mnt: &Path,
    kernel: &str,
    refusals: &mut Refusals<'_>,
) -> Option<String> {
    let (dir, kern) = kernel.rsplit_once('/')?;
    // The "version": from the first hyphen on, or empty
    let kver = kern.find('-').map_or("", |hyphen| &kern[hyphen..]);
    let candidates = [
        format!("initramfs-{kern}.img"),
        format!("initramfs{kver}.img"),
        format!("initrd.img-{kern}"),
        format!("initrd.img{kver}"),
        format!("initramfs-{kern}"),
        format!("initramfs{kver}"),
    ];
    let mut names: Vec<String> = candidates.to_vec();
    let compressions = ["", ".gz", ".bz2", ".xz", ".lzma", ".lz4", ".lzo", ".zstd"];
    for img in [".img", ""] {
        for compression in compressions {
            let ext = format!("{img}{compression}");
            for prefix in ["initramfs", "initrd"] {
                for label in [kern, kver] {
                    names.push(format!("{prefix}{label}{ext}"));
                    names.push(format!("{prefix}{ext}{label}"));
                }
            }
        }
    }
    for (at, name) in names.iter().enumerate() {
        // A name tried before, or one too long to exist.
        if name.len() > NAME_MAX || names.get(..at).is_some_and(|tried| tried.contains(name)) {
            continue;
        }
        let candidate = format!("{dir}/{name}");
        match boot_image(files, mnt, Path::new(&candidate)) {
            Ok(_) => return Some(candidate),
            Err(why) => refusals.note(&candidate, &why),
        }
    }
    None
}

/// A kernel of a boot environment as the search finds it.
pub(crate) struct Found {
    /// The kernel, as the environment names it (`/boot/<name>`).
    pub(crate) kernel: String,
    /// Its initramfs, as the environment names it, if it has one.
    pub(crate) initramfs: Option<String>,
    /// Its command line file (`<kernel>.kcl`), as it is opened: resolved
    /// inside the environment, below its mount point, if it has one.
    pub(crate) kcl: Option<String>,
}

/// The kernels of the boot environment `dataset` mounted at `mnt`
/// ([`kernel_files`]), each with its initramfs and command line file;
/// one warning for each kernel with files that exist and are refused.
pub(crate) fn walk(files: &dyn Files, dataset: &str, mnt: &Path) -> Vec<Found> {
    let mut walked = Vec::new();
    for kernel in kernel_files(files, dataset, mnt) {
        let mut refusals = Refusals::new(dataset, &kernel);
        let initramfs = find_initramfs(files, mnt, &kernel, &mut refusals);
        // "${mnt}/${kpath}.kcl"
        let name = format!("{kernel}.kcl");
        let kcl = small_file(files, mnt, Path::new(&name))
            .inspect_err(|why| refusals.note(&name, why))
            .ok();
        refusals.report();
        walked.push(Found {
            kernel,
            initramfs,
            kcl,
        });
    }
    walked
}

/// The kernels and command line files of a boot environment.
#[non_exhaustive]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Discovery {
    /// The kernels with an initramfs, in version order.
    pub kernels: Vec<Kernel>,
    /// The `<kernel>.kcl` files next to the kernels (whether or not they have an
    /// initramfs), with the kernel each belongs to, to be copied to the command
    /// line cache: each as it is opened, resolved inside the environment.
    pub kcl_files: Vec<(String, PathBuf)>,
}

/// Finds the kernels of `dataset`, mounted at `mnt`.
pub fn discover(files: &dyn Files, dataset: &str, mnt: &Path) -> Discovery {
    let mut discovery = Discovery::default();
    for found in walk(files, dataset, mnt) {
        if let Some(initramfs) = found.initramfs {
            discovery.kernels.push(Kernel {
                dataset: dataset.to_owned(),
                kernel: found.kernel.clone(),
                initramfs,
            });
        }
        if let Some(kcl) = found.kcl {
            discovery.kcl_files.push((found.kernel, PathBuf::from(kcl)));
        }
    }
    discovery
}

/// The kernel to boot: the last one, or the last one whose kernel path
/// `selector` matches, from `org.zfsbootmenu:kernel`. `None` without
/// kernels.
pub fn select<'a>(kernels: &'a [Kernel], selector: Option<&KernelSelector>) -> Option<&'a Kernel> {
    let chosen = kernels.last()?;
    let Some(selector) = selector else {
        return Some(chosen);
    };
    let matched = kernels
        .iter()
        .rev()
        .find(|kernel| selector.matches(&kernel.kernel));
    Some(matched.unwrap_or(chosen))
}

#[cfg(test)]
mod tests;
