//! A shell in a chroot of a filesystem (ADR 0018, points 6 and 7), as the
//! legacy `zfs-chroot` opened one: on a pool imported read-only, the
//! question whether to import it read/write; the key of the filesystem's
//! encryption root; the filesystem with this system's name servers
//! (SB15) and the kernel's file systems mounted below it, never on a path
//! that passes through a symbolic link of the filesystem, the root itself
//! on a path that passes through none and through no other mount (SB16);
//! and the first shell that runs there, which the shell's gate opens
//! (ADR 0010).

use std::io;
use std::path::Path;

use zbm_boot::BootSettings;
use zbm_bootenv::{BootEnvError, Noise};
use zbm_platform::console::{Console, Screen};
use zbm_platform::interaction::{Interaction, PromptRequest, Subject};
use zbm_platform::{Context, Files, System};
use zbm_pool::writable;
use zbm_state::State;
use zbm_tools::console::{ConsoleInteraction, colorize};
use zbm_tools::mount::inside::InRoot;
use zbm_tools::mount::{HostEfivars, Mount, Mounted};
use zbm_tools::programs::Chroot;
use zbm_types::{Answer, FilesystemName, Printable, RetryDelay};

use crate::auth::{self, Authority};

mod kernel;
mod resolv;
mod root;

/// How long the question waits, in seconds, before the chroot stays
/// read-only.
const QUESTION: u64 = 10;

/// Why no shell opened in a chroot.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ChrootError {
    /// The image is locked, and no one proved themselves: nothing was
    /// imported or mounted.
    #[error("this ZFSBootMenu is locked")]
    Refused,
    /// The key of the filesystem's encryption root was not loaded: the
    /// person gave none, or a wrong one.
    #[error(transparent)]
    Key(#[from] BootEnvError),
    /// The filesystem could not be mounted.
    #[error("failed to mount {0}")]
    Mount(FilesystemName),
    /// No shell runs in the filesystem.
    #[error("unable to test execute a shell in {0}")]
    NoShell(FilesystemName),
    /// The chroot could not be entered, or its shell could not start.
    #[error("unable to start a shell in the chroot")]
    Start(#[from] io::Error),
}

/// What a chroot has mounted, which dropping it unmounts: the kernel's
/// file systems and the name servers below the root in reverse, each by
/// its path where no name on it is a symbolic link now, then the root with
/// everything below it, and while its mount ID is still on its path, what
/// is on it, lazily ([`root::Root::tear_down`]); then the host's
/// `efivarfs`, set back to read-only. Nothing is unmounted by a path where
/// a link or a mount has appeared on the root's way since it was mounted,
/// another root of its filesystem is on it, or it is gone
/// ([`root::Root::why_left`]): the mounts are left then, with a warning.
struct Mounts<'a> {
    /// The name servers and the kernel's file systems below the root, in
    /// the order mounted.
    below: Vec<Mounted<'a>>,
    /// Where the filesystem is mounted, and how the teardown finds it.
    place: root::Root,
    /// The filesystem, unmounted with everything below it, until dropped.
    root: Option<Mounted<'a>>,
    /// How the root is detached where `umount -R` left it.
    mount: Mount<'a>,
    /// The host's `efivarfs`, in the chroot's mode until dropped.
    host: Option<HostEfivars<'a>>,
    /// What is mounted how, or why it is not, for the person.
    notices: Vec<String>,
    /// Where the paths below the root are looked at.
    files: &'a dyn Files,
    /// Whether this system's `/tmp` is mounted below the root.
    tmp: bool,
}

impl Drop for Mounts<'_> {
    fn drop(&mut self) {
        if let Some(why) = self.place.why_left(self.files) {
            let left = self.below.drain(..).chain(self.root.take());
            left.for_each(|mounted| drop(mounted.leave()));
            let path = self.place.at().display();
            tracing::warn!(%path, why, "the chroot's mounts are left");
            return;
        }
        while let Some(mounted) = self.below.pop() {
            unmount_below(self.files, self.place.at(), mounted);
        }
        if let Some(root) = self.root.take() {
            self.place.tear_down(self.mount, self.files, root);
        }
    }
}

/// Unmounts `mounted`, below the root at `at`, by its path where nothing
/// is in the way ([`obstacle`]); leaves it otherwise, with a warning, to
/// the root's recursive unmount.
fn unmount_below(files: &dyn Files, at: &Path, mounted: Mounted<'_>) {
    match obstacle(files, at, mounted.path()) {
        None => drop(mounted),
        Some(why) => {
            let path = mounted.leave();
            tracing::warn!(path = %path.display(), why, "left mounted for the root's unmount");
        }
    }
}

/// Why nothing is to be mounted on `path`, or unmounted by it, in the
/// chroot whose root is mounted at `at`: it is not below `at`, or has
/// `..`, or a name on its way below `at` is a symbolic link, or could not
/// be read ([`InRoot::first_link`]). `None` when nothing is in the way: a
/// missing name, or one that is no directory, makes the mount or the
/// unmount fail by itself.
fn obstacle(files: &dyn Files, at: &Path, path: &Path) -> Option<String> {
    let Some(names) = path.strip_prefix(at).ok().and_then(InRoot::new) else {
        return Some(format!("{} is not below the root", path.display()));
    };
    match names.first_link(files, at) {
        Ok(None) => None,
        Ok(Some((link, target))) if link == names => {
            Some(format!("it is a link to {}", target.display()))
        }
        Ok(Some((link, target))) => Some(format!(
            "{} is a link to {}",
            link.in_chroot().display(),
            target.display()
        )),
        Err(error) => Some(error.to_string()),
    }
}

/// A chroot made ready: its shell, and what it mounted, which dropping
/// it unmounts.
pub(crate) struct Prepared<'a> {
    /// The chroot and the shell that runs there.
    chroot: Chroot,
    /// What it mounted, held until it is dropped.
    _mounts: Mounts<'a>,
}

impl AsRef<Chroot> for Prepared<'_> {
    fn as_ref(&self) -> &Chroot {
        &self.chroot
    }
}

/// Opens a shell in a chroot of `fs` on the console of `system`, as the
/// menu's Chroot does: when `authority` authorises it now, as a person who
/// opened a volume of `state` is, prepares the chroot, in the order of the
/// legacy `zfs-chroot`, asking on the console whether to import a
/// read-only pool read/write and for a locked key, with the key hooks and
/// the import policy of `settings`; then runs its shell
/// through the shell's gate until it ends. The mounts are undone and the
/// proof forgotten after. The shell's exit status.
///
/// # Errors
///
/// [`ChrootError::Refused`] without authority, before anything is
/// imported or mounted, and the other [`ChrootError`]s when no shell
/// started, with nothing left mounted.
pub fn open(
    system: &System<'_>,
    state: &State<'_>,
    authority: &Authority,
    settings: &BootSettings,
    fs: &FilesystemName,
) -> Result<i32, ChrootError> {
    let context = Context::new(*system);
    let now = system.clock.now();
    let prepare = || prepare(system, &context, state, settings, fs);
    auth::chroot(authority, state, system.runner, now, prepare)
        .unwrap_or(Err(ChrootError::Refused))
        .map(|ran| ran.status.shell_code())
}

/// Prepares a chroot of `fs` on the machine of `system`, whose ports
/// `context` holds, in the order of the legacy `zfs-chroot`: the question
/// and the key ([`ask_and_unlock`]), asked on the console, never answered
/// by a default; a clear screen with the cursor on; the mounts
/// ([`mount`]); and the shell ([`find_shell`]).
///
/// # Errors
///
/// Why no shell can run there; nothing is left mounted then.
pub(crate) fn prepare<'c>(
    system: &System<'_>,
    context: &'c Context<'_>,
    state: &State<'_>,
    settings: &BootSettings,
    fs: &FilesystemName,
) -> Result<Prepared<'c>, ChrootError> {
    let interaction = ConsoleInteraction::new(*system);
    let writable = ask_and_unlock(context, &interaction, state, settings, fs)?;
    system.console.screen(Screen::Clear);
    system.console.screen(Screen::ShowCursor);
    let mounts = mount(context, state, fs, writable)?;
    find_shell(context, system.console, mounts, fs)
}

/// For a filesystem that is no snapshot, on a pool imported read-only,
/// asks through `interaction` whether to import the pool read/write:
/// ESCAPE does, RETURN and the time running out keep it read-only. A pool
/// that cannot be imported read/write stays read-only, as `zfs-chroot`
/// left it. Then loads the key of the filesystem's encryption root if it
/// is locked, as a boot chosen in the menu loads it, with the key hooks
/// of `settings`. Whether the filesystem is mounted read/write: it is no
/// snapshot, and its pool is imported read/write.
///
/// # Errors
///
/// [`ChrootError::Key`] when the key stays unloaded.
fn ask_and_unlock(
    context: &Context<'_>,
    interaction: &ConsoleInteraction<'_>,
    state: &State<'_>,
    settings: &BootSettings,
    fs: &FilesystemName,
) -> Result<bool, ChrootError> {
    let pool = fs.pool();
    let runner = context.runner();
    if !fs.is_snapshot() && !writable::is_writable(runner, &pool) {
        let question = PromptRequest {
            subject: Subject::Chroot {
                filesystem: fs.clone(),
            },
            delay: RetryDelay::new(QUESTION),
            message: String::new(),
            on_return: "Enter r/o chroot".to_owned(),
            on_escape: "Enter r/w chroot".to_owned(),
        };
        let policy = settings.import_policy;
        if interaction.ask(&question) == Answer::Escape
            && let Err(error) = writable::set_rw_pool(context, interaction, state, &pool, policy)
        {
            tracing::warn!(%pool, %error, "the pool stays read-only");
        }
    }
    crate::boot::load_menu_key(context, interaction, state, settings, fs.dataset())?;
    Ok(!fs.is_snapshot() && writable::is_writable(runner, &pool))
}

/// Mounts `fs` on its mount point in the state directory, read/write when
/// `writable`, where that path passes through no link and no other mount
/// and holds no other filesystem, as `zbm_bootenv::mount_zfs` checks for
/// every mount of a filesystem, and reads its mount ID; this system's
/// `/etc/resolv.conf`, read-only, over the filesystem's, before anything
/// else is mounted below it ([`resolv::name_servers`]); then, as
/// `zfs-chroot` did, the kernel's file systems ([`kernel::mount`]): `/tmp`
/// bound below it, a `proc`, a `sysfs` and `/dev` bound below it; on EFI,
/// this system's `efivarfs` remounted in the chroot's mode, or mounted so
/// where it is not, and one on the chroot's `sysfs`; and a `devpts` where
/// the bound `/dev` has `pts`. A mount point that is a symbolic link, or
/// passes through one, is not mounted, with a notice; a mount below the
/// filesystem that fails is left out, as `zfs-chroot` left it.
///
/// # Errors
///
/// [`ChrootError::Mount`] when the filesystem could not be mounted, or its
/// mount point is refused, with an error line that says why; nothing is
/// created or mounted then.
fn mount<'c>(
    context: &'c Context<'_>,
    state: &State<'_>,
    fs: &FilesystemName,
    writable: bool,
) -> Result<Mounts<'c>, ChrootError> {
    let files = context.files();
    let root = if writable {
        zbm_bootenv::mount_zfs_writable(context, state, fs.dataset(), Noise::Shown)
    } else {
        zbm_bootenv::mount_zfs(context, state, fs, Noise::Shown)
    };
    let root = root.ok_or_else(|| ChrootError::Mount(fs.clone()))?;
    let at = root.path().to_path_buf();
    let mount = Mount::new(context.runner(), files);
    let mut mounts = Mounts {
        below: Vec::new(),
        place: root::Root::new(files, state.base(), at.clone(), fs.clone()),
        root: Some(root.recursive()),
        mount,
        host: None,
        notices: vec![notice(&fs.to_string(), writable)],
        files,
        tmp: false,
    };
    let names = resolv::name_servers(mount, files, &at);
    mounts.below.extend(names.mounts);
    mounts.notices.extend(names.notices);
    kernel::mount(mount, &at, writable, &mut mounts);
    Ok(mounts)
}

/// Finds the shell that runs in the chroot of `mounts`, as `zfs-chroot`
/// did ([`Chroot::find`]), then says on `console` what is mounted how, or
/// why it is not, and `/tmp`, which is this system's, last, where it is
/// mounted.
///
/// # Errors
///
/// [`ChrootError::NoShell`] when none runs there, and
/// [`ChrootError::Start`] when the chroot cannot be entered: no other
/// shell is tried then.
fn find_shell<'c>(
    context: &Context<'_>,
    console: &dyn Console,
    mounts: Mounts<'c>,
    fs: &FilesystemName,
) -> Result<Prepared<'c>, ChrootError> {
    let chroot = Chroot::find(context.runner(), context.files(), mounts.place.at(), fs)
        .map_err(ChrootError::Start)?
        .ok_or_else(|| ChrootError::NoShell(fs.clone()))?;
    let mut said = mounts.notices.join("\n");
    said.push('\n');
    if mounts.tmp {
        said.push_str(&notice("/tmp", true));
        said.push('\n');
    }
    said.push('\n');
    let _ = console.write(said.as_bytes());
    Ok(Prepared {
        chroot,
        _mounts: mounts,
    })
}

/// The notice that `what` is mounted read/write, or read-only, in the
/// colours of `zfs-chroot`.
fn notice(what: &str, writable: bool) -> String {
    let mode = if writable {
        colorize("red", "read/write")
    } else {
        colorize("green", "read-only")
    };
    let what = Printable(what).to_string();
    format!(
        "{} {} is mounted {mode}",
        colorize("white", "*"),
        colorize("orange", &what)
    )
}

/// The notice that `what` is not mounted, and `why`, both escaped.
fn not_mounted(what: &str, why: &str) -> String {
    let what = Printable(what).to_string();
    format!(
        "{} {} is not mounted: {}",
        colorize("white", "*"),
        colorize("orange", &what),
        Printable(why)
    )
}

#[cfg(test)]
mod tests;
