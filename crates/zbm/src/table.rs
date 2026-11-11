//! The table of `zbm`'s commands: each one's name, the other names it
//! answers to through a link, its summary for `zbm --list`, its usage text
//! for `--help`, and the function that runs it.

use crate::{
    Applet, applets, chroot, console, console_font, debug, explain, fonts, image, kexec, layouts,
    lock, luks, measure, menu, power, report, run_hooks, unlock, zfunc,
};

const INIT: Applet = Applet {
    name: "zfsbootmenu-init",
    names: &[],
    summary: "the start-up program, and with --supervise process 1",
    usage: "\
Usage: zbm zfsbootmenu-init [--console] [--resume]
       zbm zfsbootmenu-init --supervise

The start-up program: its stages, then with --console the countdown, the menu
and the recovery shell on the console; --resume skips the countdown and the
automatic boot after a restart. With --supervise, the supervisor that runs it
as process 1.
",
    run: applets::init,
};

const PARSE_COMMANDLINE: Applet = Applet {
    name: "zfsbootmenu-parse-commandline",
    names: &[],
    summary: "read the command line into /etc/zfsbootmenu.conf (dracut cmdline hook)",
    usage: "\
Usage: zbm zfsbootmenu-parse-commandline

Reads the kernel command line into /etc/zfsbootmenu.conf: dracut's cmdline
hook, and mkinitcpio's.
",
    run: applets::parse_commandline,
};

const PREINIT: Applet = Applet {
    name: "zfsbootmenu-preinit",
    names: &[],
    summary: "set up the console and start the supervisor (dracut pre-mount hook)",
    usage: "\
Usage: zbm zfsbootmenu-preinit

Sets up the console, then starts the supervisor in a new session on the
control terminal: dracut's pre-mount hook, and mkinitcpio's.
",
    run: applets::preinit,
};

pub(crate) const ZLUKS: Applet = Applet {
    name: "zluks",
    names: &[],
    summary: "unlock, close and list LUKS volumes",
    usage: "\
Usage: zluks [command]

Commands:
  unlock               Unlock all configured LUKS volumes (default)
  open <device> [name] Unlock a single LUKS volume; device may be a path or
                       UUID=, PARTUUID=, LABEL=, PARTLABEL=, ID=, PATH=
  open <device> <name> --key-file <path> [--allow-discards] [--read-only]
       [--journal <file>] [--verbose]
                       Open a LUKS volume with a key file and nothing else,
                       with a JSON line in <file> for every event
  close [name]         Export pools on, and close, mappings opened by
                       ZFSBootMenu (all of them unless a name is given)
  status               List mappings opened by ZFSBootMenu
  list                 List all LUKS volumes on the system and their
                       LUKS2 tokens (TPM2, FIDO2, clevis)

After unlocking, return to ZFSBootMenu (type 'exit') to import pools, or
run 'zpool import' manually.
",
    run: luks::zluks,
};

pub(crate) const UNLOCK: Applet = Applet {
    name: "unlock",
    names: &[],
    summary: "the keyholder, which opens LUKS volumes for start-up and zluks",
    usage: "\
Usage: zbm unlock serve [--quiet] [--journal FILE] [--verbose]

The keyholder: opens LUKS volumes for the program that started it, on the
connection it passed in ZBM_KEYHOLDER_FD. Start-up and zluks start it.
",
    run: unlock::run,
};

const CONSOLE_AUTOSIZE: Applet = Applet {
    name: "console-autosize",
    names: &[],
    summary: "size the console font to the display (early-setup hook)",
    usage: "\
Usage: zbm console-autosize [--console DEVICE]

Sizes the console font to the display (zbm.autosize), on the control terminal
or the virtual console DEVICE: the early-setup hook 30-console-autosize. The
exit status is 1 when no font could be loaded.
",
    run: applets::console_autosize,
};

pub(crate) const CONSOLE_FONT: Applet = Applet {
    name: "console-font",
    names: &[],
    summary: "load a console font on a virtual terminal, as setfont does",
    usage: "\
Usage: zbm console-font [-C <console>] [-d] <font>

Loads the PSF font <font> on the virtual terminal <console> (/dev/tty0, the
one shown), doubled with -d, as setfont does.
",
    run: console_font::run,
};

pub(crate) const CONSOLE_FONTS: Applet = Applet {
    name: "console-fonts",
    names: &[],
    summary: "write an image's console fonts as 256-glyph subsets (image build)",
    usage: "\
Usage: zbm console-fonts <source directory> <destination directory>

Writes each PSF font of the source to the destination, as a 256-glyph subset
where it has more glyphs (image build).
",
    run: fonts::run,
};

pub(crate) const KEYBOARD_LAYOUTS: Applet = Applet {
    name: "keyboard-layouts",
    names: &[],
    summary: "compile an image's keyboard layouts from XKB data (image build)",
    usage: "\
Usage: zbm keyboard-layouts [--xkb <directory>] [--compose <file>]
                            <destination directory> <layout>...

Compiles each layout from the XKB data (/usr/share/X11/xkb) into the
destination, with the dead keys of the compose file (image build).
",
    run: layouts::run,
};

pub(crate) const IMAGE: Applet = Applet {
    name: "image",
    names: &[],
    summary: "the plan of an image, for the dracut module and mkinitcpio hook (image build)",
    usage: "\
Usage: zbm image plan --builder dracut|mkinitcpio --config <file> --stage <dir>
       zbm image prune-firmware <image root> <directories> <files>
",
    run: image::run,
};

pub(crate) const CONSOLE: Applet = Applet {
    name: "console",
    names: &[],
    summary: "the console on the display, without the kernel's (zbm.console=drm)",
    usage: "\
Usage: zbm console [--card <path>] [--height <pixels>] [--psf] [--layout <name>]
                   [--numlock on|off] [--repeat <delay>,<rate>] [--scrollback <lines>]
                   [--palette <name>] [--large] [--status]
                   [--bell none|visual|audible|both] [--compose ralt|rwin|menu|rctrl|caps]
                   [--keyboard <vendor>:<product>=<layout>]... [--mirror <file>]
                   [--mode <width>x<height>] [--blank <minutes>] [--cells-fd <fd>]
                   [--serial <device> [--serial-size <columns>x<rows>]]
                   [--control-fd <fd>] [--devices-fd <fd>] [--kmsg-fd <fd>]
       zbm console release [--timeout <milliseconds>]
       zbm console reattach [--timeout <milliseconds>]
       zbm console layout [<name>]
       zbm console font [<pixels>|+|-|default]
",
    run: console::run,
};

pub(crate) const DEBUG: Applet = Applet {
    name: "debug",
    names: &[],
    summary: "show crash reports and saved logs (zbm debug crash, saved)",
    usage: "\
Usage: zbm debug crash [--list]   print the latest crash report, or list them
       zbm debug config           the options in force, and where they came from
       zbm debug state            the files of the state directory
       zbm debug journal [--level LEVEL]
                                  the runtime journal (error, warn, info, debug)
       zbm debug commands         the programs the runtime ran, and how they ended
       zbm debug luks             the LUKS volumes and their key methods
       zbm debug pools            the pools and the boot environments
       zbm debug saved            list the logs saved on the EFI system partition
       zbm debug save             save this boot's logs there now
       zbm debug yubikey [SLOT CHALLENGE]
                                  the YubiKeys, or a slot's response beside ykchalresp's
",
    run: debug::run,
};

pub(crate) const REPORT: Applet = Applet {
    name: "report",
    names: &[],
    summary: "write a bug report, without key material, on the ESP or a USB stick",
    usage: "\
Usage: zbm report              write a report on the EFI system partition
       zbm report DIR          write it in the directory DIR
       zbm report PARTITION    write it on PARTITION (a device, or LABEL=...)
",
    run: report::run,
};

pub(crate) const EXPLAIN: Applet = Applet {
    name: "explain",
    names: &[],
    summary: "say what an error code (ZBM-E0412) means and what to do about it",
    usage: "\
Usage: zbm explain           list the error codes
       zbm explain CODE      explain one, such as ZBM-E0412
",
    run: explain::run,
};

pub(crate) const MEASURE: Applet = Applet {
    name: "measure",
    names: &[],
    summary: "replay the TPM event log of this boot, or predict a boot's PCR values",
    usage: "\
Usage: zbm measure log [FILE]       replay the event log of this boot
       zbm measure unseal [FILE]    PCR 9 as ZFSBootMenu unsealed, for systemd-cryptenroll
       zbm measure predict [OPTIONS] DATASET [KERNEL]
                                    the PCR values a boot of DATASET leaves
Options of predict:
       --root DIR           where DATASET is mounted, if not in the mount table
       --command-line TEXT  the command line, if not that of this boot's log
       --bank NAME          sha1, sha256 (the default), sha384 or sha512
",
    run: measure::run,
};

pub(crate) const LOCK: Applet = Applet {
    name: "lock",
    names: &[],
    summary: "ask for the PIN of a locked image, then open the recovery shell",
    usage: "\
Usage: zbm lock shell    ask for the PIN, then open the recovery shell
       zbm lock hash     ask for a new PIN, and print the configuration that locks an image
       zbm lock status   say whether this image is locked
",
    run: lock::run,
};

pub(crate) const RUN_HOOKS: Applet = Applet {
    name: "run-hooks",
    names: &[],
    summary: "run a hook stage and its built-in steps (zfsbootmenu-run-hooks)",
    usage: "Usage: zbm run-hooks [-once] STAGE\n",
    run: run_hooks::run,
};

pub(crate) const ZFUNC: Applet = Applet {
    name: "zfunc",
    names: &[],
    summary: "run a function of the hooks' shell library (/libexec/zfunc)",
    usage: "\
Usage: zbm zfunc FUNCTION [ARGS]

Runs FUNCTION of the hooks' shell library with ARGS, on this terminal if there
is one, as /libexec/zfunc and the generated library's wrappers do.
",
    run: zfunc::run,
};

const MENU: Applet = Applet {
    name: "zfsbootmenu",
    names: &[],
    summary: "the menu: boot environments, booting and the recovery shell",
    usage: "\
Usage: zfsbootmenu

The menu, as users run it over SSH or from the recovery shell. It takes no
arguments. The exit status is 0 when the menu ended, 1 when the machine could
not be read or the terminal used, 2 for an argument.
",
    run: menu::run,
};

pub(crate) const REBOOT: Applet = Applet {
    name: "reboot",
    names: &[],
    summary: "export the writable pools, then reboot",
    usage: "\
Usage: reboot

Exports the pools imported read/write, then reboots the machine. It takes no
arguments, and returns only when the machine did not reboot: it then says why
and exits 1.
",
    run: power::reboot,
};

pub(crate) const POWEROFF: Applet = Applet {
    name: "poweroff",
    names: &[("shutdown", &[])],
    summary: "export the writable pools, then power off (also shutdown)",
    usage: "\
Usage: poweroff
       shutdown

Exports the pools imported read/write, then powers the machine off. It takes
no arguments, and returns only when the machine did not power off: it then
says why and exits 1.
",
    run: power::poweroff,
};

pub(crate) const FIRMWARE_SETUP: Applet = Applet {
    name: "firmware-setup",
    names: &[],
    summary: "export the writable pools, then reboot into the firmware's setup",
    usage: "\
Usage: firmware-setup

Exports the pools imported read/write, then reboots into the firmware's setup,
where the firmware says it can open it. It takes no arguments, and returns only
when the machine did not reboot, as on a machine that boots from a BIOS: it
then says why and exits 1.
",
    run: power::firmware_setup,
};

pub(crate) const ZFS_CHROOT: Applet = Applet {
    name: "zfs-chroot",
    names: &[],
    summary: "open a shell in a chroot of a filesystem, behind the PIN of a locked image",
    usage: "\
Usage: zfs-chroot FILESYSTEM

Opens a shell in a chroot of FILESYSTEM, with /tmp, /proc, /sys and /dev
mounted in it. On a pool imported read-only it asks first whether to import
the pool read/write. It loads the key of a locked encryption root, and in a
locked image it asks for the PIN first. The exit status is 0 once a shell
ran, or without FILESYSTEM, and 1 when no shell could be opened.
",
    run: chroot::run,
};

pub(crate) const ZKEXEC: Applet = Applet {
    name: "zkexec",
    names: &[],
    summary: "boot a kernel and initramfs in /boot of a filesystem",
    usage: "\
Usage: zkexec FILESYSTEM KERNEL INITRAMFS

Boots /boot/KERNEL and /boot/INITRAMFS of FILESYSTEM, a boot environment
(not a snapshot), as the menu boots a kernel: it loads the key of a locked
encryption root first, runs the hooks and the built-in steps, and executes
the kernel. It returns only when the boot failed, saying why, with the exit
status 1.
",
    run: kexec::run,
};

/// The commands `zbm` provides.
pub const APPLETS: &[Applet] = &[
    INIT,
    PARSE_COMMANDLINE,
    PREINIT,
    ZLUKS,
    UNLOCK,
    CONSOLE_AUTOSIZE,
    CONSOLE_FONT,
    CONSOLE_FONTS,
    KEYBOARD_LAYOUTS,
    IMAGE,
    CONSOLE,
    MENU,
    DEBUG,
    REPORT,
    EXPLAIN,
    MEASURE,
    LOCK,
    RUN_HOOKS,
    ZFUNC,
    REBOOT,
    POWEROFF,
    FIRMWARE_SETUP,
    ZFS_CHROOT,
    ZKEXEC,
];

#[cfg(test)]
mod tests {
    use zbm_types::LINKED;

    use super::*;

    /// Every name `zbm` answers to: the applets' names and their other
    /// names.
    fn every_name() -> Vec<&'static str> {
        APPLETS
            .iter()
            .flat_map(|applet| {
                std::iter::once(applet.name).chain(applet.names.iter().map(|(name, _)| *name))
            })
            .collect()
    }

    #[test]
    fn every_name_is_unique_across_applets_and_names() {
        let names = every_name();
        for name in &names {
            let count = names.iter().filter(|other| *other == name).count();
            assert_eq!(count, 1, "{name}");
        }
    }

    #[test]
    fn every_applet_has_a_usage_that_starts_with_its_name() {
        for applet in APPLETS {
            let name = applet.name;
            let starts = [format!("Usage: zbm {name}"), format!("Usage: {name}")];
            let after = starts
                .iter()
                .find_map(|start| applet.usage.strip_prefix(start.as_str()));
            assert!(
                after.is_some_and(|rest| rest.starts_with([' ', '\n'])),
                "{name}: {}",
                applet.usage
            );
            assert!(applet.usage.ends_with('\n'), "{name}");
        }
    }

    #[test]
    fn every_name_of_an_applet_is_linked() {
        for applet in APPLETS {
            for (name, first) in applet.names {
                let linked = LINKED.iter().find(|linked| linked.name == *name);
                assert_eq!(linked.map(|linked| linked.first), Some(*first), "{name}");
            }
        }
    }

    #[test]
    fn every_linked_name_is_an_applet_or_a_name_of_one() {
        for linked in LINKED {
            let first = crate::find(APPLETS, linked.name).map(|(_, first)| first);
            assert_eq!(first, Some(linked.first), "{}", linked.name);
        }
    }
}
