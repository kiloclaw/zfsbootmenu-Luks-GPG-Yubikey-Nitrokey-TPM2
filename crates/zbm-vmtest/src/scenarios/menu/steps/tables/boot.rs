//! The step tables of `menu-boot`, in the order it runs them.

use super::super::{Await, Step, key};
use super::Act::{Select, Type};
use super::{CLEAR, ENTER, ESCAPE};

/// `menu-boot`, first boot: the palette's entries, and three it does not
/// offer: boot tries back and the re-seal, which the menu does not perform,
/// and Reboot into Firmware Setup, which a BIOS cannot open.
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
    Step::new(
        "the command line offers no Reboot into Firmware Setup",
        &[CLEAR, Type("firmware")],
        Await::Text(": firmware"),
    )
    .hiding(&["Reboot into Firmware Setup"]),
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
