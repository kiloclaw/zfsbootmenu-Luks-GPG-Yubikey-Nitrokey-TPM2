# ADR 0018: The recovery tools as `zbm` applets, help from the code's tables, and `fzf` out of Rust images

- **Status:** accepted, 9 November 2026 (proposed the same day); implemented on 11 and 12 November 2026 (see the
  notes at the end)
- **Decides:** item SB6 of [ROADMAP.md](../../../ROADMAP.md) (`ROADMAP.md:617`): the form of every tool of the legacy
  `bin/`, `libexec/` and `lib/` that ADR 0017 left to SB6 (`docs/dev/adr/0017-native-hooks.md:129-130`), the LUKS
  libraries among them; which of the 54 functions still legacy (`crates/zbm/src/zfunc.rs:112-166`) get Rust code in
  SB6 (three) and which go to a new item, SB14; what SB6 leaves to SB7 (`ROADMAP.md:618`); and the firmware setup
  offered on BIOS, which ADR 0016 found and left in no item (`docs/dev/adr/0016-menu-workspace-built.md:543-544`).
  **Amends** ADR 0017 point 10 (`0017-native-hooks.md:133-136`): `zbm zfunc` asks for the PIN for `zfs_chroot` too
  (point 8)
- **Governs:** `zbm`'s applet table and its new applets (`crates/zbm/src/lib.rs`), the menu's one new option
  (`crates/zbm/src/menu.rs`), `zbm-app`'s chroot, power and firmware setup (`application.rs`, `auth.rs`, `boot.rs`),
  `zbm_tools::programs` (`ShellRoot`, `ZFS_CHROOT`, the legacy function's runner) and `zbm_tools::mount`,
  `zbm_platform::dmesg`, three natives of `zbm zfunc` (`crates/zbm-startup/src/zfunc.rs`), a pager and the help
  topics in `zbm-tui` (a `Pager` context of the key map), `zbm-host`'s image plan (`image/core.rs`, `image/lists.rs`,
  `image/shell.rs`, `image/builders.rs`), `xtask gen-docs` and `xtask metrics`, `docs/man/zfsbootmenu.7.rst`,
  `docs/man/zfsbootmenu-luks.7.rst`, the threat model's P17, `crates/DESIGN.md`, `CHANGES-FROM-LEGACY.md`, and
  ROADMAP's rows of SB7 and SB14

## Context

**What a Rust image installs.** `zbm_host::image::core::core` installs every file of the module root's `lib`,
`libexec` and `bin` but the three files of the generated library and the six of `RUST_REPLACES`
(`crates/zbm-host/src/image/core.rs:14-21`, `:93-104`), and links `/bin/zluks`, `/bin/zfsbootmenu` and
`/libexec/zfunc` to `/libexec/zbm` (`core.rs:105-108`). So a Rust image carries 26 legacy entries, 20 files and 6
links (`crates/zbm-host/src/image/snapshots/dracut-rust.snap:162-187`). Each tool sources the generated library, so
each of its library calls is a `zbm zfunc` process that replaces itself with Bash on the legacy copy
(`crates/zbm-tools/src/programs.rs:193-240`).

| Tool | What it does | What Rust has | `fzf` |
| --- | --- | --- | --- |
| `zfs-chroot FS` | asks for 10 s whether to import a read-only pool read/write (Escape) or not (Enter or the wait), and after a yes runs `set_rw_pool` and `load_key` (`legacy/zfsbootmenu/bin/zfs-chroot:32-40`; `lib/zfsbootmenu-core.sh:1455-1468`); clears the screen (`:42-43`); mounts FS (`:47`); always binds `/tmp` and `/dev` and mounts a fresh `proc` and `sysfs`, and `devpts` only where `/dev/pts` exists (`:65-79`); remounts the host's `efivarfs` to the chroot's mode, mounts one inside, and sets the host's back to read-only on exit (`:4-7`, `:71-73`; `zfsbootmenu-core.sh:2341-2360`); prints the mounts' modes (`:61`, `:75`, `:100-101`); tries `/bin/bash`, `/bin/sh`, `busybox sh` (`:81-98`); runs `chroot` from Bash (`:108`) | the menu's `Chroot` runs this script through `ShellGate` (`crates/zbm-app/src/application.rs:420-452`; `auth.rs:197-205`; `programs.rs:111`, `:130`), offered only when the file exists (`application.rs:426`, `:463`); `Mount` mounts ZFS read-only or read/write (`zfs`, `zfs_writable`), a device read-only, FAT read/write or read-only, `efivarfs` (a `bool`, no guard) and securityfs (`crates/zbm-tools/src/mount.rs:87-227`), but no other device read/write and no bind, `proc`, `sysfs` or `devpts` | no |
| `zsnapshots [FS]` | an `fzf` picker, then `zfsbootmenu-ui.sh`'s snapshot screen with duplicate, clone, rollback, diff, chroot (`bin/zsnapshots:34-88`; `lib/zfsbootmenu-ui.sh:237-510`); any filesystem (`zsnapshots:62`) | the workspace's Snapshots tab and its actions (`crates/zbm-app/src/model/action.rs:43-87`, `:165`) | yes (`zsnapshots:45`) |
| `zkexec FS KERNEL INITRAMFS` | calls `kexec_kernel "${fs} ${kernel} ${initramfs}"` (`bin/zkexec:18`), which splits on tabs (`zfsbootmenu-core.sh:365`): as read, `fs` gets all three words and the kernel is empty, so it cannot boot (not run in a VM); wrong usage exits 0 (`zkexec:9-12`) | `zbm_boot::kexec_kernel` takes `Kernel { dataset, kernel, initramfs }` (`crates/zbm-boot/src/boot/sequence.rs:151`; `crates/zbm-bootenv/src/kernels.rs:29-36`) | no |
| `mount_esp DEVICE` | `mount_block`: a block device mounted read/write with `mount DEV MNT` at `/mnt/<name>`, or where it is mounted already, printed (`bin/mount_esp:19`; `zfsbootmenu-core.sh:251-294`, `:279`) | `Mount::fat`, `Mounted::leave` (`mount.rs:185`, `:310`); `zbm report`'s partition, the ESP by default (`crates/zbm/src/report.rs:32-33`) | no |
| `reboot`, `poweroff`, `shutdown`, `firmware-setup` | by its name: exports the pools of `zpool list -H -o name` that are writable, writes `/proc/sysrq-trigger`; `firmware-setup` sets `OsIndications` with `od` and `tr` (`bin/reboot:21-50`, `:40-42`; `lib/efi-firmware-lib.sh:22`, `:38`) | `boot::reset` and `boot::firmware_setup`, `pub(crate)`, which take the menu's `&[model::Pool]` (`crates/zbm-app/src/boot.rs:245`, `:260`), from the menu (`application.rs:348-350`) | no |
| `logs`, `zlogtail` | ZFSBootMenu's errors and warnings in the kernel log, in `fzf`; removes `have_errors` and `have_warnings` (`bin/zlogtail:19-20`, `:45`) | `zbm_platform::dmesg::read_logs`, levels only, no "since" (`crates/zbm-platform/src/dmesg.rs:336-353`); the shell's banner prints it through `Printable` (`crates/zbm-startup/src/shell.rs:131-142`); `zbm debug journal` (`crates/zbm/src/debug.rs:11-13`); the Log tab, from the journal (`crates/zbm-app/src/read.rs:283-310`) | yes |
| `ztrace`, `debug` | the kernel log's debug lines (`<7>ZBM:`) that `zdebug` writes from shell code, each with its call stack, through `dmesg` and `less` (`bin/ztrace:7-44`; `lib/kmsg-log-lib.sh:16-36`) | `zbm debug commands`, the programs Rust ran, from `journal.jsonl` (`crates/zbm/src/debug/journal.rs:1-10`): not the same lines | no |
| `help` | an `fzf` viewer of the `.ansi` pages and a "System Report" that runs `zreport` (`libexec/zfsbootmenu-help:33-119`) | the menu's help view: the keys of a view (`crates/zbm-tui/src/menu/view/screens.rs:402-476`) | yes |
| `recovery-error-printer` | `PROMPT_COMMAND`: while `have_errors` exists, the errors since the last prompt's wall-clock stamp, printed raw, then only that marker removed (`libexec/recovery-error-printer:3-17`; `kmsg-log-lib.sh:102-106`) | the start-up shell's banner, escaped (`zbm-startup/src/shell.rs:131-142`); `read_logs_with_dmesg` takes a "since" but runs the `dmesg` program (`dmesg.rs:362-376`); a `Record` holds microseconds since boot (`dmesg.rs:141-142`) | no |

The rest is reached from nowhere in a Rust image but these tools or the legacy menu, which Rust images do not install
(`legacy/zfsbootmenu/bin/zfsbootmenu:149`, `:261`, `:273`): `zfsbootmenu-ui.sh`, `fzf-defaults.sh`,
`zfsbootmenu-{preview,diff,edit-kcl}`, `zpowermenu`. Nothing in a Rust image sources `zfsbootmenu-luks.sh`,
`zfsbootmenu-luks-keys.sh` or `echo-log-lib.sh`: `zbm-luks` replaces the first two (`crates/zbm-luks/src/lib.rs:4-5`),
and their one script user, `teardown.d/80-luks-close.sh`, is a built-in step in Rust images (ADR 0017).
`zfsbootmenu-input` is run by three of the legacy functions (`zfsbootmenu-core.sh:708`, `:1338`, `:1524`), and the
run-hooks shim by three more (`:389`, `:466`, `:1971`).

**`fzf`.** It is in `ESSENTIAL_BINARIES` (`crates/zbm-host/src/image/lists.rs:38`), the one list of both runtimes
(`crates/zbm-host/src/image/builders.rs:220-225`), and a missing essential program fails the build
(`builders.rs:66-69`), so a Rust image cannot be built on a host without `fzf`. The only runtime-specific list is
`LUKS_LEGACY_BINARIES` (`lists.rs:67`). `conf()`, shared by both runtimes, probes the build host's `fzf` for five
`HAS_*` flags and `which column` for `HAS_COLUMN` (`crates/zbm-host/src/image/shell.rs:60-104`, `:87`); only
`zfsbootmenu-ui.sh:44` and `fzf-defaults.sh:34` read them. The Rust menu runs no `fzf`
(`crates/zbm-vmtest/src/scenarios/runtime/signals.rs:5-6`). The C-program metric already counts it as not C
(`xtask/src/metrics/image.rs:32-35`), so its 45 do not move. The `fzf` package is about 3.7 MB installed (Ubuntu's
0.44.1, survey of 8 November); no build measured it.

**The recovery shell.** brush reads `/root/.bashrc` (`CHANGES-FROM-LEGACY.md:20-22`), the same text in both runtimes
(`shell.rs:28-44`, `:121`): `help` is `/libexec/zfsbootmenu-help -L recovery-shell` (`:41`), `trace` is `ztrace`
(`:40`), `PROMPT_COMMAND` the error printer (`:35`), and the legacy completions are sourced (`:33`). The shell's
banner says "type 'help'" (`zbm-startup/src/shell.rs:123-129`). The pages are `docs/online/*.rst` and four man pages
that `scripts/rst2help.sh` renders with Python's `rst2ansi` into `legacy/zfsbootmenu/help-files/`
(`scripts/rst2help.sh:1-16`), 30 files that both runtimes install (`core.rs:156-168`;
`dracut-rust.snap:1048-1077`). They were last made on 26 October (0c141f4f), and `zfsbootmenu.7.rst` changed on
8 November (59504b93). `docs/online/recovery-shell.rst:7-74` is the only list of the shell's commands; it names six
library functions (`zreport`, `zbmcmdline`, `set_rw_pool`, `set_ro_pool`, `mount_zfs`, `mount_efivarfs`) and the
script `mount_esp` (`:28-54`). The man page's Recovery section is the Recovery Tools view's keys
(`docs/man/zfsbootmenu.7.rst:1210-1219`), inside the block `gen_keys` writes (`:959` to `:1435`). RL2 removes
`docs/online/` (`ROADMAP.md:735`) one release after phase 7 (`:820`, `:873`); phase 4 ends with no Bash or Python under
`scripts/` (`ROADMAP.md:801-803`), and VE7, in phase 4, turns `scripts/` into `xtask` commands (`:695`, `:799`).

**The library.** `zbm zfunc` runs Rust for two functions (`crates/zbm-startup/src/zfunc.rs:27-30`) and the legacy
function for 54 (`crates/zbm/src/zfunc.rs:112-166`). Its Bash runner sources `/lib/zfsbootmenu-ui.sh` and exits 1
without it (`programs.rs:202-207`), though none of the 54 calls a function of it. The legacy `zfs_chroot` runs
`/bin/bash -c "zfs-chroot ${fs}"` (`zfsbootmenu-core.sh:2085`), a shell outside the gate, with `${fs}` split into
words: only `emergency_shell` is the gate's inside the runner (`programs.rs:210-212`), so P17
(`docs/dev/threat-model.md:127`) and `crates/DESIGN.md:587` claim more than holds. `zreport` has no Rust code; the
import stage keeps a placeholder for `zdebug "$( zreport )"` until it has (`crates/zbm-pool/src/stage.rs:84-89`), and
it lists hooks by their files, not the built-in steps (`CHANGES-FROM-LEGACY.md:185-190`). ADR 0017 names `zreport`
and `zfs_chroot` as SB6's (`0017-native-hooks.md:178-180`) and leaves the rest to "SB6 and SB7" (`:282`;
`crates/DESIGN.md:590`; `ROADMAP.md:616`). SB7 needs the list empty and is sized S (`ROADMAP.md:618`).

**What is decided already.** `fzf` leaves Rust images with the recovery tools (decision 3, `ROADMAP.md:906`).
Applets keep the legacy names, arguments and subcommands (`crates/DESIGN.md:901`). SB6 owns the library but the hook
contract's three files, the LUKS libraries included (ADR 0017 point 9, `0017-native-hooks.md:129-130`;
`crates/DESIGN.md:533-537`). `zbm zfunc` asks for the PIN for nothing but `emergency_shell`, since hooks are the
image's or measured and the recovery shell is behind the gate (ADR 0017 point 10, `0017-native-hooks.md:133-136`).
The menu's `Chroot` runs `zfs-chroot` until SB6 makes it an applet (ADR 0016 point 1,
`0016-menu-workspace-built.md:63-65`); the classic menu gains no view (point 10, `:112-115`); the Rust menu's help is
its help view (point 17, `:147-151`). The lock keeps the shell, the chroot, any boot but the default and every
change, and leaves looking around, help, power and firmware setup open (`docs/dev/adr/0010-locked-menu.md:34-39`;
`crates/zbm-app/src/model/action.rs:279-303`). One key map for input and help (P7, `threat-model.md:117`); no outside
text raw on a terminal (P13, `:123`); every shell through the gate (P17). Bash and the text tools stay for hooks and
the shell (`docs/dev/adr/0005-c-dependencies.md:54-58`; `ROADMAP.md:341`).

## Decision

**Each recovery tool becomes a `zbm` applet, linked under its legacy names, on the Rust code the menu already uses;
the chroot is prepared in Rust and its shell started by the gate; `help` reads the tables the man page is written
from, in a pager whose keys come from the key map; `zfs_chroot`, `zreport` and `zbmcmdline` get Rust code; and Rust
images lose `fzf`, the legacy tools, the LUKS libraries and the help pages, keeping of the legacy payload only what
the 51 remaining functions need. Those go to a new item, SB14; the legacy runtime does not change.**

### Scope

1. **The menu views are UI10's; SB6 adds none.** The Recovery Tools view already lists Shell, Chroot, Log, Pools,
   Firmware Setup, Reboot and Power Off (`crates/zbm-tui/src/menu/model/recovery.rs:10-18`), the workspace has the
   Snapshots tab and the help view. `mount_esp`, `zkexec` and the system report stay shell commands, and the classic
   menu gains nothing (ADR 0016 point 10). SB6's one change to the views is the firmware setup offered only where it
   can open (point 11). Question 5.
2. **The legacy runtime does not change until RL2:** legacy images keep `bin/`, `libexec/`, `lib/`, `fzf`, their
   `.bashrc` and completions, the `.ansi` help pages and the `HAS_*` flags of `/etc/zfsbootmenu.conf`
   (`shell.rs:28-44`, `:60-104`; `core.rs:156-168`); `dracut-legacy.snap` and `mkinitcpio-legacy-miser.snap` stay
   as they are.

### Applets and names

3. **`Applet` gains `names`,** the other names it answers to through a link, each with the arguments it puts first,
   and **`usage`,** its usage text, which `--help`, `zbm help` and the man page print. `run_with` finds an applet by
   its name or one of its names (`crates/zbm/src/lib.rs:226-235`). Rust images link each name to `/libexec/zbm` in
   `/bin`, as `zluks` is (`core.rs:105-108`). The applets: `zfs-chroot`, `zkexec`, `mount_esp`, `reboot`, `poweroff`
   (also `shutdown`), `firmware-setup`, `logs` (also `zlogtail`, and `ztrace` as `logs --trace`, point 12),
   `zsnapshots` and `help`. While a step has not removed a legacy file, its name joins `RUST_REPLACES`
   (`core.rs:14-21`), so a link replaces it.
4. **`ztrace` stays a program,** so scripts and `bash -c` keep it, and `trace` stays the `.bashrc` alias of it, as in
   the legacy `.bashrc` (`shell.rs:40`). `/bin/debug` goes: a link of that name starts `zbm debug`
   (`lib.rs:200-205`, `:226-235`), not `ztrace`; a `CHANGES-FROM-LEGACY.md` entry says so. `zbm debug commands` stays
   what it is, the programs Rust ran.
5. **No applet asks for the PIN but those that open a shell.** Whoever types a command is at a shell that passed the
   gate, runs a hook that is the image's or measured, or logged in over SSH and is root already (R8,
   `threat-model.md:143`); the image holds `kexec`, `mount` and `chroot` either way. This is ADR 0017 point 10's rule
   for the new applets. The menu keeps its lock (`Action::is_kept_by_lock`, `application.rs:225`): the chroot, a
   boot by kernel and every change stay kept; power, firmware setup, logs and help stay open.

### The chroot

6. **`zbm_app::chroot` prepares a chroot** of a filesystem, in the legacy script's order: for a filesystem on a
   read-only pool it asks through `Interaction` whether to import the pool read/write, and stays read-only after
   10 seconds without an answer; after a yes it makes the pool writable (`crates/zbm-pool/src/writable.rs:125`) and
   loads the filesystem's key, as `zfs-chroot` ran `set_rw_pool` and `load_key` (`zfs-chroot:37-38`). It clears the
   screen, mounts the filesystem through `Mount` (`zfs`, or `zfs_writable` on a writable pool and not a snapshot),
   always binds `/tmp` and `/dev`, mounts a fresh `proc` and `sysfs`, and `devpts` only where the filesystem has
   `/dev/pts` (`zfs-chroot:65-79`). On EFI it remounts the host's `efivarfs` to the chroot's mode, mounts one in the
   chroot, and sets the host's back to read-only when the chroot ends (`:4-7`, `:71-73`). Each mount is a `Mounted`,
   dropped in reverse order. It prints, through `Printable`, the notices of the mounts' modes (`:61`, `:75`, `:100`),
   and finds the shell as `zfs-chroot` did: `/bin/bash --norc`, `/bin/sh`, then `busybox sh`, each tried in the
   chroot. `Mount` gains `bind`, `proc`, `sysfs` and `devpts`, each a `Mounted`, and an `efivarfs` that remounts and
   gives a guard that sets the mode back. The menu shows the question as a dialog; the applet asks on its terminal.
7. **`ShellGate::chroot` starts the prepared chroot's shell,** with `chroot(2)` in the child before `exec`, as SB1
   moved `setsid` into the child (`ROADMAP.md:612`), `PS1` naming the filesystem and no `PROMPT_COMMAND`. It stays
   the gate's one start (`programs::interactive_shell`, `programs.rs:124-137`; `auth.rs:207-210`), so `shell_starts`,
   which counts the call sites of `interactive_shell` and `run_recovery_shell` (`xtask/src/metrics/scan.rs:33`), does
   not change. `ShellRoot::Chroot` becomes the prepared chroot, and `ZFS_CHROOT` (`programs.rs:111`) goes.
8. **Three ways in, one code.** The menu's `Chroot` (`application.rs:420-452`), offered whenever the machine has a
   filesystem, with no test of a file (`application.rs:426`, `:463`); from the SSH menu the session performs it on
   the client's terminal (`crates/zbm-tui/src/menu/machine/remote.rs:162-186`; `action.rs:203-215`). The applet
   `zfs-chroot FS`, with its process's authority, or in a locked image the PIN asked on its terminal first, as
   `zbm lock shell` does (`auth.rs:280-301`). The library's `zfs_chroot`, native in `zbm zfunc`, which opens the
   same way, so a hook's chroot goes through the gate (P17) and the unquoted `bash -c` goes. **This amends ADR 0017
   point 10:** `zbm zfunc` asks for the PIN, in a locked image, for `zfs_chroot` as for `emergency_shell`. So the
   shell's `zfs-chroot` asks for the PIN again, as the shell's `emergency_shell` does today
   (`crates/zbm-startup/src/zfunc.rs:105-111`): the proof is a process's, and forgotten when its shell ends
   (`auth.rs:303-319`); and a hook's `zfs_chroot`, which opens an interactive shell, waits for a PIN in a locked image,
   a `CHANGES-FROM-LEGACY.md` entry. Question 4.

### Booting a kernel by its files

9. **`zkexec FS KERNEL INITRAMFS`** boots `/boot/KERNEL` and `/boot/INITRAMFS` of FS through `zbm-app`'s boot with
   the built-in steps (ADR 0017 point 2), a third caller of `zbm_boot::kexec_kernel` that the callers' test covers;
   it loads the key and asks what the menu's boot asks, and the teardown context's `loaded_by` is set
   (`0017-native-hooks.md:265-267`). A wrong number of arguments prints the usage and exits 2; the legacy script
   exited 0 (`zkexec:9-12`). The library's `kexec_kernel` stays legacy, for SB14.

### Mounting a partition

10. **`mount_esp [DEVICE]`** mounts DEVICE, or with no argument the EFI system partition that `zbm report` writes to
    (`report.rs:32-33`), read/write at `/mnt/<name>` through a new `Mount::block`, `mount DEVICE DIR` with the type
    found by `mount` as `mount_block` did (`zfsbootmenu-core.sh:279`) and as SB3 left block devices
    (`ROADMAP.md:614`), and leaves it mounted (`Mounted::leave`). A device mounted already is printed where it is; a
    busy `/mnt/<name>` is refused, as `mount_block` did. It prints the mount point. `mount_block` stays legacy, for
    SB14.

### Power and firmware setup

11. **`reboot`, `poweroff` and `firmware-setup`** run `boot::reset` and `boot::firmware_setup`, made a use case of
    `zbm-app` the applets reach, and exit 1 saying why when they return. Both take the imported pools' names in place
    of the menu's `&[model::Pool]` (`boot.rs:200-206`, `:245`, `:260`); the applets list them through the `Zfs` port,
    as the legacy script ran `zpool list -H -o name` (`bin/reboot:40-42`), and the writable ones are exported as
    before. **Firmware setup is offered only where the firmware can open it:** `offers` (`application.rs:462`) reads
    the check that `request_firmware_setup` makes (`boot.rs:215-226`), so the menu, the palette and the Recovery
    Tools view no longer offer it on BIOS (ADR 0016, found and left). `efi-firmware-lib.sh`, with its `od` and `tr`,
    leaves Rust images.

### Logs

12. **`logs [--level LEVELS] [--trace]`** (also `zlogtail`) prints ZFSBootMenu's messages in the kernel log at the
    levels asked, errors and warnings by default, read from `/dev/kmsg` (`dmesg.rs:220`, `:336`), each line through
    `Printable` (P13). It removes `have_errors` and `have_warnings`, as `zlogtail` did (`zlogtail:19-20`). With
    `--trace`, which `ztrace` is, it shows the debug lines `zdebug` writes from shell code with their call stacks, in
    `ztrace`'s format (`ztrace:19-44`; `kmsg-log-lib.sh:16-36`): hooks and the remaining legacy functions keep
    writing them. On a terminal the help viewer's pager (point 15) shows them; otherwise they are printed. The kernel
    log, not the journal: the library's log functions stay shell code that writes to `/dev/kmsg`
    (`0017-native-hooks.md:139-142`), so only the kernel log has the lines of hooks and of the legacy functions. The
    journal stays `zbm debug journal`, `zbm debug commands` and the Log tab.
13. **`logs --prompt` is `PROMPT_COMMAND`:** while `have_errors` exists, the errors since the last prompt, escaped,
    as the banner prints them; then it removes that marker alone, as `recovery-error-printer` did
    (`recovery-error-printer:10-14`), and writes its stamp. The "since" is new Rust code: `read_logs` takes none
    (`dmesg.rs:336-353`), and the legacy stamp is a wall-clock time for `dmesg --since` (`recovery-error-printer:17`;
    `kmsg-log-lib.sh:102-106`). The applet keeps instead the microseconds since boot of the last record it read
    (`Record::micros`, `dmesg.rs:141-142`) in `${BASE}/errors_since`, which nothing else in a Rust image reads, and
    shows the records after it. `recovery-error-printer`, which printed the kernel's text raw
    (`recovery-error-printer:12`), leaves Rust images. `print_kmsg_logs` stays legacy, for SB14.

### Help

14. **`zbm help [TOPIC]`,** the shell's `help`, shows text from the tables the man page is written from:
    - `commands`: the recovery shell's commands, each applet linked into `PATH` with its usage, and the six library
      functions the shell's page names as commands (`zbmcmdline`, `set_rw_pool`, `set_ro_pool`, `mount_zfs`,
      `mount_efivarfs`, `zreport`; `recovery-shell.rst:28-54`) with theirs;
    - `keys`: the menu's keys, from `zbm-tui`'s `keymap/manual.rs`, the source of `zfsbootmenu(7)`'s key section;
    - `options`: the kernel command line's options, from `zbm-config`'s table (`xtask/src/gen_docs.rs:1-17`).

    With no topic it shows `commands`, then names the other topics.
15. **The pager** is a view of `zbm-tui` on a text, with its own context in the key map, **`Pager`:** the help view's
    scroll and page keys (`zfsbootmenu.7.rst:1232-1294`), `/` to filter the lines, and Esc, `h`, `q` and Ctrl-C to
    leave; not the tab keys or F1, which do nothing there. The `Help` context has no command, `Filter` included
    (`crates/zbm-tui/src/menu/keymap/commands_tests.rs:129-131`), so it cannot be reused as it is. `gen_keys` writes
    the new context into the man page, and the keys the pager lists are those it reads (P7). Without a terminal, or
    with `TERM=dumb`, the text is printed as lines, which the line view's users, `speakup`, a Braille display through
    the kernel's `/dev/vcsaN`, and `grep` read (`docs/dev/adr/0011-accessibility.md:72-77`, `:87-90`).
16. **`xtask gen-docs` writes the commands into `zfsbootmenu(7)`** through a third example, `gen_commands` of `zbm`,
    beside `zbm-config`'s `gen_docs` and `zbm-tui`'s `gen_keys` (`gen_docs.rs:17`), into a new `commands` block of its
    own: a section "The Recovery Shell" beside "The Shell Library" (`zfsbootmenu.7.rst:1698`), not the Recovery
    section, which is in the `keys` block (`:959` to `:1435`); `--check` fails on a stale page. It replaces
    `docs/online/recovery-shell.rst` for the Rust runtime. The legacy runtime's `.ansi` pages stay as committed;
    `scripts/rst2help.sh` is VE7's, which ports or drops it with the rest of `scripts/` before phase 4 ends
    (`ROADMAP.md:695`, `:801-803`). RL2 removes `docs/online/`.
17. **Rust images install no help pages** (`core.rs:156-168`) and no legacy viewer, and do not show the five pages of
    `fzf` screens. **`zreport` is native,** with the sections the legacy function printed
    (`zfsbootmenu-core.sh:2134-2210`): the release with the initramfs builder and `uname -srm` (`:2139-2174`), the
    command line, the hooks and the built-in steps with their state, the ZFS modules' versions, `zfs version`
    (`:2202-2203`), `zpool list` and the datasets' properties; so it lists the steps
    (`CHANGES-FROM-LEGACY.md:185-190` goes, as does `zfsbootmenu.7.rst:1607`'s last sentence). Names, hook paths and
    the command line go through `Printable` (P13), which the legacy function printed raw (`:2176-2210`). The import
    stage's placeholder becomes the report at debug level (`stage.rs:84-89`). **`zbmcmdline` is native,** on the
    same code as the report's command line.

### Snapshots

18. **`zsnapshots [ENVIRONMENT]`** runs the menu with its one new option, `zfsbootmenu --snapshots [ENVIRONMENT]`
    (the menu takes none, `menu.rs:271-274`): the workspace opens on the Snapshots tab, at the environment's
    snapshots when one is named. The session serves it as any menu run from the shell (`signals.rs:6-9`), so its
    changes keep the lock. The starting tab is the client's own view state and does not cross the session's protocol,
    whose messages carry the snapshot, actions and answers (`crates/zbm-app/src/remote.rs:1-40`, `:72-110`), so the
    protocol's version stays. A filesystem that is no boot environment is refused, naming `zfs list -t snapshot -r`:
    the workspace shows the snapshots of environments only (`read.rs:93`). With `zbm.menu=classic` the option is
    refused, as the classic menu gains no view.

### The library

19. **SB6 gives three functions Rust code: `zfs_chroot` (point 8), `zreport` and `zbmcmdline` (point 17)**, the ones
    an SB6 tool changes. Each keeps the legacy function's status, implicit outputs and log lines, and its output but
    for the escapes of P13; each has a parity test that says so (`0017-native-hooks.md:280-283`). The test's list falls
    from 54 to 51. The applets call the Rust code directly and none of the other functions, and the users of those
    keep working on the legacy runner (`contrib/keycache.sh:50`; `contrib/snapshot-teardown.sh:28`, `:33`). Question 2.
20. **The LUKS libraries leave Rust images:** `zfsbootmenu-luks.sh`, `zfsbootmenu-luks-keys.sh` and `echo-log-lib.sh`
    are no part of the hook contract, which is three files (`crates/DESIGN.md:533-537`), and nothing in a Rust image
    sources them (Context). `zfsbootmenu-luks(7)`'s files section names them for the legacy runtime alone
    (`docs/man/zfsbootmenu-luks.7.rst:291-293`), and a `CHANGES-FROM-LEGACY.md` entry says so.

### The image

21. **`fzf` leaves Rust images.** It moves from `ESSENTIAL_BINARIES` to a new list, `LEGACY_BINARIES`, installed in
    legacy images only, as `LUKS_LEGACY_BINARIES` are (`lists.rs:63-67`): the dracut builder (`builders.rs:220-225`)
    and the mkinitcpio builder take it for the legacy runtime alone, and the image tests name it
    (`crates/zbm-host/src/image/tests.rs:21`, `:68`). The metric leaves the list out with the other legacy lists and
    drops `fzf` from `NOT_C` (`image.rs:20-35`). `conf()` splits by runtime: Rust images' `/etc/zfsbootmenu.conf`
    loses `HAS_REFRESH`, `HAS_DISABLED`, `HAS_BORDER`, `HAS_COLORS`, `HAS_RAW` and `HAS_COLUMN`, which only legacy
    files read (`zfsbootmenu-ui.sh:44`; `fzf-defaults.sh:34`), and their build probes no `fzf` and looks for no
    `column` for it (`shell.rs:84-104`). Legacy images keep both, unchanged.
22. **Rust images install, of the module root's `lib`, `libexec` and `bin`, only what SB7 removes:** the three library
    files, generated, with their legacy copies (`core.rs:124-154`), and `libexec/zfsbootmenu-input`. A list of what
    Rust images take replaces `RUST_REPLACES`' exclusion (`core.rs:93-104`). The legacy function's runner stops
    sourcing `zfsbootmenu-ui.sh` (`programs.rs:202-207`) in the same commit.
23. **Rust images get their own `.bashrc`:** `help` is `zbm help`; `trace` is `ztrace`; `PROMPT_COMMAND` is
    `/libexec/zbm logs --prompt`; completions, which `zbm-host` writes from the applets' table and which list through
    a hidden `zbm complete KIND`, keep what the legacy ones offered (`zfsbootmenu-completions.sh:7-140`): filesystems
    and snapshots for `zfs-chroot` and `zfs_chroot` (`:13-24`) and `mount_zfs`, filesystems for `zsnapshots` and
    `zkexec`'s first word, read-only pools for `set_rw_pool` and writable ones for `set_ro_pool` (`:26-54`), for
    `mount_esp` the partitions of the ESP's GPT type `c12a7328-f81f-11d2-ba4b-00a0c93ec93b` not mounted yet
    (`:74-87`), from `/sys/class/block` and udev's records (`zbm_tools::devices`), and `ro`/`rw` for
    `mount_efivarfs`; but no kernels, which the legacy completion listed by mounting the environment (`:105-134`).
    `clear` and `reset` stay on `tput` (`shell.rs:38-39`).
24. **No other program leaves in SB6.** `cut`, `tr`, `wc`, `column`, `less`, `tput`, `stty`, `od`, `tac`, `chroot`,
    `dmesg`, `lsblk` and kmod are for SB7's own list to decide, between ADR 0005 point 7 (Bash and the text tools for
    hooks and the shell) and `ROADMAP.md:392-393`; users' hooks run them (a VM test's hook runs `tr`,
    `signals.rs:30`). CD3's note (`ROADMAP.md:674`) changes from "SB6 and SB7" to SB7.

### What is left to SB14 and SB7

25. **SB14, a new item: the rest of the hook library in Rust,** the 51 functions still legacy, each with a parity
    test; the fate of the seven only the legacy runtime calls (found and left, below); and the Log tab's lines of
    hooks from the kernel log (found and left). Sized L; it needs SB5 only, since no SB6 tool calls those functions,
    so it may run beside SB6. It joins phase 4's list (`ROADMAP.md:799`). Question 3.
26. **SB7** needs SB6 and SB14, and takes out the legacy copies and `zfsbootmenu-input`, the mkinitcpio `pre-init`
    scripts that Rust images still install (`builders.rs:309-313`), which lie outside its `{lib,libexec,bin}`, and
    gives Rust images their own binary list, which the metric reads. SB6h rewrites SB7's row to say so; SB7 stays S.

### Steps

27. Each step links its names, adds its legacy files to `RUST_REPLACES`, and brings its own `CHANGES-FROM-LEGACY.md`
    entries and man-page lines, so each checks on its own:
    - **SB6a,** the applet table's `names` and `usage` (point 3); power and firmware setup, firmware setup not offered
      on BIOS (point 11);
    - **SB6b,** the chroot, the new `Mount` methods and `zfs_chroot` native, with ADR 0017 point 10 amended and P17
      and `crates/DESIGN.md:587` true again (points 6 to 8);
    - **SB6c,** `zkexec` and `mount_esp` (points 9 and 10), and `zfsbootmenu.7.rst:1607` and `:1696` on `zkexec`;
    - **SB6d,** `logs`, `ztrace` and the prompt's errors (points 4, 12 and 13);
    - **SB6e,** `zbm help`, the `Pager` context, `gen_commands`, `zreport` and `zbmcmdline` (points 14 to 17), and
      `zfsbootmenu.7.rst:1705`'s list of functions that run Rust;
    - **SB6f,** `zsnapshots` and the menu's option (point 18);
    - **SB6g,** after SB6d, SB6e and SB6f, which replace `fzf`'s users and the viewer: the image, `fzf` out, `conf()`
      split, the list of what Rust images take, the LUKS libraries out, the runner without `zfsbootmenu-ui.sh`, the
      Rust `.bashrc` and completions (points 20 to 24);
    - **SB6h,** the record: `crates/DESIGN.md` (`:533-537`, `:590`, `:817`, `:901`), ROADMAP (SB6 done, the rows of
      SB7 and SB14, phase 4's list, CD3's note), `CHANGES-FROM-LEGACY.md:827`, the checks and the VM handoff.

## Consequences

- The recovery tools and the shell's documented commands but four library functions (`set_rw_pool`, `set_ro_pool`,
  `mount_zfs`, `mount_efivarfs`, SB14's) run Rust; no Bash runs for the chroot, power, logs or help, and `zkexec`
  boots, which the legacy script could not.
- One gate for every chroot; `shell_starts` stays as it is. In a locked image the shell's `zfs-chroot` asks for the
  PIN a second time, and a hook's `zfs_chroot` waits for one.
- Rust images lose `fzf`, about 3.7 MB, the help pages, about 246 KB, the LUKS libraries and the legacy tools; their
  builds need no `fzf` on the host. The golden plans hold paths, not sizes: the maintainer compares the image's size
  before and after SB6g (`crates/DESIGN.md:817`).
- `help` is a reference, not a manual: the commands, the keys and the options. The LUKS guides and the man pages'
  prose stay on the host and online; `zluks --help` stays.
- Users see changes, each a `CHANGES-FROM-LEGACY.md` entry from its step: `zsnapshots` takes environments only;
  `zkexec` exits 2 on wrong usage; `logs` and `ztrace` show the log in the pager, not `fzf` or `less`; `help` has
  three topics; `/bin/debug` is gone; `mount_esp` finds the ESP itself; `zreport` lists the steps and escapes names;
  a hook's `zfs_chroot` asks for the PIN in a locked image; firmware setup is not offered on BIOS; the LUKS libraries
  and the `HAS_*` flags are gone from Rust images; the completions list no kernels.
- The three natives cost a process per call, as the wrappers did, but no Bash.
- SB7 stays files and lists, behind a new item of 51 functions.

## Alternatives considered

- **Menu views only, the shell commands gone.** Fewer applets, but `crates/DESIGN.md:901` keeps the names, and a
  shell with no menu, on a serial line or over SSH, would lose them.
- **The legacy tools on the generated library until SB7.** Least work, but `fzf` stays for `zsnapshots`, `logs` and
  `help`, against decision 3, and `zfs_chroot` stays outside the gate.
- **The chroot as a program the gate runs** (`/bin/zfs-chroot` a link to `zbm`). Keeps `ShellRoot` as it is, but
  the program starts the shell itself, a start `shell_starts` does not see, and it cannot tell the gate's start
  from a user's.
- **Help from the `.ansi` pages,** moved out of `legacy/`, in a Rust pager. The same pages as now, but made by
  Python's `rst2ansi` under `scripts/`, which phase 4's exit forbids, and stale already.
- **Help through `less` or `man`.** `less` is an optional C program (`lists.rs:44`); `man` needs `mandoc` or `groff`,
  new C programs, while the CD1 count may only fall.
- **The pager on the `Help` context.** No new context, but `Help` has no filter and no `q`, and lists tab keys that do
  nothing in a pager.
- **`logs` from the journal.** The journal does not hold the lines of hooks and of the legacy functions.
- **`ztrace` as an alias of `zbm debug commands`.** One applet fewer, but it shows other lines, and an alias does not
  expand in a script or `bash -c`.
- **17 or all 54 functions in SB6.** No SB6 tool calls the other functions, and each native costs a parity test;
  SB5d managed two (`0017-native-hooks.md:276-291`).
- **`zsnapshots` of any filesystem.** The workspace holds the snapshots of environments; a column for other
  filesystems is new view work, for the classic menu too.

## How it is checked

- **Unit tests:** the applet table (names unique across applets, a usage for each, each name linked in Rust plans,
  `ztrace` putting `--trace` first); the chroot on `FakeSystem` (the question read-only by default, `load_key` after
  a yes, the mounts, bind or fresh, and their order, `devpts` only with `/dev/pts`, read-only for a snapshot and a
  read-only pool, the host's `efivarfs` set back, unmounted in reverse when dropped (`mount.rs:316`), the shell
  found, refused without authority); `zfs_chroot` through the gate and asking for the PIN when locked, as
  `the_emergency_shell_goes_through_the_gate`; `zkexec` with the steps table and its usage; `mount_esp` (reused,
  busy, not a block device, the ESP found); `reset` with the listed pools; `offers` without
  `OsIndicationsSupported`; `logs` (escaped, both markers removed, `--trace`'s format); `logs --prompt` (nothing
  without `have_errors`, only that marker removed, records after the stamp); `help` (every linked applet listed,
  plain text with `TERM=dumb`, the `Pager` context's keys in the key map and the man page); a parity test per native,
  escapes aside; the list of legacy functions at 51.
- **Golden plans:** `dracut-rust.snap`, `dracut-rust-release.snap` and `mkinitcpio-rust.snap` change (`fzf` at
  `dracut-rust.snap:112`, the legacy entries at `:162-187` but the list's, the pages at `:1048-1077`, the `.bashrc` at
  `:1117`); `dracut-legacy.snap` and `mkinitcpio-legacy-miser.snap` do not change; a test that no Rust plan names
  `fzf`, a help page or a module-root file outside the list.
- **Checks:** `xtask gen-docs --check` for the commands and the `Pager` keys; `xtask metrics` (`shell_starts`
  unchanged, `c_programs` still 45); `cargo xtask lint`, `test`, coverage and parity. `contrib/` is not edited, and
  the generated library's comparison with the legacy files still passes (the design's row F, `crates/DESIGN.md:817`).
- **VM tests.** None runs `zfs-chroot`, `zkexec`, `zsnapshots` or `mount_esp` today (nothing in
  `crates/zbm-vmtest/src`). New, **`runtime-rust-recovery`** in the `runtime` group: from the menu's Recovery Tools,
  Chroot of the environment reads a file of it and exits; on the BIOS machine the view lists no Firmware Setup and
  the shell's `firmware-setup` says why and exits 1; in the shell, `help | cat` lists `zfs-chroot`, `help` opens the
  pager and `q` leaves it, `logs | cat` prints, `mount_esp` on a FAT disk (attached as `runtime-rust-saved` attaches
  one) prints `/mnt/<name>`, `zreport` lists a built-in step, `zfs-chroot ENV` opens and exits, `zsnapshots ENV` opens
  the Snapshots tab and leaves, `command -v fzf` fails, and last `zkexec ENV KERNEL INITRAMFS` boots the environment.
  About 6 minutes, estimated. **`integrity-locked`** gains a step: after the PIN, the shell's `zfs-chroot ENV` asks
  for it again (about a minute more). **`menu-boot`**'s tour checks that the palette offers no Reboot into Firmware
  Setup on BIOS, which it skips today (`crates/zbm-vmtest/src/scenarios/menu/steps/tables.rs:424-428`).
  **`firmware-boot`**, the only group on OVMF (`crates/zbm-vmtest/src/qemu.rs:158-163`;
  `scenarios/firmware.rs:283`), gains a step: the menu offers Firmware Setup (about a minute more). Run again,
  unchanged: `runtime-rust-console`, `runtime-rust-signals`, `runtime-rust-hooks` and `runtime-rust-saved`.
- **The maintainer's run,** in four lanes, each with its `-setup` first: `runtime-setup`, `runtime-rust-recovery`,
  `runtime-rust-console`, `runtime-rust-signals`, `runtime-rust-hooks`, `runtime-rust-saved`; `integrity-setup`,
  `integrity-locked`; `menu-setup`, `menu-boot`; `firmware-setup`, `firmware-boot`. `ZBMTEST_REUSE=1` is safe: the
  images' sources change, so they are built again (`docs/luks/testing.rst:81-85`). The build container does not
  change; it keeps `fzf` for legacy images (`packaging/container/setup-image.sh:44`). The image's size is compared
  with the parent's.

## Questions for the maintainer

1. **The help viewer.** Proposed: topics from the code's tables (commands, keys, options) in a pager of `zbm-tui`
   with its own key context, lines without a terminal, and no pages in Rust images; `rst2help.sh` to VE7. The
   alternatives keep the `.ansi` pages, moved out of `legacy/`, in a Rust pager, or print a page through `less`.
   Recommended as proposed: one source keeps the help, the man page and the code in step, needs no Python for Rust
   images, and phase 4's exit holds.
2. **Which functions SB6 makes native.** Proposed: `zfs_chroot`, `zreport` and `zbmcmdline`, which an SB6 tool
   changes. The alternatives are the 17 the tools and the shell's page touch (with `kexec_kernel`, `mount_block`,
   `is_writable`, `export_pool`, `print_kmsg_logs`, `set_rw_pool`, `mount_zfs`, ...), or all 54. Recommended as
   proposed: no applet calls the others, each native costs a parity test, and SB6 stays L.
3. **The 51 remaining functions.** Proposed: a new item, SB14 (L, needs SB5, phase 4), which SB7 needs; SB7 stays S.
   The alternatives make SB7 L with the functions in it, or let SB7 keep the legacy copies on Bash for the hook
   contract, against "without the legacy payload". Recommended as proposed: the sizes stay honest, SB14 can run
   beside SB6, and SB7 stays files and lists.
4. **The PIN at a chroot in a locked image.** Proposed: `zbm zfunc`'s `zfs_chroot` and the `zfs-chroot` applet ask
   for it as `emergency_shell` does, so the shell's `zfs-chroot` asks again and a hook's waits for it (amending ADR
   0017 point 10). The alternative is a proof the gated shell hands its children (an inherited descriptor the applets
   accept), so the shell's chroot asks nothing. Recommended as proposed: it is the rule `emergency_shell` follows
   today from the same shell, no credential crosses `exec`, and it costs a PIN per chroot in locked images only.
5. **New Recovery Tools entries.** Proposed: none; the view keeps UI10's seven entries, and the report, `mount_esp`
   and `zkexec` stay shell commands. The alternative adds "System Report" (the legacy viewer had one) and "Mount the
   ESP". Recommended as proposed: the menu has the Pools tab and the Log tab, `zbm report` saves a report, and a
   mounted ESP is of use only in the shell.
6. **`zsnapshots`.** Proposed: the menu on the Snapshots tab, environments only. The alternatives drop it, as
   `zfsbootmenu` reaches the tab, or take any filesystem with a new column. Recommended as proposed: one option of
   the menu keeps a documented command, and other filesystems have `zfs list -t snapshot`.
7. **`logs`.** Proposed: ZFSBootMenu's errors and warnings in the kernel log, and `ztrace` as its `--trace`. The
   alternative is the journal (`zbm debug journal --level warn`, `zbm debug commands`). Recommended as proposed: the
   kernel log has the lines of hooks and of the legacy functions, and the journal has its own commands and the Log tab.
8. **Completions.** Proposed: written by `zbm-host` for the commands' names, listing what the legacy ones listed but
   kernels. The alternative drops them, leaving brush's file names. Recommended as proposed: they are small, ADR 0017
   gives them to SB6, and the legacy one that mounts an environment on Tab goes.

## Where the work stands

Proposed on 9 November 2026, at c7b6d396 (one commit past 5c7b5efa, which changes only the expectations of
`crates/zbm-vmtest/src/scenarios/menu*`); nothing is built. On acceptance the record gets a line in
`docs/dev/adr/README.md`'s table, ADR 0017 a note that point 10 is amended, and SB6's row in ROADMAP.md points to it.
SB6 stays L: eight steps, nine applets, a pager with its key context, a chroot in Rust with four new mounts,
generated completions, `gen_commands`, `--snapshots`, three natives with parity tests and a new VM test. What is not
SB6's goes to its item: the 51 functions to SB14, the files, `pre-init` and the programs' lists to SB7, the
`scripts/` to VE7, `docs/online/` to RL2.

- **Found while surveying,** each settled by a point above:
  - the legacy `zkexec` joins its arguments with spaces where `kexec_kernel` splits on tabs, so it cannot boot
    (as read, `zkexec:18`, `zfsbootmenu-core.sh:365`; no VM test runs it): point 9;
  - the legacy `zfs_chroot` opens a shell outside the gate, against P17 and `crates/DESIGN.md:587`: point 8;
  - `recovery-error-printer` prints the kernel's text raw at every prompt, against P13: point 13; `zreport` prints
    names raw: point 17;
  - `CHANGES-FROM-LEGACY.md:827` still names R5 and R6 for `fzf`'s exit: SB6h.
- **Found and left:**
  - seven functions only the legacy runtime calls (`write_hostid`, `match_hostid`, `log_unimportable`,
    `check_for_pools`, `import_zbm_hooks`, `set_default_kernel`, `rewind_checkpoint`), which the contract still covers
    (`crates/DESIGN.md:537`): mapped onto the Rust code or dropped, SB14's;
  - the Log tab reads the journal alone (`read.rs:283-310`), so the errors of hooks, which reach the kernel log only,
    do not show in it: SB14's;
  - the legacy runtime's help pages were made on 26 October, before `zfsbootmenu.7.rst`'s change of 8 November, so
    its key section is not in them: frozen as they are, VE7's with `rst2help.sh`.

## Answers of the maintainer

Accepted as proposed, on 9 November 2026:

1. **The help viewer:** topics from the code's tables (commands, keys, options) in a pager of `zbm-tui` with its own
   key context, lines without a terminal, no pages in Rust images; `rst2help.sh` to VE7.
2. **Which functions SB6 makes native:** `zfs_chroot`, `zreport` and `zbmcmdline`.
3. **The 51 remaining functions:** a new item, SB14 (L, needs SB5, phase 4), which SB7 needs; SB7 stays S. SB14's
   row and SB7's were written into ROADMAP.md on acceptance, so SB6h checks them rather than writing them.
4. **The PIN at a chroot in a locked image:** `zbm zfunc`'s `zfs_chroot` and the `zfs-chroot` applet ask for it as
   `emergency_shell` does, amending ADR 0017 point 10.
5. **New Recovery Tools entries:** none; the view keeps UI10's seven entries.
6. **`zsnapshots`:** the menu on the Snapshots tab, environments only.
7. **`logs`:** ZFSBootMenu's errors and warnings in the kernel log, and `ztrace` as its `--trace`.
8. **Completions:** written by `zbm-host` for the commands' names, listing what the legacy ones listed but kernels.

## Notes from the implementation

Implemented in 18 commits, SB6a-1 to SB6h (844c5dc0 onwards), on 11 and 12 November 2026, in the ADR's eight steps.
The VM run of SB6 is the maintainer's.

**The question the ADR left open.** Point 6 loads the filesystem's key only after a read/write yes, as `zfs-chroot`
did, while UI14's command line already marked every chroot of a locked environment with the `key` it costs. The
maintainer answered on 11 November 2026: a chroot of an environment whose root is locked loads its key first,
read-only or read/write, through the console's interaction as the menu's boot does
(`a_locked_root_is_unlocked_before_the_chroot`); the command line's cost and the man page stay, and
`CHANGES-FROM-LEGACY.md` says "A chroot loads the environment's key first". This holds for the menu's chroot, the
shell's `zfs-chroot` and a hook's `zfs_chroot` alike.

**Three changes to the accepted text:**

- **Point 6, "the menu shows the question as a dialog":** the chroot asks on the console, through
  `ConsoleInteraction` (`Subject::Chroot`), as every action that takes the console asks once the menu has yielded it;
  over SSH, on the client's terminal. The menu has no dialog and no Fluent id for it. The time-out and Enter keep the
  pool read-only, which a test pins, since `NoInteraction` would have answered Escape, read/write.
- **Points 23 and 27, the Rust `.bashrc` in SB6g:** it split from the legacy one in SB6d-2, with
  `PROMPT_COMMAND="/libexec/zbm logs --prompt"`; SB6e-3 set `alias help="zbm help"` and SB6g-3 added the completions.
  Each step that took a legacy file out of Rust images had to stop the `.bashrc` naming it, as point 27's own rule
  asks.
- **Point 15, the `Pager` context:** `keymap/pager.rs` binds `q` (Back) and `/` (Filter) alone; the arrows and page
  keys come from the shared tables, Esc and Ctrl-C leave, `h` leaves in the workspace map only (as a mover: the classic
  map's `h` means Rewind), and F1 does nothing. No screen of the model opens the pager, so its context stays out of
  `CONTEXTS` and the key map's rule-1 tests and has tests of its own, as
  [keymap.md](../menu-design/keymap.md#the-recovery-shells-pager) says.

**Other departures, in brief:**

- **Layers.** `zbm` may not use an adapter, so the applets parse, print and map exit codes, and their logic lives in
  `zbm-app` (power, the chroot, `mount_esp`, `zkexec`'s boot files, the completions) and `zbm-startup` (the natives).
  `zbm-host` cannot read `APPLETS`, so the linked names and what Tab completes after them are a table of `zbm-types`
  (`LINKED`), which a `zbm` test holds to `APPLETS` (points 3 and 23). The import stage cannot reach `zbm-startup`, so
  `zbm_pool::stage` takes the report as an argument (point 17).
- **Applets.** `--help` as the first argument prints the usage on standard output and exits 0 for every applet,
  `zfunc` and `zfsbootmenu` among them; `-h` stays only where an applet took it (`zluks`, `zbm lock`, `zbm measure`).
  The power applets share `zbm_app::power::reset` with the menu, from one `zpool list`; their messages go to standard
  error, and an argument is a usage error.
- **The chroot** needed five mounts, not four (`bind`, `proc`, `sysfs`, `devpts` and the chroot's `efivarfs`), and
  `mount_esp` a sixth, `Mount::block` (point 10); `devpts` is tested after the bind of `/dev`, as the script did; the
  root's guard unmounts recursively, as `recursive_umount` took what the user mounted inside; the filesystem mounts
  through `zbm_bootenv::mount_zfs`; the authority is proven before anything is imported or mounted. `zfs-chroot` blocks
  `SIGHUP`, `SIGINT`, `SIGQUIT` and `SIGTERM` until it has unmounted, as the script's trap undid the mounts, so Ctrl-C
  no longer ends it at the PIN or the question. After the menu's chroot the menu stays on its Recovery Tools.
- **`zkexec`** takes a boot environment and refuses a snapshot; it searches the environment's kernels first, so its
  command line is the menu's.
- **`mount_esp`** without an argument mounts the EFI system partition the boot loader names, or the only one, not
  `zbm report`'s partition (point 10); it finds a device named by a link mounted by its canonical path, says its
  refusals on standard error too, and takes two arguments or more as a usage error.
- **Firmware setup** is offered where `OsIndicationsSupported` has the bit, as point 11 says; the legacy power menu
  also wanted `OsIndications` to exist. Where it does not, choosing it failed until a follow-up of SB6 made it create
  the variable, as systemd does (`efi_set_reboot_to_firmware`), and refuse either variable when it holds no 64-bit
  value. No VM test chooses Firmware Setup, so the creation is tested on fakes only.
- **Logs.** The prompt's stamp is the kernel's sequence number of the last record shown, not its microseconds, which
  several records share; the banner writes it always, and `logs` when it shows the errors, so the first error after
  the banner is not shown twice (point 13). `--level` takes what `dmesg -l` takes and refuses the rest; `--trace`
  reads the Rust runtime's debug lines as well as the shell's; `logs` forgets only the levels it showed; `ztrace` exits
  1 where `/dev/kmsg` cannot be read.
- **Help.** The topics are plain text from the tables' reStructuredText (`zbm_config::docs::plain`), and the six
  library functions got a usage table in `zbm`; `/bin/help` is linked, but Bash's alias and builtin come first.
- **Completion** has seven kinds: the six of point 23 and the boot environments, which `zsnapshots` and
  `zfsbootmenu --snapshots` take (point 18); `zkexec`'s first word keeps every filesystem, as point 23 says. Names are
  printed shell-escaped, so one with a space stays one word, and a name with a control character is left out.
- **The image.** The context's "26 legacy entries, 20 files and 6 links" were 21 files and 5 links. `fzf` left with
  `zfsbootmenu-ui.sh`, `fzf-defaults.sh`, `zfsbootmenu-diff` and `zpowermenu`, which run it (SB6g-1); `RUST_TAKES`
  (`libexec/zfsbootmenu-input`) replaced `RUST_REPLACES` in SB6g-2, which took `efi-firmware-lib.sh` out too, its last
  users gone. The `RUST_REPLACES` of points 3, 22 and 27 is the decision's word for the steps on the way.
- **The VM checks** of "How it is checked" changed in three places. `menu-boot`'s tour has no palette to check: the
  command line (`:`) must offer no Reboot into Firmware Setup on the BIOS machine, and `menu-display` replays the
  same steps. The FAT disk of `runtime-rust-recovery` has no GPT, so no partition type to find: `mount_esp` gets the
  disk by name there, and the form without an argument is unit-tested. `firmware-boot`'s image counted down 3 seconds
  and its boot cannot stop in the menu: with the Rust runtime that image (the `boot` scenario's, not the USB
  stick's) counts down 30 seconds, and the test escapes the countdown, as `runtime` does, before it checks the menu's
  power sheet. `runtime-rust-recovery` is estimated at about 7 minutes, not 6 (`docs/luks/testing.rst`).

**Left open,** for the maintainer: a copy of the legacy `teardown.d/80-luks-close.sh` among a Rust image's hooks
replaces the built-in LUKS close but closes nothing, as the LUKS library it sources is gone (`CHANGES-FROM-LEGACY.md`
says so; a warning at build time is offered); a failed `zkexec` in `runtime-rust-recovery` fails only at the step's
1200 s time-out; no VM test chroots into an encrypted environment or hangs up an SSH session in `zfs-chroot`; and
`zbm-sys`'s coverage sits at its floor. Firmware without an `OsIndications` variable, possibly a fresh OVMF, was
offered firmware setup that then failed; a follow-up of SB6 creates the variable, as systemd does, which is still to
be checked on OVMF: on the UEFI machine, Reboot into Firmware Setup must open the firmware's setup.
