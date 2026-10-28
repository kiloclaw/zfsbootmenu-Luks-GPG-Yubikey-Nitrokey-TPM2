# Design of the Rust runtime

This document describes how the Rust runtime of ZFSBootMenu is built: the program that replaces the Bash code in
`legacy/zfsbootmenu/` inside the boot image. It was the outcome of Phase 4 of the port plan
([port-roadmap.md](../docs/dev/history/port-roadmap.md)); [ROADMAP.md](../ROADMAP.md) now plans where it goes. The
legacy runtime is no longer the reference: behaviour may change where that makes ZFSBootMenu better, and each change
is recorded in `CHANGES-FROM-LEGACY.md`, but every interface users depend on stays as it is.

**Status:** draft. A first review checked its claims against the legacy code, dracut, mkinitcpio and the test suites,
and its findings are applied; it awaits a review by a maintainer. Changes to this design are made here first, in the
same change as the code that needs them.

## Contents

- [Goals](#goals)
- [Overview](#overview)
- [Crates](#crates)
- [Data model](#data-model)
- [Process model](#process-model)
- [Hooks](#hooks)
- [Errors and logging](#errors-and-logging)
- [External programs](#external-programs)
- [Secrets](#secrets)
- [Testing](#testing)
- [The minimal runtime of Phase 5](#the-minimal-runtime-of-phase-5)
- [Freezing the legacy runtime](#freezing-the-legacy-runtime)
- [Decisions for review](#decisions-for-review)

## Goals

1. **Same behaviour.** Every screen, prompt, key binding, kernel command line option, property, hook, recovery
   command, state file and log message behaves as in the legacy runtime. Where this design departs from the legacy
   code internally, it says why, and nothing a user, a hook or a test can observe changes.
2. **Never leave the machine without a way out.** As in the legacy runtime, every failure ends in a message and a
   recovery shell, never in a hang or a kernel panic.
3. **Testable without a machine.** Every decision is a function that can be unit-tested, and every effect on the
   system goes through an interface that tests replace with a fake.
4. **Secrets stay in memory.** No passphrase, PIN or key reaches a command line, an environment variable or a file.

## Overview

The boot image holds one Rust program, `/libexec/zbm`. Like BusyBox, it acts as each of the legacy executables: the
image links each legacy path (`/bin/zfsbootmenu`, `/bin/zluks`, `/libexec/zfsbootmenu-preview` and the rest) to it,
and `zbm` runs the applet named after the path it was started as. The paths, names, arguments and subcommands of the
legacy executables stay the same, so documentation, `fzf` key bindings and user habits keep working; no applet gains a
subcommand a user could see.

A boot runs these processes:

```text
PID 1   zbm, as zfsbootmenu-init --supervise   the supervisor: never exits, reaps every child
└── zfsbootmenu-init                           start-up sequence, countdown, automatic boot
    ├── early-setup.d, setup.d, ... hooks      shell scripts, unchanged
    ├── zluks unlock                           LUKS unlocking without the menu, in its own process
    ├── zfsbootmenu                            the menu (also started over SSH or from the recovery shell)
    │   └── fzf                                draws a screen
    │       └── zfsbootmenu-preview, -help,    started by fzf for previews and key bindings
    │           zlogtail, ztrace, -diff
    └── bash -i                                the recovery shell
```

The processes share state through the files under `/zfsbootmenu`, as the legacy runtime does, and each process reads
its configuration from `/etc/zfsbootmenu.conf` and keeps its own data in memory while it runs. External programs
(`zfs`, `zpool`, `cryptsetup`, `kexec`, `gpg`, `fzf` and the rest) stay the same, and are run through one interface
that tests replace.

## Crates

The runtime is split into crates by responsibility. Library crates know nothing of the processes they run in; the
`zbm` crate turns them into applets.

| Crate | Responsibility | Legacy counterpart |
| --- | --- | --- |
| `zbm-sys` | The only FFI and the only crate with `unsafe`: signals, `kill`, `poll`, `waitpid`, credentials, wiping and locking memory, the child set-up between `fork` and `exec`, the virtual terminal's modes, the stack for crash reports. | Bash built-ins |
| `zbm-types` | Names and paths checked once, when parsed: devices, mappings, pools, datasets, snapshots, kernel versions, stages. | — |
| `zbm-trace` | Tracing: the macros code logs with, the kernel log layer with its levels and legacy formats, the JSON journal, crash report formats. | `kmsg-log-lib.sh`, `echo-log-lib.sh` |
| `zbm-platform` | The ports to the system: running programs (`Command`, `Runner`), files, clock, console, processes and environment, with their real implementations; the `System` bundle and the `Context` of services; the `Error` type, `Secret`, the kernel log writer and reader, crash reports, the command journal and file trees; shell quoting, colours, the version, and the file tests of Perl and the shell for the host tools. | `/dev/kmsg`, `tput`, `read`, `sleep`, `kill`, `cp -a`, `rm -rf`, variables |
| `zbm-display` | The display and keyboard without a terminal: DRM buffers, TrueType and PSF fonts, keyboard layouts, evdev keys, the virtual terminal's hand-over. | `setfont`, `loadkeys` |
| `zbm-state` | The state directory `/zfsbootmenu`: its layout, reading and writing each file, and its locks. | `${BASE}/*` |
| `zbm-config` | The table of options, the ZFSBootMenu command line, `zbm.*` options, kernel command lines and `%{parent}`, `/etc/zfsbootmenu.conf` and crypttab files. | `zfsbootmenu-kcl.sh`, `zfsbootmenu-parse-commandline.sh` |
| `zbm-zfs` | `zfs` and `zpool`: properties, listings, pools and datasets, mounts, importing and exporting. | the ZFS parts of `zfsbootmenu-core.sh` |
| `zbm-crypt` | `cryptsetup` with typed requests and mappings, and the key programs: `tpm2_*`, `fido2-*`, `clevis`, `ykchalresp`, `gpg`. | the program calls of `zfsbootmenu-luks*.sh` |
| `zbm-tools` | The system's other programs: `mount`, kernel modules, `udevadm`, the terminal, `kexec`, `blkid`, user scripts; block devices from `/sys`. | `zfsbootmenu-core.sh` helpers |
| `zbm-luks` | Package B: crypttab and options, finding volumes, every key method, closing volumes. | `zfsbootmenu-luks.sh`, `zfsbootmenu-luks-keys.sh` |
| `zbm-pool` | Pool import: policies, host IDs, the retry loop of the start-up stage. | `init.d/50-import-pools` |
| `zbm-boot` | Packages A, C and D: boot environments, kernels, ZFS encryption keys, hooks, `kexec`, snapshots, pool management, saved logs and reports. | `zfsbootmenu-core.sh` |
| `zbm-kcl` | The host tool that reads and writes the kernel command line of boot environments and EFI images. | `bin/zbm-kcl` |
| `zbm-app` | What front ends talk to: actions, events and the snapshot of the machine. | — |
| `zbm-startup` | The start-up program: its stages as steps, the recovery shell, the start-up hooks. | `init.d/*`, `zfsbootmenu-init` |
| `generate-zbm` | The host tool that builds images. | `bin/generate-zbm` |
| `zbm-tui` | The menu: an Elm architecture on Ratatui and nucleo-matcher. | `zfsbootmenu-ui.sh`, `fzf-defaults.sh`, `fzf` |
| `zbm-console` | The console on the display: Alacritty's terminal on a pseudo-terminal, drawn through DRM. | the kernel's console |
| `zbm-brush` | The shell of the Rust images, brush. | Bash |
| `zbm` | The multi-call program: one applet per legacy executable, the recovery commands (package F) and the supervisor. | `bin/*`, `libexec/*` |
| `zbm-testkit` | Fakes of every system interface, the legacy code runner for parity tests, seeded inputs. Tests only. | — |
| `zbm-vmtest` | The VM tests: QEMU, disks, images and the scenarios. Developers' machines only. | `tests/luks/vm` |

The host tools `generate-zbm` and `zbm-kcl` keep their own crates and share `zbm-platform`, `zbm-config`, `zbm-trace`
and `zbm-zfs` with the runtime.

Dependencies point from the program to the system and never back: each crate belongs to one layer and uses only
crates of lower layers. `docs/dev/architecture.md` lists the layers and every allowed dependency, generated from
`xtask/src/deps.rs`, and `cargo xtask check-deps` enforces them.

Third-party crates are vendored and need a reason; so far the runtime needs none.

## Data model

The data model describes what the runtime knows about the machine. ZFS and the devices are the source of truth: the
model is a snapshot, read when it is needed (as the legacy runtime reads properties each time it draws a screen), and
never written back except through `zfs`, `zpool` and `cryptsetup`.

```text
Pool 1───* Dataset ───┬── BootEnvironment 1───* Kernel
  │                   │        │ 0..1               │
  │                   │        └── EncryptionRoot   └── command line (property, %{parent}, .kcl file, override)
  │                   └── Snapshot
  └── vdevs ◄── LUKS Volume (mapping) ◄── KeySpec, options
```

- **Pool:** name, health, whether it was imported read-write, whether it is restricted (`degraded` state file:
  unhealthy, or with unsupported features), `bootfs`, `org.zfsbootmenu:readonly`, and whether it has a checkpoint.
- **Dataset name:** a validated `pool/path[@snapshot]`. Functions that take a boot environment take a dataset, not a
  string, so a snapshot cannot be passed where a filesystem is meant.
- **Boot environment:** a filesystem with `mountpoint=/`, or `mountpoint=legacy` and `org.zfsbootmenu:active=on`,
  unless `org.zfsbootmenu:active=off`; with `canmount`, its encryption root and key status, and the
  `org.zfsbootmenu:*` properties that apply to it (`kernel`, `commandline`, `rootprefix`, `keysource`, `devicetree`).
  The list is sorted by `zbm.sort_key` (descending, as `zfs list -S` sorts), with `bootfs` last, as
  `find_be_candidates` sorts it; the menu shows it reversed.
- **Kernel:** a boot environment, a kernel path and its initramfs path, relative to the boot environment's root.
  Kernels without an initramfs are left out. Kernels are kept in the order `sort -V` gives their paths (see "Version
  order" below), and the default is the last one, or the last one whose path matches the regular expression in
  `org.zfsbootmenu:kernel`. A kernel's version is taken from its file name only to find its device tree.
- **Snapshot:** a dataset name with the columns the snapshot screen shows and sorts by.
- **Encryption root:** dataset, `keyformat`, `keylocation` and `keystatus`; with the key source, if any, from which
  its key file is cached.
- **LUKS volume:** the name of its mapping, the device specification and the device it resolves to, its options and
  key specification, whether it was discovered automatically, and its state (open, skipped, failed). Which pool's
  vdevs it holds is unknown until the pool is imported.
- **Key method:** the ordered methods `luks_open_device` tries (key script, key file or GnuPG key file, tokens, reused
  passphrases, YubiKey challenge-response, passphrase prompt), as an enum, so the order is one list a test can check.
- **Boot target:** a kernel with its command line and device tree, from which the `kexec` arguments and the hook
  environment are built.

Each type comes with the parser that builds it from command output and the functions that make decisions about it.
These are pure functions: they take text and values and return values, so unit tests cover them with recorded output.

### Version order

The legacy runtime orders kernels with the `sort -V` of the image, which is not the same program in every image:
dracut images include the host's `sort`, which is GNU coreutils on most distributions but uutils (0.12.0) in the
build container, whose Ubuntu 26.10 ships the Rust coreutils, and mkinitcpio images use BusyBox's where BusyBox
provides one. The Rust runtime uses the order of uutils, the coreutils of its images (ROADMAP.md,
decision 4): `zbm_types::version::version_cmp` is `uucore`'s `version_cmp` with bytes breaking ties, as
`sort -V` does, and a test compares it with the build container's `sort -V` on generated names. On 2,745
kernel-like names, uutils differed from GNU only where a version part is `0` and another name has a dot followed by
a letter at that place (`kernel-0` against `kernel-.X`, which GNU puts first and uutils last); no distribution names
kernels that way. `legacy/RUNTIME-CHANGES.md` lists this as a known inconsistency of the legacy runtime, and package C
compares BusyBox on recorded kernel names.

## Process model

### Processes

The runtime keeps the legacy process structure, for three reasons:

- **The menu can be started more than once.** A user connected over SSH, or in the recovery shell, runs
  `zfsbootmenu`, which takes over from the instance on the console. That needs the menu to be a separate process.
- **`fzf` starts other programs.** Previews and key bindings run commands, which are new processes and cannot share
  memory with the menu.
- **A crash must not take the machine down.** In the legacy runtime, `zfsbootmenu-init` is process 1: dracut's
  `/init` sources its `pre-mount` hooks (`source_hook pre-mount`), and mkinitcpio's `/init` runs `run_hook` in its own
  shell, so the `exec` in the start-up scripts replaces process 1. Bash survives failing commands, but a Rust panic or
  abort in process 1 would make the kernel panic. So process 1 is a small supervisor that runs the start-up program as
  its child and never exits.

| Process | Applet | Does |
| --- | --- | --- |
| Supervisor | `zfsbootmenu-init --supervise` | Process 1. Starts `zfsbootmenu-init` on the console and reaps every child, including orphans. If `zfsbootmenu-init` ends for any reason, it logs that, opens a recovery shell and then starts it again. Contains no ZFS logic. |
| Start-up | `zfsbootmenu-init` | The start-up sequence, the countdown and the automatic boot, then the loop of menu and recovery shell, as in `libexec/zfsbootmenu-init`. Its modes are described below. |
| Menu | `zfsbootmenu` | The menu loop of `bin/zfsbootmenu`. Holds `/zfsbootmenu/active`. |
| `fzf` callbacks | `zfsbootmenu-preview`, `zfsbootmenu-help`, `zfsbootmenu-diff`, `zlogtail`, `ztrace`, `zfunc` | Short-lived; read the configuration, the state directory and ZFS, draw or act, and exit. |
| LUKS unlocking | `zluks unlock` | The existing `zluks` subcommand. On the plain console, the start-up program runs it as its own process, so that passphrases leave memory when it ends, as the legacy subshell in `init.d/45-luks-unlock` ensures. On the menu's start-up view, the start-up program unlocks itself, asking in the view's dialogs (below, and [Secrets](#secrets)). |
| Recovery shell | `bash -i` | Unchanged. The recovery commands it offers are applets. |

#### Modes of the start-up program

`zfsbootmenu-init` keeps the three modes of the legacy script:

- **On the console** (`--console`, which the supervisor passes, or `ZFSBOOTMENU_CONSOLE` set, as the legacy start-up
  hook sets it): run the stages that have not run, then the countdown, the automatic boot, and the loop of menu and
  recovery shell. The legacy program unsets the variable after the stages; an argument leaves nothing for the programs
  it starts to inherit.
- **Stages only** (`ZFSBOOTMENU_CONSOLE` not set): run the stages that have not run and exit. `zfsbootmenu` starts it
  this way when it finds no `initialized` marker.
- **Forced** (`ZFSBOOTMENU_FORCE_INIT` set to yes, y, true, t or 1, in any case): run every stage again.

On the console, the stages run on the menu's start-up view when the image has the menu (`/bin/zfsbootmenu`) and the
terminal can run it (`TERM` set, and not `dumb`), except after `--resume` (ROADMAP.md, R5 step 4). The view
shows the stages as they run, and its dialogs answer what they ask: the secrets and devices of LUKS volumes (the
`Interaction` of `zbm-platform`), the retries of the import (the `Operator` of `zbm-pool`) and the devices `zbm.wait_for`
names, with the timeouts of the console prompts. LUKS volumes are unlocked in the start-up program, not by
`zluks unlock`. The stages' programs run off the console, what they print going to the log
(`zbm_platform::QuietRunner`); `early-setup.d` hooks and the recovery shell get the console, which the view hands
over and takes back. The view closes before the countdown, which the menu shows. Otherwise, the stages run on the
plain console, as in the legacy program (`zbm_startup::front::ConsoleFront`).

The legacy script runs each executable file in `/libexec/init.d` and then removes its permissions, so a stage runs
once unless forced. The Rust image has no such files; the stages that have finished are listed in
`/zfsbootmenu/.stages`, a new internal file with one stage name per line, which plays the part of the permission bits.

When the supervisor starts `zfsbootmenu-init` again after it ended unexpectedly, it passes `--resume`: the stages that
finished are skipped, and the countdown and the automatic boot are skipped too, because a machine that just recovered
from a crash should not boot without the user. The menu loop starts at once.

#### Start-up hooks

The dracut and mkinitcpio hooks stay shell scripts, as the roadmap requires, because they run inside the shell of
`/init`:

- **dracut `cmdline` hook:** runs `/libexec/zbm zfsbootmenu-parse-commandline`, which parses the command line and
  writes the parsed options to `/etc/zfsbootmenu.conf`, and then sets `rootok=1` and `root=zfsbootmenu` and links
  `/dev/root` to `/dev/null` itself, as the legacy script does, because dracut reads those variables from its own
  shell and a child process cannot set them.
- **dracut `pre-mount` hook:** `exec /libexec/zbm zfsbootmenu-preinit`. The applet sets the console size, turns kernel
  messages off, sets the host name and execs the runtime's shell (`bash -c '...'`) in a session of its own
  (`zbm_sys::prepare_child`, a `setsid` before the `exec`, as the `setsid` program does: in a child, and this process
  ends, when it leads its process group), whose script sources `/etc/zfsbootmenu.conf` and `/etc/profile` and execs
  `/libexec/zbm zfsbootmenu-init --supervise` with the control terminal as its standard streams, so process 1 becomes
  the supervisor. The shell opens the terminal in the new session, so it becomes the controlling terminal, as in the
  legacy hook; and the variables the legacy start-up program exports by sourcing the two files reach every program the
  runtime starts, hooks included. The supervisor starts the programs on the console's terminal in sessions of their
  own with it as their controlling terminal (`setsid -c`) the same way, without the `setsid` program.
- **mkinitcpio:** `/libexec/zfsbootmenu-initcpio` runs the two applets the same way.
- The dracut `initqueue` checks (`zfsbootmenu-ready-set.sh`, `zfsbootmenu-ready-chk.sh`) stay as they are.
- `zfsbootmenu_runtime=rust` selects these hooks (`packaging/dracut/zfsbootmenu-rust-*.sh` and the mkinitcpio entry
  point) and leaves the legacy start-up program and `init.d` out of the image.

The start-up stages run in this order, as the steps of `zbm-startup` (one `Step` per stage, run by `run_steps`):

| Stage | Legacy |
| --- | --- |
| Mount `efivarfs` read-only | `zfsbootmenu-init` |
| Kernel modules, `udevadm settle` | `init.d/10-kmods` |
| Host ID | `init.d/20-hostid` |
| Wait for `zbm.wait_for` devices | `init.d/30-device-wait` |
| External hook root, `zbm.skip_hooks`, `early-setup.d` hooks | `init.d/40-early-hooks` |
| LUKS unlocking | `init.d/45-luks-unlock` |
| Pool import, retries, host ID matching, restricted pools, unsupported features, `bootfs` | `init.d/50-import-pools` |
| Mark the start-up done (`initialized`) | `zfsbootmenu-init` |

#### The supervisor

The supervisor is the only code in process 1, so it is kept small and cannot panic: its module denies `unwrap`,
`expect`, `panic!` and indexing, and allocates nothing after start-up beyond what starting a child needs. It ignores
`SIGINT`, and it starts its children itself and waits for them with `waitpid(-1)` (through `zbm-sys`), which also
reaps orphans. It never uses `Runner` or the standard library's `Child::wait`, because its `waitpid(-1)` would collect
their exit statuses first. If the recovery shell itself cannot start, it waits ten seconds and tries again, so an
error never turns into a busy loop.

### Shared state: the state directory

The processes share state only through files under `/zfsbootmenu` (`BASE`), with the legacy layout and formats.
`zbm-state` owns the layout: no other code builds these paths. Keeping the layout has a cost, since files are slower
than memory, but it is small next to the `zfs` commands the runtime runs, and it buys three things: users and
documentation rely on some of these paths (remote access ends the menu named in `active`, the GRUB migration guide works in
`environments/<dataset>/mnt`, `zluks(8)` documents the LUKS files); a hook or a user in the recovery shell sees what
they see today; and short-lived callback processes need no server to ask.

| File | Written by | Read by | Format |
| --- | --- | --- | --- |
| `ready` | dracut's `initqueue/settled` hook | dracut's `initqueue/finished` hook | empty |
| `initialized` | start-up, when done | menu | empty |
| `.stages` | start-up, after each stage (new, replaces the permission bits of `/libexec/init.d`) | start-up | one stage name per line |
| `active`, `active.lock` | menu, at start; `active` removed at exit | menu (takeover), start-up (before the automatic boot), users | process ID and newline; a file locked with `flock` while a menu runs |
| `bootfs` | pool import; menu (`CTRL+D`) | start-up, menu | dataset name and newline |
| `degraded` | pool import, re-import after the recovery shell | read-write imports | one pool per line |
| `spl_hostid` | pool import, when a host ID matched | host ID decisions | host ID, no newline |
| `noresume` | resume prompt | command line assembly, command line cache | empty; its modification time matters |
| `zbm.cmdline`, `cmdline.d/<key>` | command line parsing | option lookups | tokenized command line; one value per key |
| `cmdline` | command line editor, `zbm.kcl_override` | command line assembly, main screen header | tokenized command line |
| `bootenvs` | menu | `fzf` | one boot environment per line |
| `environments/<dataset>/kernels` | kernel discovery | kernel selection, kernel screen, previews | `dataset<TAB>kernel<TAB>initramfs` lines |
| `environments/<dataset>/kcl/<kernel>.kcl` | kernel discovery, command line cache | command line assembly | command line |
| `environments/<dataset>/mnt` | mounting a boot environment | users | mount point |
| `.keys/<key source>/<path>` | key caching | key loading | copies of ZFS key files, mode 0600; removed before `kexec` |
| `.keys/<key source>/.lock` | key caching, while it runs | key caching | empty; locked with `flock` |
| `.external_hooks` | external hook root | hook import | mount point |
| `have_errors`, `have_warnings`, `errors_since` | logging, error printer | previews, recovery shell, `zlogtail` | empty; time stamp |
| `luks-mappings`, `luks-mappings.lock`, `luks-skipped` | LUKS unlocking | teardown hook, `zluks`, retries | `name<TAB>device<TAB>auto` lines; a file locked with `flock`; one device per line |

Files that other processes may read while they are rewritten (`bootfs`, `bootenvs`, `kernels`, `.kcl` files) are
written to a temporary name and renamed, so a reader sees the old or the new contents, never a partial file. The
legacy code writes them in place; nothing can observe the difference except a reader that would have seen a partial
file. The locks of the state directory are `flock` locks on files that stay (`luks-mappings.lock`, `active.lock`, a key
cache's `.lock`), taken through `Files::lock`: the kernel lets go of a lock when its process ends, killed or not, so no
lock is left behind and none is taken anyway after a wait, as the legacy code's lock directory was after five seconds;
`active` holds the menu's PID only while `active.lock` is held. `degraded` and `luks-skipped` are appended to without a
lock, as in the legacy code.

Within a process, data read from ZFS lives in the data model and is read again at the points where the legacy code
reads it again (each time a screen is drawn), so the Rust runtime sees changes made from a hook or the recovery shell
at the same moments.

### Configuration and environment

`/etc/zfsbootmenu.conf` stays the configuration of the image, in the same shell format, because shell code reads it
directly: hooks source it (the bundled console hooks do), `/root/.bashrc` sources it for the recovery shell and for
SSH sessions, and the legacy scripts source it at their start. The image build writes its first part (the `HAS_*`
feature flags, `ZBM_BUILDSTYLE`, `ZBM_RELEASE_BUILD`, `ZBM_RUNTIME`, behind an include guard); the command line
parsing appends the parsed options (`BASE`, `loglevel`, `import_policy`, `menu_timeout`, `zbm_*`, `spl_hostid`,
`control_term` and the rest) as `export name='value'` lines, as the legacy start-up script does.

Every Rust applet reads the file itself at start, with a parser for the lines the image build and the command line
parsing write, as each legacy script sources it, instead of trusting an environment inherited from the supervisor: a
`zfsbootmenu` started over SSH, or with `ssh host zfsbootmenu`, does not descend from the supervisor. Values that
change while the machine runs come from the state directory, as in the legacy code: `bootfs`, and `spl_hostid` once a
pool matched.

The environment remains the way to pass what the legacy code passes in variables: `SSH_TTY` (which replaces
`control_term` over SSH), the stage variables of hooks, and the variables the legacy code sets for one call
(`read_write=yes import_pool`, `CLEAR_SCREEN`, `NO_CACHE`, `allow_rw`, `rewind_to_checkpoint`), which become function
parameters inside Rust and which the applets that hooks call accept from the environment. Code reads variables through
the `Environment` interface. No process changes its own environment (`std::env::set_var` is `unsafe` since the 2024
edition): variables for a child are set on its `Command`.

### `fzf` callbacks

The menu writes the screen's rows to a file or a pipe and runs `fzf` with the legacy options and key bindings. The
bindings and the preview command run applets by their legacy paths with the legacy arguments (`fzf` substitutes the
selected row for `{}`), for example `/libexec/zfsbootmenu-preview -b <bootfs> <dataset>`. The applet reads what it
needs from the configuration, the state directory and ZFS, as the legacy scripts do, and exits. `fzf` reports the key
that ended the selection (`--expect`), and the menu loop maps it to the next screen as `bin/zfsbootmenu` does; that
mapping is a pure function, which unit tests check for every key binding.

### Concurrent instances and takeover

The protocol stays as it is, because users rely on it (`remote-access.rst`): the menu writes its process ID to
`active`; a second `zfsbootmenu` sends `SIGUSR1` to that process, finds its deepest child through
`/proc/<pid>/task/<pid>/children` and ends it, so the blocked `fzf` returns; the first menu exits on `SIGUSR1`, which
removes `active`. The start-up sequence does not boot automatically while `active` exists, and opens the recovery
shell instead. The menu blocks `SIGUSR1`, `SIGTERM` and `SIGHUP` first thing, so they wait in a `signalfd`
(`zbm_sys::Signals`, ROADMAP.md PM3), and reads them after each child process returns, which is when the legacy trap
runs too.

### Signals

The legacy code ignores `SIGINT` in `zfsbootmenu-init` and in `bin/zfsbootmenu` (`trap '' SIGINT`). An ignored signal
stays ignored in every program those processes start, so hooks and `fzf` run with `SIGINT` ignored too, and a menu
started by `zfsbootmenu-init` already has it ignored on entry, where Bash refuses to trap it: the `trap 'exit 0'
SIGINT` in `duplicate_snapshot` takes effect only in a menu started over SSH or from the recovery shell. The recovery
shell (`trap - SIGINT; exec /bin/bash -i`) runs interactively and handles `SIGINT` itself.

The Rust runtime reproduces the dispositions each program sees, rather than describing them from the code: package A
adds a VM test that records the ignored and caught signals (`SigIgn` and `SigCgt` in `/proc/<pid>/status`) of a hook,
`fzf`, the chroot shell and the recovery shell, in a menu on the console and one over SSH, first against the legacy
image. The supervisor, the start-up program and the menu ignore `SIGINT` through `zbm-sys`, so their children inherit
the same disposition.

A program that waits for signals blocks them and reads them from a `signalfd` (`zbm_sys::Signals`): the menu its
takeover's, the console the requests of `zbm console release` and `layout`, `generate-zbm` `SIGINT` and `SIGTERM`; no
handler runs in the middle of the program, and no global slot holds the last signal. A signal sent to the process
reaches a thread that does not block it, so each blocks them before it starts a thread. Programs inherit the mask, and
the standard library's `Command` keeps it: `zbm_sys::prepare_child` empties it in every child that `zbm-platform`
starts, so hooks, `fzf` and the shells see the dispositions above and no blocked signal. A process that is signalled
and waited for is held by a pidfd (`zbm_sys::Process`): `zbm console release` sees the console end.

`SIGPIPE` stays ignored in every runtime process, unlike in the host tools, which restore its default action. In the
legacy code, a value piped into a program (`printf '%s' "${pass}" | cryptsetup ...`) is written by a subshell, which
dies alone if the program exits without reading it. In Rust the runtime writes the value itself, so a write to a
closed pipe must fail with an error instead of ending the process. Children are not affected: the standard library
restores the default `SIGPIPE` action in every program it starts, which matches the children of Bash.

## Hooks

Hooks stay shell scripts in the same directories (`/libexec/hooks/{early-setup,setup,load-key,boot-sel,teardown}.d`),
with the same rules as `zfsbootmenu-run-hooks`:

- Every executable file in the directory runs, in the order of its name in the C locale (the image sets no locale),
  with the standard streams of the process that runs the stage. `processing <hook>` is logged at the information level
  before each one; its exit status is ignored and not logged.
- A stage reports whether any hook ran: `load-key.d` checks the encryption root again only then.
- `early-setup.d` hooks run once: afterwards they lose their permissions and are renamed to `*.completed`, and
  `Disabling hook after execution: <hook>` is logged.
- `zbm.hookroot` imports hooks from another file system first, and `zbm.skip_hooks` then removes the permissions of the
  named hooks, before the `early-setup.d` stage.
- The environment is that of the process that runs the stage (the variables of `/etc/zfsbootmenu.conf`, and `BOOTFS`
  once it is known), plus the variables of the stage. The start-up program has the configuration's variables because
  the supervisor starts with them exported (see "Start-up hooks"); an applet that does not descend from it, such as a
  menu started over SSH, adds them to the hooks it runs, as `zfsbootmenu-run-hooks` sources the file. The stage
  variables are `ZBM_LOCKED_FS` and `ZBM_ENCRYPTION_ROOT` for `load-key.d`; `ZBM_SELECTED_BE`,
  `ZBM_SELECTED_KERNEL`, `ZBM_SELECTED_INITRAMFS` for `boot-sel.d` and `teardown.d`, and `ZBM_SELECTED_MOUNTPOINT`
  for `boot-sel.d`.

Running a hook is a `Command` with inherited streams; the hook environment is built by a pure function, so unit tests
check it for every stage.

### Compatibility library

Hooks may source `/lib/zfsbootmenu-core.sh` and `/lib/kmsg-log-lib.sh` (the hooks in `contrib/` do), and
`zfsbootmenu-core.sh` sources `/lib/zfsbootmenu-kcl.sh` and stops if it is missing. The contract covers every function
these three files define, 69 today, not a selection: the Rust image installs the three files as a shell library in
which each function calls `zbm`:

```sh
zinfo() { /libexec/zbm zfunc zinfo "$@"; }
load_key() { /libexec/zbm zfunc load_key "$@"; }
```

`zfunc` is the applet that `fzf` already uses to call a library function by name (`libexec/zfunc`), so the library
needs no new applet. The rules:

- **The list comes from the legacy files.** A test compares the functions the library defines with the functions the
  three legacy files define, so none is left out when the legacy code changes.
- **Functions that only transform text** (`colorize`, `decolorize`, `center_string`, the `kcl_*` functions) may stay
  shell code, copied from the legacy files, which avoids a process per call.
- **Implicit inputs** (`read_write=yes import_pool tank`) reach `zbm` because the shell exports prefix assignments to
  the command it runs.
- **Implicit outputs** need shell code around the call: `set_default_env` sets `BOOTFS` in the caller to its argument
  when it succeeds (the wrapper does the same), `import_pool` sets `spl_hostid` after a failed import (the wrapper asks
  `zfunc get_spl_hostid`), and `timed_prompt` leaves `HEIGHT` and `WIDTH` set (from `tput`). Package F checks every
  function for such effects and adds a test for each one it finds.
- **Calling conventions:** each wrapper passes the caller's standard streams, prints what the legacy function prints
  and returns its status.

## Errors and logging

### One error type

Library code returns `zbm_platform::error::Result<T>`, whose error is one enum:

| Variant | Meaning | Example |
| --- | --- | --- |
| `Io` | A file or device operation failed; carries what was attempted and the `io::Error`. | writing `/zfsbootmenu/bootfs` |
| `Failed` | A program ended unsuccessfully; carries the command (without its input), its status and its output. | `zpool import -N -a` |
| `Invalid` | Input that cannot be used, with the message the legacy code logs. | an unknown crypttab option |
| `Cancelled` | The user cancelled: Escape at a prompt, an empty passphrase where that means "skip". | |

An error's text is the message the legacy code logs with `zerror` at that point; a `Failed` error also carries the
command's output, which is logged with `zdebug`, as the legacy code logs it. Functions that the legacy code lets fail
silently return `Option`, not an error, so the Rust code does not report more than the legacy code does.

The domain crates refactored in R1 have an error enum each, with the reasons their callers decide on:
`zbm_luks::LuksError` (with `TokenFailure` for the key methods) and `zbm_boot::BootError`. Their operations return
`Result` instead of a success flag, and those that end in one of several good ways return an outcome:
`unlock::Opened` (already open, opened elsewhere, or unlocked with a `Method`), `UnlockSummary`, `session::Wait`,
`resume::ResumeChoice`, and `zbm_types::Answer` for timed prompts. The step still logs what the legacy code logs; the
error is for the code that decides what happens next, and `bool` is left for questions (`is_open`).

### Where errors go

The caller decides, at the same points as the legacy code:

1. **Log and continue:** most failures inside a stage (a kernel without an initramfs, a key that failed to load).
2. **Tell the user:** a centred message with a countdown (`timed_prompt`), as the legacy code shows after a failed
   `kexec` or a prohibited read-write import.
3. **Open the recovery shell:** `emergency_shell` with a message, after which the menu starts again, as in the legacy
   loop in `zfsbootmenu-init`. The shell prints the errors logged since the last time
   (`recovery-error-printer`, `have_errors`).

Panics are bugs. In every runtime program a panic hook (`zbm_platform::CrashReporter`) writes a crash report to
`/zfsbootmenu/crash-<time>-<pid>.txt` (the message and place, the build ID, the open `tracing` spans and the stack as
offsets from the binary's load address), logs an error naming it, and writes it to the journal, before the process
ends. When the start-up program ends, the supervisor names its report, or writes one when a signal killed it, and
opens the recovery shell, which says where the report is; `zbm debug crash` prints it. The supervisor cannot panic
(see "The supervisor"). `cargo xtask symbolize` resolves a report with the debug file of its build ID
(`docs/dev/tooling.md`, "Crash reports").

### Logging

Messages go to `/dev/kmsg` in the legacy formats (`<N>ZFSBootMenu: message`, and `<7>ZBM:[pid]|trace|message` for
debugging output, with the Rust caller as the trace), filtered by `loglevel`, and warnings and errors create
`have_warnings` and `have_errors`. Code logs with the macros of `zbm-trace` (`zbm_trace::error!` and the others),
which emit `tracing` events for the kernel log; no function takes a log as a parameter. `zbm-trace`'s `KmsgLayer`
formats and filters them, and `zbm-platform`'s `KernelLog` installs that layer for a process with a sink that writes
through the files interface, so unit tests that install it on the fake files see every message. Other `tracing`
events, such as the journal's, never reach the kernel log. The recovery commands read the log back with `dmesg`, as
now.

The same `KernelLog` appends every `tracing` event, whatever its level, to `/zfsbootmenu/journal.jsonl`: one line of
JSON per event with the spans it happened in (stage, pool, volume) and the programs run through
`zbm_platform::TracedRunner`, so a failed boot can be read back step by step. Each process writes at most 4 MiB.
Secrets stay out by type (`Secret` prints as `Secret(..)`) and by name: only the fields of `zbm_trace::KEPT_FIELDS`
are written, and any other is written as `"[redacted]"`, as is one named `passphrase`, `password`, `pin`, `secret`,
`challenge` or `response` (or ending in `_` and one of them). A new field joins the list in the change that adds it.

## External programs

### Running programs

Every program is run through the `Runner` trait of `zbm-platform` with a `Command`: program, arguments, standard input
(inherited, `/dev/null`, bytes or a secret), standard output and standard error (inherited, captured, discarded, or
captured as a secret), and environment changes. `SystemRunner` runs it; a program that does not exist returns status
127, as a shell reports it. Interactive programs (`fzf`, `zfs load-key -L prompt`, `bash -i`, hooks) inherit the
terminal.

- **Building commands** is a pure function for each operation (`zpool import` with its options, `kexec -l` with its
  command line, `cryptsetup open` with its options), so unit tests compare the exact arguments with the legacy ones.
- **Parsing output** is a pure function from text to the data model, tested with output recorded from the real
  programs (`crates/*/tests/fixtures/`, recorded in the VM tests: `zbm-vmtest run runtime-record-fixtures` and `cargo xtask record-zfs`).
- **Exit statuses** keep their meaning: commands killed by a signal report 128 plus the signal, as a shell does.
- **Locale and environment:** programs inherit the environment of the process, as now; only the variables the legacy
  code sets are added (`TPM2TOOLS_TCTI`, `GNUPGHOME` and the hook variables).

## Secrets

Passphrases, PINs, YubiKey responses, FIDO2 secrets, unsealed TPM2 secrets and decrypted key files are secrets.

- **In memory,** a secret is a `zbm_platform::Secret`: its bytes live in pages of their own
  (`zbm_sys::memory::Pages`), from `memfd_secret` where the kernel allows it, which takes them out of the kernel's
  map of memory too, else anonymous memory locked with `mlock`; either way left out of core dumps
  (`MADV_DONTDUMP`) and of child processes (`MADV_WIPEONFORK`, `MADV_DONTFORK`). They are wiped (with
  `explicit_bzero`) when the secret is dropped, it grows without leaving copies behind, and it prints as
  `Secret(..)`, so it cannot end up in a log message or an error by accident.
- **Processes** of the runtime are undumpable (`PR_SET_DUMPABLE`): no core dump, and only root may read their
  memory. Buffers that held what a secret is made of, such as the key codes the console on the display reads, are
  wiped too (`zbm_sys::wipe_values`).
- **Into a program,** a secret goes only on standard input (`Input::Secret`), for example
  `cryptsetup open --key-file=- ...`, `gpg --passphrase-fd 0`, or `tpm2_unseal -p session:<context>+file:-`. Some
  programs read their input as text: `tpm2_unseal` stops at a zero byte and drops a final newline, so the
  authorization value goes to it as `hex:<value>`.
- **Out of a program,** a secret is captured from standard output into a `Secret` (`Stream::CaptureSecret`), for
  example the output of `gpg --decrypt`, `tpm2_unseal`, `fido2-assert`, `ykchalresp` and `clevis decrypt`.
- **From the user,** a secret is read from the console with echo off, directly into a `Secret`. In the start-up
  view, the unlock dialog types it into a `Secret`. The view reads the terminal itself (`zbm_tui::menu::term_keys`)
  rather than through the terminal library's event reader, which keeps the last bytes it read in a buffer of its
  own, and wipes the bytes and the keys it decoded. The menu still reads keys through the library, as it asks for no
  secret yet; it moves to the same reader before it does (R5 step 6, LUKS volumes from the menu).
- **Derived values** (the SHA-256 authorization value of a TPM2 PIN, systemd's PBKDF2-salted PIN) are computed in the
  process instead of by `sha256sum` and `openssl`.
- **Lifetime:** secrets that unlock several volumes are kept for one unlocking run and dropped at its end. On the plain
  console the run is a process of its own (`zluks unlock`); on the start-up view it runs in the start-up program,
  where the dropped secrets are wiped (decision 11).
- **Before `kexec`,** which leaves memory as it is, the start-up program drops the clean page cache (key files read
  from key devices) and warns if it still holds a secret (`zbm_boot::boot::wipe_before_kexec`). With `init_on_free=1`
  on its command line, the kernel zeroes every page it frees, so what ended processes and closed pipes left behind is
  gone too; `zbm report` says whether it does. The VM tests `secrets-passphrase` and `secrets-keyfile` search every
  process's memory, command line and environment, and every file in memory, just before `kexec`.

The legacy code is close to this but not the same, and the Rust runtime is stricter where nothing observable changes:

- It decrypts GnuPG key files, and stores unsealed TPM2 and FIDO2 secrets, in temporary files below `/run` (RAM) and
  passes those files to `cryptsetup`; `zfsbootmenu-luks(7)` documents this. The Rust runtime passes them on standard
  input instead, and creates no such files.
- It passes a salted TPM2 PIN to `openssl kdf` as an argument, where any process could read it in `/proc` while the
  program runs. `openssl kdf` takes the password no other way, so the legacy code keeps this, as
  `legacy/RUNTIME-CHANGES.md` records; the authorization value it used to pass to `tpm2_unseal` as an argument now goes
  on standard input there too.
- Key files the user provides (in a crypttab, on a key device, or cached from an `org.zfsbootmenu:keysource` in
  `/zfsbootmenu/.keys`) remain files, passed by path, because the features are defined that way and `cryptsetup` and
  `zfs load-key` read them by path. They are the user's files, not secrets the runtime created.

## Testing

### System interfaces

All code that reaches the system does so through these interfaces, which `zbm-platform` defines and which a `System`
value bundles, so a function needs only one parameter for all of them.

| Interface | Covers | Real implementation | Fake (`zbm-testkit`) records |
| --- | --- | --- | --- |
| `Runner` | every external program | `SystemRunner` | each command with its input; answers from responders |
| `Files` | the state directory, `/proc`, `/sys`, `/dev/kmsg`, `/etc/zfsbootmenu.conf`, mount points, hook directories, waiting for device nodes | `RealFiles` (the standard library) | an in-memory tree with modes, times and symbolic links; every write |
| `Clock` | countdowns, retry delays, waiting, time stamps | `SystemClock` | the time, which only advances when the code sleeps; every sleep |
| `Console` | prompts, key presses, passphrases, the screen size, output | `Terminal` (standard input and output, with `stty` for the terminal's modes, whose failures are ignored as in the legacy code) | scripted key presses, lines and secrets; everything written |
| `Processes` | its own process ID, signals to other processes | `SystemProcesses` (`zbm-sys`) | every signal sent |
| `Environment` | environment variables | `ProcessEnvironment` | the variables the test sets |

Devices are files (`/dev/disk/by-*`), so waiting for them is `Files` and `Clock`; mounting, `kexec` and module loading
are programs. The supervisor's starting of and waiting for children is the one part outside these interfaces (see
"The supervisor"); its decisions (what to start next, after which status) are a pure function with unit tests, and the
VM tests cover the rest. The fakes check what must not happen as well: `FakeSystem::assert_no_leak` fails if a
secret's bytes appear in any argument, environment variable, written file or console output.

### How each package is tested

| Package | Unit tests | Suites that must pass against the Rust image |
| --- | --- | --- |
| Phase 5 (minimal boot) | import arguments, `bootfs`, kernel selection, `kexec` arguments, the configuration file, the checks for unsupported set-ups, with fakes | a new VM test that boots an unencrypted pool |
| A: start-up and import | every import policy, retry and host ID case; the stage order and modes; the import loop's order with late LUKS volumes; hook environments | upstream test VMs; LUKS VM tests for import policies, host IDs and late devices; the signal disposition test |
| B: LUKS | crypttab and options (parity with the Bash parser), key method order, token metadata from recorded `luksDump` output, no leaks | LUKS unit suites; every LUKS VM test |
| C: boot environments | sorting, kernel selection and version order, command line cache, key sources, hook environment | every VM test that boots |
| D: snapshots and pools | the commands each action runs | a new suite test per action, written against the legacy image first |
| E: menu | the screen that follows each key; snapshot tests of each screen's text | the VM tests that drive the menu |
| F: recovery tools | the programs `zbm` needs, the recovery commands' logic, the compatibility library's function list and implicit outputs | the `contrib/` hooks unchanged; image sizes compared |

Pure functions with a Bash counterpart get parity tests that run both on the same inputs (`zbm-testkit::legacy`), as
in phases 2 and 3.

### Driving the existing suites

- **LUKS unit suites** (removed in October 2026, with the test build below; the device suites of
  `crates/zbm-luks/tests/devices` replace them). They loaded the code under test through `tests/luks/unit/impl.sh`, which also provides every
  seam through which they observe and control it, and documents them: messages as `LEVEL: text` lines in a log file,
  timed prompts recorded in a file and answered from another, a directory of stand-ins for open volumes, which the
  stub `cryptsetup` keeps, and the six operations the suites call (`luks_unlock_all`, `luks_close_all`,
  `luks_resolve_device`, `luks_list_tokens`, `luks_record_mapping`, `luks_token_wait`). With `ZBMTEST_IMPL=rust`,
  the adapter provides the same seams with a test build of `zbm` (the Cargo feature `test-seams`): the environment
  variables `ZBMTEST_LOG`, `ZBMTEST_PROMPTS`, `ZBMTEST_ANSWERS` and `ZBMTEST_HOLDERS` send its messages, prompts
  and the check for open volumes to those files, and the hidden command `zbm-luks-test` runs the six operations.
  Images never contain that build; CI checks that the release `zbm` has no such command, and runs the suites
  against both implementations. Two cases in `edge.sh` reach inside the legacy code (one replaces
  `luks_available`, one runs the import stage with its functions replaced); they run only against the legacy code,
  and unit tests of `zbm-luks` and `zbm-boot` cover them for Rust. One check accepts either way of passing an
  unsealed TPM2 key to `cryptsetup` (a file in `/run` for the legacy code, standard input for Rust).
- **VM tests and upstream test VMs.** They boot an image. CI builds a legacy and a Rust image
  (`zfsbootmenu_runtime=rust`) and runs every suite against both; from Phase 5 on, the results against the Rust image
  are the measure of progress.

## The minimal runtime of Phase 5

The Phase 5 runtime boots only the default boot environment of an unencrypted pool, and must say so instead of
misbehaving when a machine needs more. Before anything that the legacy runtime would do differently, it checks the
set-up with a pure function, which lists what it cannot handle yet:

- a crypttab or `zbm.luks` and `zbm.luks_key` options, or LUKS volumes the legacy runtime would unlock;
- an encryption root that is locked when its boot environment is chosen;
- an executable hook in `load-key.d`, which only runs for locked encryption roots;
- the menu: `zbm.show`, no default boot environment, or any key press during the countdown.

Package A of Phase 6 removed the import options (`zbm.import_policy`, `zbm.prefer`, `zbm.force_import`,
`zbm.wait_for`) and `zbm.hookroot` from the list, once their VM tests passed against the Rust image, and package B
the LUKS volumes. Step 1 of R5 (`ROADMAP.md`) removed locked encryption roots and `load-key.d` hooks: the
runtime loads keys in the legacy order, with the key-source cache for boots chosen in the menu. Step 3 added the menu
(`/bin/zfsbootmenu`, the `zfsbootmenu` applet of `zbm`), so the list is empty in every Rust image; an image without
the menu program would still list the menu.

Hooks themselves are not on the list: every image installs the bundled hooks (console set-up in `early-setup.d`, LUKS
closing and xHCI unbinding in `teardown.d`), so the Phase 5 runtime already runs the `early-setup.d`, `boot-sel.d` and
`teardown.d` stages, with `zbm.skip_hooks`, through the hook runner of package A. Running executable files in order
with an environment is small, and leaving it out would leave the console unset and some USB controllers unprepared for
`kexec`.

If the list is not empty, the runtime logs each item as an error and opens the recovery shell with the message
`not supported by this build of ZFSBootMenu yet`, from which the user can boot by hand or switch to the legacy image.
The LUKS configuration is checked before the stages run, LUKS volumes when the LUKS stage or a retry of the import
would unlock them, and the rest before the boot. Where the legacy program starts the menu, the Phase 5 runtime opened
the recovery shell with the same message, and kept doing so, importing pools again between shells as the legacy loop
does. Since R5 step 3 that loop runs the menu program and then the recovery shell, as the legacy loop does, and the
countdown runs in the menu (`zfsbootmenu --countdown`).

Until package F replaces them with the compatibility library, the Rust image keeps the legacy shell libraries, the
recovery shell's tools and the bundled hooks, which source those libraries; only the legacy start-up program and
`init.d` are left out.

## Freezing the legacy runtime

From Phase 4 on, the legacy runtime (`legacy/zfsbootmenu/`, and the dracut and mkinitcpio files in `packaging/` that
install it) gets bug fixes only. Each change is listed in `legacy/RUNTIME-CHANGES.md`, with the commit and what the Rust
runtime must do about it, and must be ported before the beta ends. `scripts/dev/legacy-runtime-changes.sh` lists the
commits that changed the frozen files since the freeze, so the list can be checked against the history.

## Decisions for review

These choices shape the later phases and are the points to review:

1. **A supervisor as process 1,** instead of the start-up program itself, so a Rust failure cannot panic the kernel;
   after a crash it restarts the start-up program without the countdown and the automatic boot.
2. **The state directory stays the only shared state,** with the legacy layout, instead of a server process the
   others would ask over a socket: a crashed server would take the menu with it, hooks and users need the files, and
   the documentation names some of them. One internal file is new: `.stages`.
3. **Every applet reads `/etc/zfsbootmenu.conf` itself,** like the legacy scripts, so a menu started over SSH works
   as one started on the console.
4. **Applets keep the legacy names, arguments and subcommands,** so `fzf` bindings, documentation and habits stay
   valid; LUKS unlocking at start-up is `zluks unlock` on the plain console (decision 11).
5. **The compatibility library covers every function** of the three legacy libraries, checked by a test.
6. **Secrets never touch files the runtime creates,** which is stricter than the legacy code but invisible to users.
7. **The LUKS unit suites drive the Rust code through a test build of `zbm`** with the same seams as the legacy
   adapter; release images never contain it. (Superseded: the suites and the test build are gone.)
8. **The console is driven with `stty`, not with terminal FFI,** which keeps `unsafe` out of the console code at the
   cost of a few short-lived processes per prompt.
9. **Kernels follow the version order of GNU coreutils,** where the legacy images disagree with each other.
10. **No third-party crates in the runtime** so far: SHA-256 and PBKDF2 for the TPM2 PIN are small enough to write and
    test against `sha256sum` and `openssl kdf`, and wiping memory uses the C library's `explicit_bzero`.
11. **On the menu's start-up view, the start-up program unlocks LUKS volumes itself** (R5 step 4), where it ran
    `zluks unlock` as a process of its own so that secrets left memory when it ended. The secrets are typed into the
    view's dialogs, in the start-up program, and a second process would need a second terminal program or a protocol
    for every dialog. They stay in `Secret`s, which are wiped when the unlocking run drops them, and the view reads
    keys into buffers it wipes (see [Secrets](#secrets)), so the start-up program holds no copy afterwards; the VM
    test `secrets-passphrase` searches its memory before `kexec`. The plain console keeps `zluks unlock`.
