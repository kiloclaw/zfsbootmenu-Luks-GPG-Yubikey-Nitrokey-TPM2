//! The step tables of the menu tests, in the order each test runs them.
//!
//! The keys: `2` and `3` choose the Snapshots and Pools tabs, `1` the Boot
//! tab; `r` rolls back, `c` clones, `y` duplicates, `m` clones and
//! promotes, `n` takes a snapshot, `f` shows the differences, Space marks,
//! `Delete` destroys; `i` imports, `w` imports read/write, `u` unlocks;
//! `l` opens the kernels, `e` edits the command line; `:` opens the
//! palette. Enter opens a snapshot's actions, chooses in a sheet and the
//! palette, confirms a name typed, and runs a change the palette chose
//! once its dry run shows its plan.

use super::super::super::drive::Key;
use super::super::{VOLUME, device, uuid};
use super::{Act, Await, Step, key};
use Act::{Press, Select, SelectRow, Type};

/// Enter.
const ENTER: Act = Press(Key::Enter);

/// Escape.
const ESCAPE: Act = Press(Key::Escape);

/// Ctrl-U, which clears a name field before a name is typed.
const CLEAR: Act = Press(Key::CtrlU);

/// The recovery shell, from the palette: Escape first leaves a view for
/// the Boot tab, where it does nothing.
pub(in crate::scenarios::menu) const SHELL: &[Step] = &[Step::new(
    "the palette's Recovery Shell",
    &[
        ESCAPE,
        key(':'),
        Type("shell"),
        Select("Recovery Shell"),
        ENTER,
    ],
    Await::Nothing,
)];

/// Power off, from the palette.
pub(in crate::scenarios::menu) const POWER_OFF: &[Step] = &[Step::new(
    "the palette's Power Off",
    &[key(':'), Type("power off"), Select("Power Off"), ENTER],
    Await::Nothing,
)];

/// Reboot, from the palette: the row that reads Reboot alone, as Reboot
/// into Firmware Setup holds the word too.
pub(in crate::scenarios::menu) const REBOOT: &[Step] = &[Step::new(
    "the palette's Reboot",
    &[key(':'), Type("reboot"), SelectRow("Reboot"), ENTER],
    Await::Nothing,
)];

/// Back to the Boot tab, where the recovery shell of the serial tests
/// gives the menu back between two of their tables: Escape leaves a view
/// for its tab, and a tab for the Boot tab. The display test, which opens
/// no shell, goes on from there.
pub(in crate::scenarios::menu) const BOOT_TAB: &[Step] = &[Step::new(
    "back to the Boot tab",
    &[ESCAPE, ESCAPE, key('1')],
    Await::Text("[1 Boot]"),
)];

/// `menu-snapshots`, up to the shell: a rollback, one ZFS refuses, a new
/// snapshot, a clone, a copy, and a restore from the differences. The
/// changes the setup made after `three` stay after the rollback to `four`.
pub(in crate::scenarios::menu) const SNAPSHOTS_CHANGES: &[Step] = &[
    Step::new(
        "the Snapshots tab of tmenu/ROOT/be",
        &[key('2')],
        Await::Text("five"),
    )
    .listing(&["one", "two", "held", "three", "four", "five"]),
    Step::new(
        "roll back to four: the review page",
        &[Select("four"), key('r')],
        Await::Text("WHAT IS LOST"),
    )
    .showing(&["Roll Back tmenu/ROOT/be to four", "Type be to confirm:"])
    .listing(&["five"]),
    Step::new(
        "roll back to four: be typed",
        &[Type("be"), ENTER],
        Await::Done,
    ),
    Step::new(
        "roll back to one: ZFS would refuse it",
        &[Select("one"), key('r')],
        Await::Text("ZFS would refuse this:"),
    )
    .showing(&[
        "two has the clone tmenu/two-clone: destroy or promote the clone first.",
        "held is held by zbmtest-hold: release the hold first.",
    ]),
    Step::new(
        "a new snapshot",
        &[ESCAPE, key('n')],
        Await::Text("New Snapshot of tmenu/ROOT/be"),
    )
    .showing(&["Name: tmenu/ROOT/be@"]),
    Step::new(
        "a new snapshot: menu-new",
        &[CLEAR, Type("menu-new"), ENTER],
        Await::Done,
    ),
    Step::new(
        "clone two",
        &[Select("two"), key('c')],
        Await::Text("Clone two"),
    )
    .showing(&["Name: tmenu/ROOT/be_NEW"]),
    Step::new(
        "clone two: cloned",
        &[CLEAR, Type("cloned"), ENTER],
        Await::Done,
    ),
    Step::new(
        "duplicate four",
        &[Select("four"), key('y')],
        Await::Text("Duplicate four"),
    )
    .showing(&["Name: tmenu/ROOT/be_NEW"]),
    Step::new(
        "duplicate four: copied",
        &[CLEAR, Type("copied"), ENTER],
        Await::Done,
    ),
    Step::new(
        "the differences since one",
        &[Select("one"), key('f')],
        Await::Text("CHANGES SINCE ONE"),
    )
    .showing(&[
        "M /zbmtest/restore-me",
        "- /zbmtest/removed",
        "Enter Restore",
    ]),
    Step::new(
        "the differences since one: both files marked",
        &[
            Select("/zbmtest/restore-me"),
            key(' '),
            Select("/zbmtest/removed"),
            key(' '),
        ],
        Await::Text("• - /zbmtest/removed"),
    )
    .showing(&["• M /zbmtest/restore-me"]),
    Step::new(
        "restore the marked files",
        &[ENTER],
        Await::Text("Restore 2 Files from one"),
    )
    .showing(&["tmenu/ROOT/be@before-restore-<time>", "Type be to confirm:"]),
    Step::new(
        "restore the marked files: be typed",
        &[Type("be"), ENTER],
        Await::Done,
    ),
];

/// `menu-snapshots`, from the menu the first shell gave back: the diff of
/// two marked snapshots offers no restore, and their destroy.
pub(in crate::scenarios::menu) const SNAPSHOTS_DESTROY: &[Step] = &[
    Step::new(
        "the Snapshots tab again",
        &[key('2')],
        Await::Text("menu-new"),
    )
    .listing(&["one", "three"]),
    Step::new(
        "one and three marked: their differences, with no restore",
        &[Select("one"), key(' '), Select("three"), key(' '), key('f')],
        Await::Text("CHANGES SINCE ONE"),
    )
    .hiding(&["Enter Restore"]),
    Step::new(
        "destroy the marked snapshots",
        &[ESCAPE, Press(Key::Delete)],
        Await::Text("WHAT IS LOST"),
    )
    .showing(&[
        "Destroy 2 Snapshots of tmenu/ROOT/be",
        "Destroying the snapshots frees",
        "Type be to confirm:",
    ]),
    Step::new(
        "destroy the marked snapshots: be typed",
        &[Type("be"), ENTER],
        Await::Done,
    ),
];

/// `menu-snapshots`, from the menu the second shell gave back: destroys
/// ZFS would refuse, one from the palette, and last the clone and promote,
/// which takes `two`, `held` and `four` from `tmenu/ROOT/be`.
pub(in crate::scenarios::menu) const SNAPSHOTS_REFUSED: &[Step] = &[
    Step::new(
        "the Snapshots tab once more",
        &[key('2')],
        Await::Text("held"),
    )
    .listing(&["two", "held", "four", "menu-new"]),
    Step::new(
        "the actions of held",
        &[Select("held"), ENTER],
        Await::Text("Snapshot held"),
    )
    .showing(&["Destroy It…"]),
    Step::new(
        "destroy held: ZFS would refuse it",
        &[Select("Destroy It…"), ENTER],
        Await::Text("ZFS would refuse this:"),
    )
    .showing(&["held is held by zbmtest-hold: release the hold first."]),
    Step::new(
        "destroy two: ZFS would refuse it",
        &[ESCAPE, Select("two"), Press(Key::Delete)],
        Await::Text("ZFS would refuse this:"),
    )
    .showing(&["two has the clone"]),
    Step::new(
        "destroy menu-new from the palette",
        &[
            ESCAPE,
            Select("menu-new"),
            key(':'),
            Type("destroy"),
            Select("Destroy tmenu/ROOT/be@menu-new"),
            ENTER,
        ],
        Await::Text("WHAT IS LOST"),
    )
    .showing(&[
        "Destroy tmenu/ROOT/be@menu-new",
        "Type menu-new to confirm:",
    ]),
    Step::new(
        "destroy menu-new: menu-new typed",
        &[Type("menu-new"), ENTER],
        Await::Done,
    ),
    Step::new(
        "clone and promote four",
        &[Select("four"), key('m')],
        Await::Text("Clone and Promote four"),
    )
    .showing(&["Name: tmenu/ROOT/be_NEW"]),
    Step::new(
        "clone and promote four: promoted",
        &[CLEAR, Type("promoted"), ENTER],
        Await::Text("Type be to confirm:"),
    )
    .showing(&["then depends on the clone"]),
    Step::new(
        "clone and promote four: be typed",
        &[Type("be"), ENTER],
        Await::Done,
    ),
];

/// `menu-pools`: an import of each kind, a rewind, an export from the
/// palette, after its dry run, the unlock of the volume, and last the
/// import that takes the host ID of the host that last imported
/// `tforeign`.
///
/// `w` imports read/write and never toggles back: the import's choices
/// offer read-only again. The forced import is read-only, as its review
/// page says, so it writes nothing to `tforeign`, which keeps `deadbeef`
/// as its last host for the import that takes its host ID.
pub(in crate::scenarios::menu) const POOLS: &[Step] = &[
    Step::new(
        "the Pools tab",
        &[key('3')],
        Await::Row("tforeign", "host deadbeef · not imported"),
    )
    .listing(&["tmenu", "tother", VOLUME]),
    Step::new(
        "import tother: the choices",
        &[Select("tother"), key('i')],
        Await::Text("Import tother"),
    )
    .showing(&[
        "Import with This Host ID",
        "Import with Its Last Host ID",
        "Force the Import…",
    ]),
    Step::new("import tother with this host ID", &[ENTER], Await::Done),
    Step::new(
        "tother is imported read-only",
        &[],
        Await::Row("tother", "read-only"),
    ),
    Step::new(
        "tother read/write",
        &[Select("tother"), key('w')],
        Await::Done,
    ),
    Step::new(
        "tother is imported read/write",
        &[],
        Await::Row("tother", "read/write"),
    ),
    Step::new(
        "import tother again: the choices",
        &[Select("tother"), key('i')],
        Await::Text("Import tother"),
    )
    .showing(&["Import Read-Only", "Rewind to the Checkpoint…"]),
    Step::new("import tother read-only", &[ENTER], Await::Done),
    Step::new(
        "tother is imported read-only again",
        &[],
        Await::Row("tother", "read-only"),
    ),
    Step::new(
        "rewind tother: the review page",
        &[
            Select("tother"),
            key('i'),
            Select("Rewind to the Checkpoint…"),
            ENTER,
        ],
        Await::Text("WHAT IS LOST"),
    )
    .showing(&[
        "Rewind tother to Its Checkpoint",
        "the filesystem tother/after, made since",
        "Type tother to confirm:",
    ]),
    Step::new(
        "rewind tother: tother typed",
        &[Type("tother"), ENTER],
        Await::Done,
    ),
    Step::new(
        "tother is imported read/write, rewound",
        &[],
        Await::Row("tother", "read/write"),
    ),
    Step::new(
        "force the import of tforeign: the review page",
        &[
            Select("tforeign"),
            key('i'),
            Select("Force the Import…"),
            ENTER,
        ],
        Await::Text("WHAT IS LOST"),
    )
    .showing(&[
        "Force the Import of tforeign",
        "Force the import of tforeign read-only",
        "WHO ELSE MAY USE IT",
        "Last imported by host",
        "deadbeef",
        "Last written",
        "Undo: export tforeign.",
        "Type tforeign to confirm:",
    ]),
    Step::new(
        "force the import of tforeign: tforeign typed",
        &[Type("tforeign"), ENTER],
        Await::Done,
    ),
    Step::new(
        "tforeign is imported read-only",
        &[],
        Await::Row("tforeign", "read-only"),
    ),
    Step::new(
        "export tforeign from the palette: its dry run",
        &[key(':'), Type("export"), Select("Export tforeign"), ENTER],
        Await::Text("zpool export tforeign"),
    ),
    Step::new("export tforeign: run", &[ENTER], Await::Done),
    Step::new(
        "tforeign is not imported",
        &[],
        Await::Row("tforeign", "not imported"),
    ),
    Step::new(
        "unlock the volume",
        &[Select(VOLUME), key('u')],
        Await::Text(concat!("Unlock ", device!())),
    )
    .showing(&["Passphrase:"]),
    Step::new(
        "unlock the volume: pw typed",
        &[Type("pw"), ENTER],
        Await::Done,
    )
    .showing(&[concat!("is open as luks-", uuid!())]),
    Step::new(
        "the scan finds tlocked in the volume",
        &[],
        Await::Row("tlocked", "not imported"),
    ),
    Step::new(
        "import tlocked: the choices",
        &[Select("tlocked"), key('i')],
        Await::Text("Import tlocked"),
    ),
    Step::new("import tlocked with this host ID", &[ENTER], Await::Done),
    Step::new(
        "tlocked is imported read-only",
        &[],
        Await::Row("tlocked", "read-only"),
    ),
    Step::new(
        "import tforeign with its last host ID",
        &[
            Select("tforeign"),
            key('i'),
            Select("Import with Its Last Host ID"),
            ENTER,
        ],
        Await::Done,
    ),
    Step::new(
        "tforeign is imported read-only, as its last host",
        &[],
        Await::Row("tforeign", "read-only"),
    ),
];

/// `menu-boot`, first boot: the palette's entries, and two it offers on no
/// machine, as the menu performs neither: boot tries back and the re-seal.
/// Its Reboot into Firmware Setup is not checked: the menu offers it on
/// BIOS too.
/// Rescan the Pools is chosen over the Pools tab, not the Log tab, whose
/// running glyph in ASCII reads as a selected row.
pub(in crate::scenarios::menu) const BOOT_TOUR: &[Step] = &[
    Step::new(
        "the palette's Kernels of tmenu/ROOT/be",
        &[
            key(':'),
            Type("kernels of"),
            Select("Kernels of tmenu/ROOT/be"),
            ENTER,
        ],
        Await::Text("Environment tmenu/ROOT/be"),
    ),
    Step::new(
        "the palette's Snapshots of tmenu/ROOT/be",
        &[
            key(':'),
            Type("snapshots of"),
            Select("Snapshots of tmenu/ROOT/be"),
            ENTER,
        ],
        Await::Text("[2 Snapshots]"),
    ),
    Step::new(
        "the palette's Edit the command line of tmenu/ROOT/be",
        &[
            key(':'),
            Type("edit the"),
            Select("Edit the command line of tmenu/ROOT/be"),
            ENTER,
        ],
        Await::Text("For this boot only"),
    )
    .showing(&["COMMAND LINE", "console=ttyS0 loglevel=4"]),
    Step::new(
        "Escape, then the palette's Pools and volumes",
        &[
            ESCAPE,
            key(':'),
            Type("pools and"),
            Select("Pools and volumes"),
            ENTER,
        ],
        Await::Text("[3 Pools]"),
    ),
    Step::new(
        "the palette's Log",
        &[key(':'), Type("log"), Select("Log"), ENTER],
        Await::Text("[4 Log]"),
    )
    .showing(&["LOG SINCE START"]),
    Step::new(
        "the Pools tab, then the palette's Rescan the Pools",
        &[
            key('3'),
            key(':'),
            Type("rescan"),
            Select("Rescan the Pools"),
            ENTER,
        ],
        Await::Text(": Commands"),
    )
    .showing(&["[3 Pools]"])
    .hiding(&["COMMANDS"]),
    Step::new(
        "the palette offers no boot tries back",
        &[key(':'), Type("tries")],
        Await::Text(": tries"),
    )
    .hiding(&["Its Boot Tries Back"]),
    Step::new(
        "the palette offers no re-seal",
        &[CLEAR, Type("re-seal")],
        Await::Text(": re-seal"),
    )
    .hiding(&["Re-seal the TPM2 Key"]),
];

/// `menu-boot`, first boot, before [`BOOT_KERNEL`]: the command line of
/// tmenu/ROOT/be saved for every boot, from the palette, through its dry
/// run. The machine imports tmenu read/write, writes the line and reads it
/// back: the inspector says it is set here. Each boot of `menu-boot`
/// starts on fresh disks, so the next does not see it.
pub(in crate::scenarios::menu) const BOOT_SAVED: &[Step] = &[
    Step::new(
        "the palette's Set the Command Line of tmenu/ROOT/be",
        &[
            ESCAPE,
            key('1'),
            key(':'),
            Type("set the command"),
            Select("Set the Command Line of tmenu/ROOT/be"),
            ENTER,
        ],
        Await::Text("COMMAND LINE FOR EVERY BOOT"),
    )
    .showing(&["For every boot"]),
    Step::new(
        "the command line to save, edited",
        &[Type(" zbmtest.saved=1")],
        Await::Text("loglevel=4 zbmtest.saved=1"),
    ),
    Step::new(
        "the save's dry run",
        &[ENTER],
        Await::Text("org.zfsbootmenu:commandline="),
    )
    .showing(&["Nothing has changed yet", "PCR 8"]),
    Step::new("save it", &[ENTER], Await::Done).showing(&[
        "is saved",
        "zbmtest.saved=1",
        "set here",
    ]),
];

/// `menu-boot`, first boot: the second kernel, with an edited command
/// line, which starts from the line [`BOOT_SAVED`] saved, and ends with the
/// edit; Enter boots it.
pub(in crate::scenarios::menu) const BOOT_KERNEL: &[Step] = &[
    Step::new(
        "the command line with the kernel 0.0.1-zbmtest",
        &[
            ESCAPE,
            key('1'),
            key('l'),
            Select("0.0.1-zbmtest"),
            key('e'),
        ],
        Await::Text("COMMAND LINE WITH 0.0.1-ZBMTEST"),
    )
    .showing(&["For this boot only"]),
    Step::new(
        "the command line edited",
        &[Type(" zbmtest.edited=1")],
        Await::Text("zbmtest.edited=1"),
    ),
    Step::new("boot it", &[ENTER], Await::Nothing),
];

/// `menu-boot`, second boot: a clone of `five`, from the palette, which
/// lists the snapshots of an environment once they are read, and shows
/// the clone's plan before it boots.
pub(in crate::scenarios::menu) const BOOT_CLONE: &[Step] = &[
    Step::new(
        "the Snapshots tab of tmenu/ROOT/be",
        &[key('2')],
        Await::Text("five"),
    ),
    Step::new(
        "the palette's Boot a clone of tmenu/ROOT/be@five",
        &[
            key(':'),
            Type("five"),
            Select("Boot a clone of tmenu/ROOT/be@five"),
            ENTER,
        ],
        Await::Text("Boot a Clone of five"),
    )
    .showing(&["Name: tmenu/ROOT/be_NEW"]),
    Step::new(
        "boot a clone of five: booted, its dry run",
        &[CLEAR, Type("booted"), ENTER],
        Await::Text("zfs clone"),
    ),
    Step::new("boot a clone of five: run", &[ENTER], Await::Nothing),
];
