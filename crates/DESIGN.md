# Design of the Rust runtime

This document describes how the Rust runtime of ZFSBootMenu is built: the program that replaces the Bash code in
`legacy/zfsbootmenu/` inside the boot image. It is the outcome of Phase 4 of [PORT-ROADMAP.md](../PORT-ROADMAP.md) and
the reference for phases 5 and 6. The roadmap's rules apply throughout: the legacy runtime is the reference, the
behaviour does not change, and every interface in its compatibility contract stays as it is.

**Status:** draft, awaiting review. Changes to this design are made here first, in the same change as the code that
needs them.

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
and `zbm` runs the applet named after the path it was started as. The paths, names and arguments of the legacy
executables stay the same, so documentation, `fzf` key bindings and user habits keep working.

A boot runs these processes:

```text
PID 1   zbm, as zfsbootmenu-init --supervise   the supervisor: never exits, reaps orphans
└── zfsbootmenu-init                           start-up sequence, countdown, automatic boot
    ├── early-setup.d, setup.d, ... hooks      shell scripts, unchanged
    ├── zluks unlock-all                       LUKS unlocking, in its own process
    ├── zfsbootmenu                            the menu (also started over SSH or from the recovery shell)
    │   └── fzf                                draws a screen
    │       └── zfsbootmenu-preview, -help,    started by fzf for previews and key bindings
    │           zlogtail, ztrace, -diff
    └── bash -i                                the recovery shell
```

The processes share state through the files under `/zfsbootmenu`, as the legacy runtime does, and each process keeps
its own data in memory while it runs. External programs (`zfs`, `zpool`, `cryptsetup`, `kexec`, `gpg`, `fzf` and the
rest) stay the same, and are run through one interface that tests replace.

## Crates

The runtime is split into crates by responsibility. Library crates know nothing of the processes they run in; the
`zbm` crate turns them into applets.

| Crate | Responsibility | Legacy counterpart |
| --- | --- | --- |
| `zbm-sys` | The only FFI: signals, `kill`, reaping orphans, credentials, wiping memory. The only crate with `unsafe`. | Bash built-ins |
| `zbm-core` | Running programs (`Command`, `Runner`), the `Error` type, `Secret`, file tests, shell quoting, `getopts`, release information. | parts of `zfsbootmenu-core.sh` |
| `zbm-system` | The interfaces to the system (files, clock, console, processes), their real implementations, the `System` bundle, and the kernel log writer. | `/dev/kmsg`, `tput`, `read`, `sleep`, `kill` |
| `zbm-state` | The state directory `/zfsbootmenu`: its layout, and reading and writing each file. | `${BASE}/*` |
| `zbm-log` | Message levels and the formats of the kernel log, reading the log back. | `kmsg-log-lib.sh`, `echo-log-lib.sh` |
| `zbm-cmdline` | The ZFSBootMenu command line, `zbm.*` options, kernel command lines and `%{parent}`. | `zfsbootmenu-kcl.sh`, `zfsbootmenu-parse-commandline.sh` |
| `zbm-zfs` | `zfs` and `zpool`: properties, listings, pools and datasets, mounts, importing and exporting. | the ZFS parts of `zfsbootmenu-core.sh` |
| `zbm-luks` | Package B: crypttab and options, finding volumes, every key method, closing volumes. | `zfsbootmenu-luks.sh`, `zfsbootmenu-luks-keys.sh` |
| `zbm-boot` | Packages A, C and D: the start-up sequence, hooks, pool import, boot environments, kernels, ZFS encryption keys, `kexec`, snapshots and pool management. | `init.d/*`, `zfsbootmenu-core.sh` |
| `zbm-menu` | Package E: the screens, `fzf`, previews, help, the command line editor and the power menu. | `zfsbootmenu-ui.sh`, `fzf-defaults.sh`, `libexec/*` |
| `zbm` | The multi-call program: one applet per legacy executable, the recovery commands (package F) and the supervisor. | `bin/*`, `libexec/*` |
| `zbm-testutil` | Fakes of every system interface, the legacy code runner for parity tests, seeded inputs. Tests only. | — |

The host tools `generate-zbm` and `zbm-kcl` keep their own crates and share `zbm-core`, `zbm-cmdline`, `zbm-log` and
`zbm-zfs` with the runtime.

Dependencies point one way: `zbm` → `zbm-menu` → `zbm-boot` → `zbm-luks` → `zbm-state`, `zbm-zfs`, `zbm-cmdline` →
`zbm-system` → `zbm-log`, `zbm-core` → `zbm-sys`. `zbm-boot` depends on `zbm-luks` because pool import retries
unlocking volumes that appear late. Third-party crates are vendored and need a reason; so far the runtime needs none.

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
  The list is sorted by `zbm.sort_key`, with `bootfs` last, as `find_be_candidates` sorts it.
- **Kernel:** a boot environment, a kernel path and its initramfs path, relative to the boot environment's root, with
  the version taken from the file name; kernels are kept in version order, as `sort -V` orders them, and the default
  is the last one or the one `org.zfsbootmenu:kernel` selects.
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

## Process model

### Processes

The runtime keeps the legacy process structure, for three reasons:

- **The menu can be started more than once.** A user connected over SSH, or in the recovery shell, runs
  `zfsbootmenu`, which takes over from the instance on the console. That needs the menu to be a separate process.
- **`fzf` starts other programs.** Previews and key bindings run commands, which are new processes and cannot share
  memory with the menu.
- **A crash must not take the machine down.** In the legacy runtime, `zfsbootmenu-init` is process 1: dracut sources
  its `pre-mount` hooks, and the mkinitcpio hook runs in the initramfs `init`, so the `exec` in the start-up script
  replaces process 1. Bash survives failing commands, but a Rust panic or abort in process 1 would make the kernel
  panic. So process 1 is a small supervisor that runs the start-up program as its child and never exits.

| Process | Applet | Does |
| --- | --- | --- |
| Supervisor | `zfsbootmenu-init --supervise` | Process 1. Starts `zfsbootmenu-init` on the console, reaps orphaned processes, and if `zfsbootmenu-init` ends for any reason, logs it, opens a recovery shell and starts it again. Contains no ZFS logic. |
| Start-up | `zfsbootmenu-init` | The start-up sequence, the countdown and the automatic boot, then the loop of menu and recovery shell, as in `libexec/zfsbootmenu-init`. |
| Menu | `zfsbootmenu` | The menu loop of `bin/zfsbootmenu`. Holds `/zfsbootmenu/active`. |
| `fzf` callbacks | `zfsbootmenu-preview`, `zfsbootmenu-help`, `zfsbootmenu-diff`, `zlogtail`, `ztrace`, `zfunc` | Short-lived; read the state directory and ZFS, draw or act, and exit. |
| LUKS unlocking | `zluks unlock-all` (an internal mode) | Runs in its own process, so that passphrases leave memory when it ends, as the legacy subshell in `init.d/45-luks-unlock` ensures. |
| Recovery shell | `bash -i` | Unchanged. The recovery commands it offers are applets. |

The start-up sequence runs in this order, as functions of `zbm-boot`; `/libexec/init.d` is not a directory of user
scripts, so the order is code, not files:

| Stage | Legacy |
| --- | --- |
| Parse the command line, write `/etc/zfsbootmenu.conf` and the cached command line | `pre-init/zfsbootmenu-parse-commandline.sh` (dracut `cmdline` hook) |
| Console size, kernel messages off, host name, start the supervisor on the console | `pre-init/zfsbootmenu-preinit.sh` (dracut `pre-mount` hook) |
| Mount `efivarfs` read-only | `zfsbootmenu-init` |
| Kernel modules, `udevadm settle` | `init.d/10-kmods` |
| Host ID | `init.d/20-hostid` |
| Wait for `zbm.wait_for` devices | `init.d/30-device-wait` |
| External hook root, `zbm.skip_hooks`, `early-setup.d` hooks | `init.d/40-early-hooks` |
| LUKS unlocking | `init.d/45-luks-unlock` |
| Pool import, retries, host ID matching, restricted pools, unsupported features, `bootfs` | `init.d/50-import-pools` |
| Mark the start-up done (`initialized`) | `zfsbootmenu-init` |

The dracut and mkinitcpio hooks stay shell scripts, as the roadmap requires. With `zfsbootmenu_runtime=rust`, they
install hooks that run `zbm` (`zbm zfsbootmenu-parse-commandline`, `zbm zfsbootmenu-preinit`) instead of the legacy
scripts; the dracut `initqueue` checks (`zfsbootmenu-ready-set.sh`, `zfsbootmenu-ready-chk.sh`) stay as they are.

### Shared state: the state directory

The processes share state only through files under `/zfsbootmenu` (`BASE`), with the legacy layout and formats.
`zbm-state` owns the layout: no other code builds these paths. Keeping the layout has a cost, since files are slower
than memory, but it is small next to the `zfs` commands the runtime runs, and it buys three things: users and
documentation rely on some of these paths (remote access removes `active`, the GRUB migration guide works in
`environments/<dataset>/mnt`, `zluks(8)` documents the LUKS files); a hook or a user in the recovery shell sees what
they see today; and short-lived callback processes need no server to ask.

| File | Written by | Read by | Format |
| --- | --- | --- | --- |
| `initialized` | start-up, when done | menu | empty |
| `active` | menu, at start; removed at exit | menu (takeover), start-up (before the automatic boot), users | process ID and newline |
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
| `.keys/<key source>/<path>` | key caching | key loading | copies of ZFS key files |
| `.external_hooks` | external hook root | hook import | mount point |
| `have_errors`, `have_warnings`, `errors_since` | logging, error printer | previews, recovery shell, `zlogtail` | empty; time stamp |
| `luks-mappings`, `luks-skipped` | LUKS unlocking | teardown hook, `zluks`, retries | `name<TAB>device<TAB>auto` lines; one device per line |

Files that other processes may read while they are rewritten (`bootfs`, `bootenvs`, `kernels`, `.kcl` files) are
written to a temporary name and renamed, so a reader sees the old or the new contents, never a partial file. The
legacy code writes them in place; nothing can observe the difference except a reader that would have seen a partial
file. Files that are appended to (`degraded`, `luks-mappings`, `luks-skipped`) keep the legacy locking.

Within a process, data read from ZFS lives in the data model and is read again at the points where the legacy code
reads it again (each time a screen is drawn), so the Rust runtime sees changes made from a hook or the recovery shell
at the same moments.

### Configuration and environment

`zfsbootmenu-parse-commandline` parses the command line once (`zbm-cmdline`) and writes `/etc/zfsbootmenu.conf` as the
legacy scripts do, with the build-time variables (`HAS_*`, `ZBM_BUILDSTYLE`, `ZBM_RELEASE_BUILD`, `ZBM_RUNTIME`) and
the parsed options (`loglevel`, `import_policy`, `menu_timeout`, `zbm_*`, `control_term` and the rest). Rust applets
read their configuration from the environment, which the supervisor builds from that file at start-up; hooks, the
recovery shell and the compatibility library inherit the same variables as today. Variables that the legacy code sets
for one call (`read_write=yes import_pool`, `CLEAR_SCREEN`, `NO_CACHE`, `allow_rw`, `rewind_to_checkpoint`) become
function parameters inside Rust, and applets that hooks call accept them from the environment.

### `fzf` callbacks

The menu writes the screen's rows to a file or a pipe and runs `fzf` with the legacy options and key bindings. The
bindings and the preview command run applets by their legacy paths with the legacy arguments (`fzf` substitutes the
selected row for `{}`), for example `/libexec/zfsbootmenu-preview -b <bootfs> <dataset>`. The applet reads what it
needs from the state directory and from ZFS, as the legacy scripts do, and exits. `fzf` reports the key that ended the
selection (`--expect`), and the menu loop maps it to the next screen as `bin/zfsbootmenu` does; that mapping is a pure
function, which unit tests check for every key binding.

### Concurrent instances and takeover

The protocol stays as it is, because users rely on it (`remote-access.rst`): the menu writes its process ID to
`active`; a second `zfsbootmenu` sends `SIGUSR1` to that process, finds its deepest child through
`/proc/<pid>/task/<pid>/children` and ends it, so the blocked `fzf` returns; the first menu exits on `SIGUSR1`, which
removes `active`. The start-up sequence does not boot automatically while `active` exists, and opens the recovery
shell instead. Signals are delivered through `zbm-sys`; a handler only records the signal, and the menu checks for it
after each child process returns, which is when the legacy trap runs too.

### Signals

As in the legacy code, `SIGINT` is ignored by the supervisor, the start-up program and the menu, and child processes
(the recovery shell, hooks, `fzf`) get the default actions back. The supervisor reaps every child, including orphans
reparented to process 1, as Bash does in process 1 today.

`SIGPIPE` stays ignored in every runtime process, unlike in the host tools, which restore its default action. In the
legacy code, a value piped into a program (`printf '%s' "${pass}" | cryptsetup ...`) is written by a subshell, which
dies alone if the program exits without reading it. In Rust the runtime writes the value itself, so a write to a
closed pipe must fail with an error instead of ending the process.

## Hooks

Hooks stay shell scripts in the same directories (`/libexec/hooks/{early-setup,setup,load-key,boot-sel,teardown}.d`),
with the same rules as `zfsbootmenu-run-hooks`:

- Every executable file in the directory runs, in the order of its name in the C locale, with the terminal as its
  standard streams. Its exit status is logged and otherwise ignored.
- A stage reports whether any hook ran: `load-key.d` checks the encryption root again only then.
- `early-setup.d` hooks run once: afterwards they lose their permissions and are renamed to `*.completed`.
- `zbm.skip_hooks` removes the permissions of the named hooks before any stage runs, and `zbm.hookroot` imports hooks
  from another file system first.
- The environment is that of the start-up program (the variables of `/etc/zfsbootmenu.conf`, `BOOTFS` once it is
  known) plus the variables of the stage: `ZBM_LOCKED_FS` and `ZBM_ENCRYPTION_ROOT` for `load-key.d`;
  `ZBM_SELECTED_BE`, `ZBM_SELECTED_KERNEL`, `ZBM_SELECTED_INITRAMFS` and, for `boot-sel.d`, `ZBM_SELECTED_MOUNTPOINT`.

Running a hook is a `Command` with inherited streams; the hook environment is built by a pure function, so unit tests
check it for every stage.

### Compatibility library

Hooks may source `/lib/zfsbootmenu-core.sh` and `/lib/kmsg-log-lib.sh` (the hooks in `contrib/` do) and call their
functions. In the Rust image these two files are a small shell library in which each function calls `zbm`:

```sh
zinfo() { /libexec/zbm zfunc zinfo "$@"; }
load_key() { /libexec/zbm zfunc load_key "$@"; }
```

`zfunc` is the applet that `fzf` already uses to call a library function by name (`libexec/zfunc`), so the library
needs no new applet. It covers the functions a hook can use meaningfully: logging (`zdebug` to `zerror`), `colorize`,
`timed_prompt`, `emergency_shell`, `load_key`, `mount_zfs`, `import_pool`, `export_pool`, `set_rw_pool`,
`set_ro_pool`, `is_writable`, `is_snapshot`, `be_has_encroot`, `be_is_locked`, `is_mountpoint`, `is_efi_system`,
`mount_efivarfs` and `recursive_umount`. Functions that read implicit variables (`read_write=yes import_pool tank`)
find them in the environment, because the shell exports prefix assignments to the command it runs. Package F checks
the list against the hooks in `contrib/` and the functions documented in `zfsbootmenu(7)`.

## Errors and logging

### One error type

Library code returns `zbm_core::error::Result<T>`, whose error is one enum:

| Variant | Meaning | Example |
| --- | --- | --- |
| `Io` | A file or device operation failed; carries what was attempted and the `io::Error`. | writing `/zfsbootmenu/bootfs` |
| `Failed` | A program ended unsuccessfully; carries the command (without its input), its status and its output. | `zpool import -N -a` |
| `Invalid` | Input that cannot be used, with the message the legacy code logs. | an unknown crypttab option |
| `Cancelled` | The user cancelled: Escape at a prompt, an empty passphrase where that means "skip". | |

An error's text is the message the legacy code logs with `zerror` at that point; a `Failed` error also carries the
command's output, which is logged with `zdebug`, as the legacy code logs it. Functions that the legacy code lets fail
silently return `Option` or `bool`, not an error, so the Rust code does not report more than the legacy code does.

### Where errors go

The caller decides, at the same points as the legacy code:

1. **Log and continue:** most failures inside a stage (a kernel without an initramfs, a key that failed to load).
2. **Tell the user:** a centred message with a countdown (`timed_prompt`), as the legacy code shows after a failed
   `kexec` or a prohibited read-write import.
3. **Open the recovery shell:** `emergency_shell` with a message, after which the menu starts again, as in the legacy
   loop in `zfsbootmenu-init`. The shell prints the errors logged since the last time
   (`recovery-error-printer`, `have_errors`).

Panics are bugs. In the start-up program and the menu, a panic hook writes the panic message to the kernel log as an
error before the process ends, and the supervisor opens the recovery shell, which shows it. No code in the supervisor
may panic.

### Logging

Messages go to `/dev/kmsg` in the legacy formats (`<N>ZFSBootMenu: message`, and `<7>ZBM:[pid]|trace|message` for
debugging output, with the Rust caller as the trace), filtered by `loglevel`, and warnings and errors create
`have_warnings` and `have_errors`. `zbm-log` formats the messages and `zbm-system` writes them through the files
interface, so unit tests see every message. The recovery commands read the log back with `dmesg`, as now.

## External programs

### Running programs

Every program is run through the `Runner` trait of `zbm-core` with a `Command`: program, arguments, standard input
(inherited, `/dev/null`, bytes or a secret), standard output and standard error (inherited, captured, discarded, or
captured as a secret), and environment changes. `SystemRunner` runs it; a program that does not exist returns status
127, as a shell reports it. Interactive programs (`fzf`, `zfs load-key -L prompt`, `bash -i`, hooks) inherit the
terminal.

- **Building commands** is a pure function for each operation (`zpool import` with its options, `kexec -l` with its
  command line, `cryptsetup open` with its options), so unit tests compare the exact arguments with the legacy ones.
- **Parsing output** is a pure function from text to the data model, tested with output recorded from the real
  programs (`crates/*/tests/fixtures/`, recorded by `scripts/dev/record-fixtures.py` and in the VM tests).
- **Exit statuses** keep their meaning: commands killed by a signal report 128 plus the signal, as a shell does.
- **Locale and environment:** programs inherit the environment of the image, as now; only the variables the legacy
  code sets are set (`TPM2TOOLS_TCTI`, `GNUPGHOME` and the hook variables).

## Secrets

Passphrases, PINs, YubiKey responses, FIDO2 secrets, unsealed TPM2 secrets and decrypted key files are secrets.

- **In memory,** a secret is a `zbm_core::secret::Secret`: its bytes are wiped (with `explicit_bzero`, through
  `zbm-sys`) when it is dropped, it grows without leaving copies behind, and it prints as `Secret(..)`, so it cannot
  end up in a log message or an error by accident.
- **Into a program,** a secret goes only on standard input (`Input::Secret`), for example
  `cryptsetup open --key-file=- ...`, `gpg --passphrase-fd 0`, or `tpm2_unseal -p session:<context>+file:-`.
- **Out of a program,** a secret is captured from standard output into a `Secret` (`Stream::CaptureSecret`), for
  example the output of `gpg --decrypt`, `tpm2_unseal`, `fido2-assert`, `ykchalresp` and `clevis decrypt`.
- **From the user,** a secret is read from the console with echo off, directly into a `Secret`.
- **Derived values** (the SHA-256 authorization value of a TPM2 PIN, systemd's PBKDF2-salted PIN) are computed in the
  process instead of by `sha256sum` and `openssl`.
- **Lifetime:** secrets that unlock several volumes are kept for one unlocking run and dropped at its end; the run is a
  process of its own, so nothing remains in the memory of the start-up program.

The legacy code is close to this but not the same, and the Rust runtime is stricter where nothing observable changes:

- It decrypts GnuPG key files, and stores unsealed TPM2 and FIDO2 secrets, in temporary files below `/run` (RAM) and
  passes those files to `cryptsetup`; `zfsbootmenu-luks(7)` documents this. The Rust runtime passes them on standard
  input instead, and creates no such files.
- It passes the TPM2 authorization value derived from a PIN to `tpm2_unseal` as an argument, and a salted PIN to
  `openssl kdf` as an argument, where any process could read them in `/proc` while the program runs. The first is
  fixed in the legacy code as well, since `tpm2_unseal` accepts the value on standard input. The second cannot be
  fixed there without another program, because `openssl kdf` only takes the password as an argument; it is recorded
  in the tracking issue and disappears with the legacy runtime.
- ZFS key files cached from an `org.zfsbootmenu:keysource` (`/zfsbootmenu/.keys`) remain files, because the feature
  is defined as copying them and `zfs load-key` reads them by path. They are copies of files the user stored in a
  pool, not secrets the runtime created.

## Testing

### System interfaces

All code that reaches the system does so through these interfaces, which `zbm-system` defines (except `Runner`, which
is in `zbm-core`) and which a `System` value bundles, so a function needs only one parameter for all of them.

| Interface | Covers | Real implementation | Fake (`zbm-testutil`) records |
| --- | --- | --- | --- |
| `Runner` | every external program | `SystemRunner` | each command with its input; answers from responders |
| `Files` | the state directory, `/proc`, `/sys`, `/dev/kmsg`, mount points, hook directories, waiting for device nodes | `RealFiles` (the standard library) | an in-memory tree with modes, times and symbolic links; every write |
| `Clock` | countdowns, retry delays, waiting, time stamps | `SystemClock` | the time, which only advances when the code sleeps; every sleep |
| `Console` | prompts, key presses, passphrases, the screen size, output | `Terminal` (the control terminal, with `stty` for its modes) | scripted key presses, lines and secrets; everything written |
| `Processes` | its own process ID, signals to other processes | `SystemProcesses` (`zbm-sys`) | every signal sent |

Devices are files (`/dev/disk/by-*`), so waiting for them is `Files` and `Clock`; mounting, `kexec` and module loading
are programs. The fakes check what must not happen as well: `FakeSystem::assert_no_leak` fails if a secret's bytes
appear in any argument, environment variable, written file or console output.

### How each package is tested

| Package | Unit tests | Suites that must pass against the Rust image |
| --- | --- | --- |
| Phase 5 (minimal boot) | import arguments, `bootfs`, kernel selection, `kexec` arguments, with fakes | a new VM test that boots an unencrypted pool |
| A: start-up and import | every import policy, retry and host ID case; the stage order; hook environments | upstream test VMs; LUKS VM tests for import policies, host IDs and late devices |
| B: LUKS | crypttab and options (parity with the Bash parser), key method order, token metadata from recorded `luksDump` output, no leaks | LUKS unit suites; every LUKS VM test |
| C: boot environments | sorting, kernel selection, command line cache, key sources, hook environment | every VM test that boots |
| D: snapshots and pools | the commands each action runs | a new suite test per action, written against the legacy image first |
| E: menu | the screen that follows each key; snapshot tests of each screen's text | the VM tests that drive the menu |
| F: recovery tools | the programs `zbm` needs, the recovery commands' logic | the `contrib/` hooks unchanged; image sizes compared |

Pure functions with a Bash counterpart get parity tests that run both on the same inputs (`zbm-testutil::legacy`), as
in phases 2 and 3.

### Driving the existing suites

- **LUKS unit suites:** they load the code under test through `tests/luks/unit/impl.sh`. With `ZBMTEST_IMPL=rust`, the
  adapter defines the `luks_*` functions the suites call as calls to `zbm` applets, so the same checks run against both
  implementations. Until package B exists, `rust` reports the suites as skipped (exit status 77).
- **VM tests and upstream test VMs:** they boot an image. CI builds a legacy and a Rust image
  (`zfsbootmenu_runtime=rust`) and runs every suite against both; from Phase 5 on, the results against the Rust image
  are the measure of progress.

## Freezing the legacy runtime

From Phase 4 on, the legacy runtime (`legacy/zfsbootmenu/`, and the dracut and mkinitcpio files in `packaging/` that
install it) gets bug fixes only. Each change is listed in the tracking issue, with the commit and what the Rust runtime
must do about it, and must be ported before the beta ends. `scripts/dev/legacy-runtime-changes.sh` lists the commits
that changed the frozen files since the freeze, so the issue can be checked against the history.

## Decisions for review

These choices shape the later phases and are the points to review:

1. **A supervisor as process 1,** instead of the start-up program itself, so a Rust failure cannot panic the kernel.
2. **The state directory stays the only shared state,** with the legacy layout, instead of a server process the
   others would ask over a socket: a crashed server would take the menu with it, hooks and users need the files, and
   the documentation names some of them.
3. **Applets keep the legacy names and arguments,** so `fzf` bindings, documentation and habits stay valid.
4. **Secrets never touch files,** which is stricter than the legacy code but invisible to users.
5. **The console is driven with `stty`, not with terminal FFI,** which keeps `unsafe` out of the console code at the
   cost of a few short-lived processes per prompt.
6. **No third-party crates in the runtime** so far: SHA-256 and PBKDF2 for the TPM2 PIN are small enough to write and
   test against `sha256sum` and `openssl kdf`, and wiping memory uses the C library's `explicit_bzero`.
