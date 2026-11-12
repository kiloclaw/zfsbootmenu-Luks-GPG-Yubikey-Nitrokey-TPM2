//! The names an image of the Rust runtime links to `zbm`, and what Tab
//! completes after them and after the shell library's functions in its
//! recovery shell.

/// A link to `/libexec/zbm` in an image of the Rust runtime: `zbm` run under
/// its name runs the applet of that name, or the applet it is another name
/// of.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Linked {
    /// The name of the link, which `zbm` finds in its table of applets.
    pub name: &'static str,
    /// The directory of the link: `/bin`, or `/libexec`.
    pub dir: &'static str,
    /// The arguments `zbm` puts before the user's under this name.
    pub first: &'static [&'static str],
    /// What Tab completes after the name, if anything: its first argument,
    /// or the argument of the option given, such as `--snapshots`.
    pub complete: Option<(Completion, Option<&'static str>)>,
}

/// What Tab completes in the recovery shell of an image of the Rust
/// runtime: the names a command takes, which `zbm complete KIND PREFIX`
/// lists (ADR 0018, point 23).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion {
    /// The filesystems, of which `zkexec` boots a kernel.
    Filesystems,
    /// The filesystems and their snapshots, which `zfs-chroot` and the
    /// library's chroot and mount take.
    FilesystemsAndSnapshots,
    /// The boot environments: filesystems mounted at `/` that can be
    /// mounted, of which `zsnapshots` shows the snapshots.
    BootEnvironments,
    /// The pools imported read-only, which the library imports read/write.
    ReadOnlyPools,
    /// The pools imported read/write, which the library imports read-only.
    WritablePools,
    /// The EFI system partitions that are not mounted, which `mount_esp`
    /// mounts.
    EspPartitions,
    /// `ro` and `rw`, how the library mounts `efivarfs`.
    EfivarfsModes,
}

impl Completion {
    /// Every kind.
    pub const ALL: [Completion; 7] = [
        Completion::Filesystems,
        Completion::FilesystemsAndSnapshots,
        Completion::BootEnvironments,
        Completion::ReadOnlyPools,
        Completion::WritablePools,
        Completion::EspPartitions,
        Completion::EfivarfsModes,
    ];

    /// The functions of the shell library that the recovery shell offers
    /// as commands and whose argument Tab completes, each with what it
    /// completes.
    pub const FUNCTIONS: &[(&str, Completion)] = &[
        ("zfs_chroot", Completion::FilesystemsAndSnapshots),
        ("set_rw_pool", Completion::ReadOnlyPools),
        ("set_ro_pool", Completion::WritablePools),
        ("mount_zfs", Completion::FilesystemsAndSnapshots),
        ("mount_efivarfs", Completion::EfivarfsModes),
    ];

    /// Its name, as `zbm complete` takes it: `boot-environments`.
    pub fn name(self) -> &'static str {
        match self {
            Completion::Filesystems => "filesystems",
            Completion::FilesystemsAndSnapshots => "filesystems-and-snapshots",
            Completion::BootEnvironments => "boot-environments",
            Completion::ReadOnlyPools => "read-only-pools",
            Completion::WritablePools => "writable-pools",
            Completion::EspPartitions => "esp-partitions",
            Completion::EfivarfsModes => "efivarfs-modes",
        }
    }

    /// The kind named `name`, if any.
    pub fn named(name: &str) -> Option<Completion> {
        Completion::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

/// The links to `/libexec/zbm` that images of the Rust runtime have, which
/// `zbm` holds to its table of applets.
pub const LINKED: &[Linked] = &[
    Linked {
        name: "zluks",
        dir: "/bin",
        first: &[],
        complete: None,
    },
    Linked {
        name: "zfsbootmenu",
        dir: "/bin",
        first: &[],
        complete: Some((Completion::BootEnvironments, Some("--snapshots"))),
    },
    Linked {
        name: "zfunc",
        dir: "/libexec",
        first: &[],
        complete: None,
    },
    Linked {
        name: "reboot",
        dir: "/bin",
        first: &[],
        complete: None,
    },
    Linked {
        name: "poweroff",
        dir: "/bin",
        first: &[],
        complete: None,
    },
    Linked {
        name: "shutdown",
        dir: "/bin",
        first: &[],
        complete: None,
    },
    Linked {
        name: "firmware-setup",
        dir: "/bin",
        first: &[],
        complete: None,
    },
    Linked {
        name: "zfs-chroot",
        dir: "/bin",
        first: &[],
        complete: Some((Completion::FilesystemsAndSnapshots, None)),
    },
    Linked {
        name: "zkexec",
        dir: "/bin",
        first: &[],
        complete: Some((Completion::Filesystems, None)),
    },
    Linked {
        name: "mount_esp",
        dir: "/bin",
        first: &[],
        complete: Some((Completion::EspPartitions, None)),
    },
    Linked {
        name: "logs",
        dir: "/bin",
        first: &[],
        complete: None,
    },
    Linked {
        name: "zlogtail",
        dir: "/bin",
        first: &[],
        complete: None,
    },
    Linked {
        name: "ztrace",
        dir: "/bin",
        first: &["--trace"],
        complete: None,
    },
    Linked {
        name: "help",
        dir: "/bin",
        first: &[],
        complete: None,
    },
    Linked {
        name: "zsnapshots",
        dir: "/bin",
        first: &[],
        complete: Some((Completion::BootEnvironments, None)),
    },
];
