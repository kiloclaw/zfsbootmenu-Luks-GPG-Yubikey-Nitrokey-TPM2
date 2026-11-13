//! This system's name servers in a chroot: its `/etc/resolv.conf`, where it
//! has one, mounted read-only over the environment's, the environment's
//! links followed inside the chroot's root. Nothing is written to the
//! environment: where its file is a link into an empty `/run`, as
//! systemd-resolved's is, a `/run` in memory holds the link's target.

use std::path::Path;

use zbm_platform::Files;
use zbm_platform::files::FileKind;
use zbm_tools::mount::inside::{self, InRoot, Inside, Unresolved};
use zbm_tools::mount::{Mount, Mounted, NotReadOnly};

use super::{not_mounted, notice};

/// Where the name servers are, in this system and in the chroot.
const RESOLV_CONF: &str = "/etc/resolv.conf";

/// The directory below the root whose links' targets a `tmpfs` may hold.
const RUN: &str = "run";

/// What a chroot mounted for its name servers: the mounts in order, and
/// their notices.
#[derive(Default)]
pub(super) struct NameServers<'c> {
    /// The mounts, in the order made.
    pub(super) mounts: Vec<Mounted<'c>>,
    /// What is mounted how, or why nothing is, for the person.
    pub(super) notices: Vec<String>,
}

/// Mounts this system's `/etc/resolv.conf`, when it is a file that is not
/// empty, read-only with `mount` on the environment's in the chroot whose
/// root is mounted at `at`, as `files` find it there
/// ([`inside::resolve`]): on a file, or, for a path missing below an empty
/// `/run`, on a file made in a `tmpfs` on that `/run`. Where the
/// environment's side leads elsewhere, nothing is mounted, and a notice
/// says why.
pub(super) fn name_servers<'c>(mount: Mount<'c>, files: &dyn Files, at: &Path) -> NameServers<'c> {
    let image = Path::new(RESOLV_CONF);
    let has_servers = files
        .metadata(image)
        .is_ok_and(|metadata| metadata.kind == FileKind::File && metadata.len > 0);
    if !has_servers {
        tracing::debug!(
            path = RESOLV_CONF,
            "this system has no name servers for the chroot"
        );
        return NameServers::default();
    }
    match inside::resolve(files, at, image) {
        Ok(inside) if files.is_file(&inside.path.under(at)) => bind(mount, at, &inside)
            .map_or_else(NameServers::default, |(bound, said)| NameServers {
                mounts: vec![bound],
                notices: vec![said],
            }),
        Ok(inside) => refused(&format!(
            "{} is not a file",
            inside.path.in_chroot().display()
        )),
        Err(Unresolved::Missing {
            path,
            names_only: true,
        }) if is_empty_run(files, at, &path) => {
            in_memory(mount, files, at, path).unwrap_or_default()
        }
        Err(unresolved) => refused(&unresolved.to_string()),
    }
}

/// Whether `path` is below the chroot's `/run`, and that `/run`, below `at`,
/// is an empty directory and no link. Only a `/run` whose link cannot be read
/// because it is no link counts: a missing `/run`, or one that cannot be
/// looked at, does not.
fn is_empty_run(files: &dyn Files, at: &Path, path: &InRoot) -> bool {
    if !path.is_below(RUN) {
        return false;
    }
    let run = path.top().under(at);
    let is_no_link = files
        .read_link(&run)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidInput);
    is_no_link && files.read_dir(&run).is_ok_and(|names| names.is_empty())
}

/// Mounts a `tmpfs` with `mount` on the chroot's `/run`, below `at`, makes
/// `path` there, its directories and an empty file, and binds the image's
/// name servers over it, read-only; nothing, with nothing left mounted,
/// when a step fails.
fn in_memory<'c>(
    mount: Mount<'c>,
    files: &dyn Files,
    at: &Path,
    path: InRoot,
) -> Option<NameServers<'c>> {
    let run = mount
        .tmpfs(&path.top().under(at))
        .inspect_err(|code| tracing::debug!(code, "no /run in memory for the chroot"))
        .ok()?;
    let target = path.under(at);
    let dir = target.parent().unwrap_or(at);
    let made = files.create_dir_all(dir);
    if let Err(error) = made.and_then(|()| files.write_private(&target, b"")) {
        tracing::debug!(%error, "the chroot's name servers have no file to mount on");
        return None;
    }
    let inside = Inside {
        path,
        followed: true,
    };
    let (bound, said) = bind(mount, at, &inside)?;
    Some(NameServers {
        mounts: vec![run, bound],
        notices: vec![notice("/run (in memory)", true), said],
    })
}

/// Binds this system's name servers with `mount` on `inside`, below `at`,
/// read-only: the mount and its notice, read/write in red when the bind
/// could be neither made read-only nor undone; nothing when nothing stays
/// mounted.
fn bind<'c>(mount: Mount<'c>, at: &Path, inside: &Inside) -> Option<(Mounted<'c>, String)> {
    let what = if inside.followed {
        format!("{RESOLV_CONF} -> {}", inside.path.in_chroot().display())
    } else {
        RESOLV_CONF.to_owned()
    };
    match mount.bind_read_only(Path::new(RESOLV_CONF), &inside.path.under(at)) {
        Ok(bound) => Some((bound, notice(&what, false))),
        Err(NotReadOnly {
            writable: Some(bound),
            code,
        }) => {
            tracing::warn!(code, "the chroot's name servers stay read/write");
            Some((bound, notice(&what, true)))
        }
        Err(NotReadOnly {
            writable: None,
            code,
        }) => {
            tracing::debug!(code, "the chroot's name servers are not mounted");
            None
        }
    }
}

/// Nothing mounted, and the notice that this system's name servers are
/// not mounted, and `why`, escaped.
fn refused<'c>(why: &str) -> NameServers<'c> {
    tracing::debug!(why, "the chroot's name servers are not mounted");
    NameServers {
        mounts: Vec::new(),
        notices: vec![not_mounted(RESOLV_CONF, why)],
    }
}
