//! The names an image of the Rust runtime links to `zbm`.

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
}

/// The links to `/libexec/zbm` that images of the Rust runtime have, which
/// `zbm` holds to its table of applets.
pub const LINKED: &[Linked] = &[
    Linked {
        name: "zluks",
        dir: "/bin",
        first: &[],
    },
    Linked {
        name: "zfsbootmenu",
        dir: "/bin",
        first: &[],
    },
    Linked {
        name: "zfunc",
        dir: "/libexec",
        first: &[],
    },
    Linked {
        name: "reboot",
        dir: "/bin",
        first: &[],
    },
    Linked {
        name: "poweroff",
        dir: "/bin",
        first: &[],
    },
    Linked {
        name: "shutdown",
        dir: "/bin",
        first: &[],
    },
    Linked {
        name: "firmware-setup",
        dir: "/bin",
        first: &[],
    },
    Linked {
        name: "zfs-chroot",
        dir: "/bin",
        first: &[],
    },
    Linked {
        name: "zkexec",
        dir: "/bin",
        first: &[],
    },
    Linked {
        name: "mount_esp",
        dir: "/bin",
        first: &[],
    },
    Linked {
        name: "logs",
        dir: "/bin",
        first: &[],
    },
    Linked {
        name: "zlogtail",
        dir: "/bin",
        first: &[],
    },
    Linked {
        name: "ztrace",
        dir: "/bin",
        first: &["--trace"],
    },
];
