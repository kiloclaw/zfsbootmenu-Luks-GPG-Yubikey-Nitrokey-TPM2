# Roadmap

This is the one plan for ZFSBootMenu in Rust: the architecture it is heading for, the rules its code follows, how it
is verified, and the work left, in order. It replaces the four plans written while the project was a port of the
legacy Bash and Perl code (`PORT-ROADMAP.md`, `REFACTOR-ROADMAP.md`, the feature list that was `ROADMAP.md`, and
`docs/dev/console-roadmap.md`). They are kept unchanged in [docs/dev/history/](docs/dev/history/) as the record of
how the code got here; [Identifiers of the old plans](#identifiers-of-the-old-plans) says where each of their items
went.

The port no longer aims at doing what the legacy code does. Behaviour may change where it makes ZFSBootMenu more
scalable, modular, fast, secure or stable; what changes is recorded in `CHANGES-FROM-LEGACY.md`. Major rewrites are
allowed on the same grounds, provided the code follows the four rules of [Principles](#principles).

Identifiers stay stable, because code, tests and commits cite them: features keep their numbers (`ROADMAP.md #18`),
console items theirs (`C61`), benchmarks theirs (`B7`) and decisions theirs (decision 9). The work items of this plan
have identifiers of their own (`PM4`, `SE7`); a finished item keeps its number.

## Contents

- [Where things stand](#where-things-stand)
- [Should we start from a white paper?](#should-we-start-from-a-white-paper)
- [Principles](#principles)
- [Target architecture](#target-architecture): [processes and trust zones](#processes-and-trust-zones),
  [concurrency](#concurrency-inside-a-process), [crates and layers](#crates-and-layers),
  [the domain model](#the-domain-model), [the ZFS boundary](#the-zfs-boundary) and
  [the system boundary](#the-system-boundary), [the shell](#the-shell), [the screen](#the-screen),
  [C dependencies](#c-dependencies), [the boot image](#the-boot-image), [security](#security-architecture),
  [observability](#observability)
- [Rules for refactored code](#rules-for-refactored-code) and [how the rules are enforced](#how-the-rules-are-enforced)
- [Verification](#verification), with [benchmarks](#benchmarks) and [release gates](#release-gates)
- [Work items](#work-items), by workstream
- [Phases](#phases): the order of the work
- [Decisions](#decisions)
- [Fixes left from the review of 27 October 2026](#fixes-left-from-the-review-of-27-october-2026)
- [Not planned](#not-planned)
- [Identifiers of the old plans](#identifiers-of-the-old-plans)

## Where things stand

On branch `claude/zbm-refactor`, 27 October 2026:

| Part | State |
| --- | --- |
| Host tools | `generate-zbm` and `zbm-kcl` in Rust; the Perl and Bash ones are gone |
| Runtime | The Rust runtime boots every layout of the VM tests, unlocks LUKS with every key method, runs start-up and the countdown in the new menu (Nucleo, Ratatui), and its shell work on brush. Images build it with `zfsbootmenu_runtime=rust`; the legacy runtime is still the default |
| Menu | Main view, start-up view, unlock and import dialogs, snapshots (diff, clone, rollback, set default), command palette. Left: kernels, the command line editor, pools and LUKS volumes, recovery tools, help, power |
| Console on the display | `zbm.console=drm`: Alacritty's terminal drawn through DRM, TrueType fonts, XKB layouts, scrollback wiped at secret prompts, status line, bells. Milestones M1 to M3 done; M3's VM test not run yet |
| Features done | #1 secrets in locked memory, #5 reproducible images, #9 saved boot logs and `zbm report`, #20 console fonts |
| Verification | About 1,670 Rust tests without root, LUKS device suites, 7 fuzz targets, about 900 VM checks (the maintainer's machine, with KVM), metrics and coverage ratchets |
| Removed in October 2026 | The Perl and Bash host tools, parity check mode, shadow mode, the Bash LUKS unit suites, and the rule that every suite runs against both images |

The code is 129,000 lines of Rust in 27 crates. Five reviews of it on 27 October 2026 (the process model, the crates
and domain, the system boundary and image, the screen, security and verification) found the same thing from five
sides: the parts are sound and well tested, and the joints between them are where the port shows. The domain takes
strings and a console; components talk through PID files, signals and polling; the menu reaches the display through a
terminal it does not need; the boot path starts a process for every ZFS property; every process is root and holds
or can reach every secret. This plan fixes the joints.

## Should we start from a white paper?

**A white paper for the design, yes; a clean-sheet rewrite of the code, no.**

The [target architecture](#target-architecture) below is that white paper: the end state, written down before the
work, so that every step moves towards one design instead of ten local ones. Each part of it that changes a contract
between components gets an Architecture Decision Record in `docs/dev/adr/` before its code (the ADRs of
[phase 0](#phase-0-the-design-and-the-quick-fixes)), and the design changes only through a new ADR.

The code then moves there by evolution, one port or protocol at a time behind stable interfaces (the strangler
pattern), with a few modules written anew where nothing worth keeping is in them. The reasons:

- **The evidence is the asset.** About 1,670 tests, 900 VM checks on real images, the LUKS device suites against real
  volumes, fuzz targets and memory searches for secrets prove behaviour on hardware that no review can. A rewrite resets
  every "holds" of the threat model to "to verify", just before the security review that a release needs.
- **The faults are in the joints, not the parts.** The Elm menu, Alacritty's grid, the glyph rendering, the
  `KeyMethod` strategies, the typed ZFS port, `Secret` and the fakes are what the target is built from. What must change
  (how processes talk, which code may see the console, how the domain reaches ZFS) is a few thousand lines of glue.
- **The VM tests run only on the maintainer's machine.** A big-bang rewrite would break them all at once, and a failure
  could not be bisected. Steps that each pass the VM tests keep every failure attributable.
- **Each step leaves the branch releasable,** and the user-visible work (the menu, the features) goes on alongside.

Written anew, because the legacy code they copied is gone or because their shape is the problem:

| What | Why | Item |
| --- | --- | --- |
| `generate-zbm` and `zbm-kcl`, as one `zbm-host` crate | 2,500 lines emulate Perl (`Getopt::Long`, `YAML::PP`, `Data::Dumper`, `File::Glob`, `Sort::Versions`) and Bash quirks for programs that no longer exist | DM10 |
| The LUKS unlocker's core (`Unlocker`, the methods, the pilot) | A god object of 30 methods with two `KeyMethod` traits; the key handlers and the device suites stay | DM6 |
| The supervisor's loop | A blocking `waitpid` loop cannot restart the console or react to it | PM6 |
| The image assembly (`ImagePlan`) | `install-helpers.sh` is lists and distribution quirks in Bash, verified by diffing file lists | SB8 |
| The menu's run loops, as one front end | Three loops (start-up view, menu, demo) for one Elm model | UI3 |

## Principles

1. **Strict Rust-native idioms.** Types say what values are (newtypes, enums, `Option` instead of sentinels such as
   `"-"`); parse, don't validate; ownership and borrowing instead of shared mutable state; `Result` with typed errors
   for everything that can fail; iterators over index loops; no `unsafe` outside `zbm-sys`.
2. **Rust design patterns.** Ports and adapters (hexagonal architecture), strategy and registry for key methods and
   boot steps, typestate for resources with a life cycle (mappings, pools, mounts, the display, child processes), RAII
   guards for everything that must be undone, the Elm architecture for front ends, actors with typed channels for
   long-running work, builders for complex values, the command pattern for actions.
3. **The Rust Style Guide.** `rustfmt` (style edition 2024) and the width rules of `cargo xtask widths`, the API
   Guidelines as the review checklist (naming, conversions, `#[must_use]`, `#[non_exhaustive]`).
4. **Rustdoc as the documentation standard.** Every crate says what it is for and how it fits; every public item says
   what it does, its errors and panics, with an example that runs where it can. Documents in `docs/dev/` explain what
   spans crates, and link into rustdoc.

And four rules of the product:

5. **Compatible where users depend on it.** Kernel command line options, `org.zfsbootmenu:*` properties, the
   configuration files and the hook environment stay compatible with upstream ZFSBootMenu, so boot environments work
   with either; a change of them is a decision with a deprecation period, not a refactoring.
6. **Safe defaults.** Features that change how a system boots are off unless configured; warnings and better messages
   may be on by default. Security fixes are on by default.
7. **Every change is tested and documented** at the lowest layer that can show it ([Verification](#verification)), and
   every security feature names what it protects against in `docs/dev/threat-model.md`.
8. **Measured, not claimed.** No speed or size claim without a [benchmark](#benchmarks).

## Target architecture

### Processes and trust zones

Four kinds of process, each with one job and one event loop, in four trust zones. They talk over typed, versioned
messages on Unix sockets (`SOCK_SEQPACKET`, `SO_PEERCRED` checked), never through PID files, signals or polling.

```text
 Z0  zbm-init (PID 1, root)          supervises the others; parses no untrusted input; holds no secret
      │  pidfd + signalfd; a table of services with restart policies
      ├──────────────┬────────────────────────────┬──────────────────────────────┐
 Z2  session (root)  │                        Z1  zbm-unlock (root)          Z3  zbm console (unprivileged)
      engine: App,   │ UnlockRequest ─────────▶  holds every secret;              DRM, evdev, pty opened, then
      imports,       │ ◀───────── UnlockOutcome  runs cryptsetup, TPM2,           privileges dropped; seccomp,
      mounts, hooks, │                           FIDO2, GnuPG, YubiKey;           landlock; draws surfaces,
      kexec          │                           extends the PCR before kexec     reads keys
      │ Snapshot, Event ▼  ▲ Action                                               ▲ cells, input mode
 Z3  menu (unprivileged): the Elm front end, a client of the session; ──── cells ──┘ keys
      on the console's surface, or on a terminal (serial, SSH)
```

- **Z0, `zbm-init`.** The supervisor that runs as PID 1. It polls a `signalfd` and one `pidfd` per child, and runs a
  table of `Service { program, restart: RestartPolicy }` (resume, back-off, never). It starts the console, the session
  and the shell, and restarts each by its policy (C89). After a crash it applies a `RecoveryPolicy`: `Open` gives the
  recovery shell as today, `Locked` gives the PIN screen (#8). It is panic-free under the strict lints, and opens no
  socket.
- **Z1, `zbm-unlock`.** The keyholder: the only process that holds passphrases, PINs, key files and unsealed keys, and
  the only one that runs the LUKS2 and TPM2 code ([C dependencies](#c-dependencies)). It answers
  `UnlockRequest { volume, method, credential: Option<Secret> }` with `UnlockOutcome`; a credential is moved into the
  message and wiped after use. It extends the PCR of measured boot (#10) after its last unseal.
- **Z2, the session.** The engine: `App` and the domain, which imports pools, mounts boot environments, runs hooks and
  loads kernels. It parses hostile input (pool and dataset names and properties, files in `/boot`, crypttab, hook roots)
  into checked types and holds no secret. It checks every action a client sends against the policy (the locked menu,
  the key map) before it acts.
- **Z3, the front ends.** The menu, a client of the session that renders snapshots and sends actions; `zbm console`,
  which owns the display and the keyboards. Both open what they need, then drop privileges (`no_new_privs`, an
  unprivileged user, a seccomp allow-list, landlock with no file access). A compromise of the terminal parser then
  leaks key presses, not root.
- **Over SSH,** `zfsbootmenu` is a client of the same session: it takes the menu over by a handshake (`Takeover` →
  `Yielded`) instead of a `SIGTERM` to whatever the menu runs.

The zones arrive in that order of need: the session as one process with the menu inside it first
([phase 2](#phase-2-the-session)), then the keyholder and the unprivileged front ends
([phase 5](#phase-5-isolation-and-boot-integrity)). The message types are defined once, in `zbm-app`, so moving a
component into its own process changes the transport, not the code.

### Concurrency inside a process

One model for the whole runtime (decision 10): **threads and channels, one reactor per process, no async runtime.**

- A process has one thread that owns its state and polls its file descriptors (`zbm_sys::console::wait_ready`): the
  terminal or display, a `Mailbox` waker (an `eventfd` paired with `std::sync::mpsc`), a `signalfd`, a uevent netlink
  socket, its sockets.
- Long work runs on workers: the session's `MachineService` actor owns `App`, takes `Command { action, reply }` and
  streams `Event`s back, so the menu redraws, counts down and cancels while Argon2 or `zpool import` runs.
- A question from the domain to the user (a passphrase, a retry) is a request with a one-shot reply channel, not a
  nested event loop.
- Every command has a `CancelToken`; the `Runner` kills the child's process group on cancel, through its `pidfd`.
- Parallel work runs in `std::thread::scope` with a bounded `WorkPool`, and a `MemoryBudget` whose RAII permits are
  sized for Argon2 (about 1 GiB each). TPM PIN attempts go through one `Mutex<Tpm>`, so they stay one at a time by
  construction.
- The ports are `Send + Sync`; tracing and the kernel log are per process, not per thread.

### Crates and layers

27 crates become 24. Dependencies point down only; `cargo xtask check-deps` enforces the table, and two new rules
join it: orchestration does not use adapters, and front ends reach the system only through `zbm-app`.

| Layer | Crates | Responsibility |
| --- | --- | --- |
| unsafe | `zbm-sys` | The few system calls `rustix` lacks: `memfd_secret`, seccomp, landlock, `kexec_file_load`; everything else moves onto `rustix` (29 `unsafe` blocks to about 6) |
| types | `zbm-types` | Domain values with no I/O: names, `KernelCommandLine`, `KernelSelector`, `HostId`, `Timeout`, `LogLevel`, questions and answers, the message types' building blocks |
| foundation | `zbm-trace` | Structured events, the journal, the kernel log |
| platform | `zbm-platform`, `zbm-display` | The ports and their real implementations, one `Context` (no console in it), the reactor's building blocks; the display, fonts (the PSF parser moves here from `zbm-types`) and keyboards |
| state | `zbm-config`, `zbm-state` | The option table and typed options; the documented state directory, with `flock` locks |
| adapters | `zbm-zfs`, `zbm-crypt`, `zbm-tools` | One port trait per capability (`Zfs`, `LuksVolumes`, `Tpm2`, `Fido2`, `KernelLoader`, `Mounts`, `Modules`, `Devices`), with in-memory fakes behind a `fake` feature |
| domain | `zbm-luks`, `zbm-pool`, `zbm-bootenv`, `zbm-boot` | Unlocking with a registry of key methods; pool import as a state machine; boot environments, kernels, command lines and key sources (`zbm-bootenv`, new, split out of `zbm-boot`); loading and booting a kernel with teardown |
| application | `zbm-app` | The `Application` trait (`snapshot`, `apply`, `tick`), its `App`, the `Action`, `Event` and `Snapshot` vocabulary, and the session's messages |
| orchestration | `zbm-startup` | Start-up as a pipeline of steps over `zbm-app` |
| front ends | `zbm`, `zbm-tui`, `zbm-console`, `zbm-brush`, `zbm-host` | The multi-call runtime binary; the menu; the console; the shell; the host tools (`generate-zbm` and `zbm-kcl` as two binaries of one crate, new) |
| tools | `xtask`, `zbm-vmtest`, `zbm-testkit` | Developer automation, the VM harness, the test kit |

Gone: `zbm-menu` (empty), `zbm-core` (Bash and Perl emulation: `getopts`, shell semantics, Perl's file tests; its
useful parts move into `zbm-platform`), `zbm-coreutils` (into `zbm-platform`), `generate-zbm` and `zbm-kcl` (into
`zbm-host`).

### The domain model

- **One way to inject the system.** Services take `&Context` and the ports they need (`Bootenvs::new(cx, &dyn Zfs)`);
  the `System` bundle, which carries a console into the domain, goes. Domain crates never take `Runner`: they reach
  programs only through adapter ports, which `check-deps` enforces.
- **One port to the user.** `trait Interaction` (`ask(Question)`, `secret(SecretPrompt)`, `notify(&Event)`,
  `lease_console()`, `shell(reason)`) replaces `Prompter`, `Operator`, `Recovery` and the questions of `Front`. Only
  front ends implement it, so no domain code draws, centres text or calls `tput`.
- **Values, not strings.** `DatasetName`, `PoolName`, `MappingName` and `DevicePath` everywhere a name is meant;
  `KernelCommandLine(Vec<KernelArg>)` instead of newline-joined text; `KernelSelector` wraps a compiled `regex::Regex`
  for `org.zfsbootmenu:kernel`, instead of a shell per kernel; properties are `Option<T>`.
- **Typestate for life cycles.** `Mapping<Closed>` → `Mapping<Open>` in the real unlocker (only the pilot uses it
  today); `Pool<ReadOnly>` → `Pool<Writable>` through `import_rw`, so only a writable pool can be
  changed; `Mounted<'cx>` unmounts on drop; `LoadedKernel::exec(self, Teardown) -> !`; `Child<Running>` → `Exit` in
  the supervisor.
- **Registries for extension.** `trait KeyMethod { id, applies, try_open }` with the order as data (a slice, later
  configurable); token handlers become methods through `TokenMethod<H: TokenHandler>`, one registry. Boot steps and
  built-in hooks are `trait BootStep`, registered in one table, which `zbm.skip_hooks` reads by name. A new key method,
  boot step or menu view is one new type registered in one place (G4 of the old plan).
- **Errors with causes.** One `thiserror` enum per crate, not `Clone`, with `#[source]`; `Step::run` returns
  `Result<StepOutcome, StartupError>`; an unlock failure keeps every `Attempt`; binaries return `ExitCode` and print the
  cause chain; stable error codes (`ZBM-E0412`) that `zbm explain` describes.
- **A small public surface.** `pub(crate)` by default, a curated facade in each `lib.rs`, `#[non_exhaustive]` on enums
  that will grow.

### The ZFS boundary

- **No libzfs** (decision 11): no bindings to `libzfs`, `libzfs_core` or `libnvpair`, whose ABI changes between
  OpenZFS releases. The speed problem is the number of calls, not the CLI.
- **The `Zfs` port on every path.** The raw `ZfsCommand` and `ZpoolCommand` builders, which the boot path uses today,
  go; every call goes through `trait Zfs`, with `ZfsCli` (JSON with `-j -p`, the `-H -p` text fallback for OpenZFS 2.2)
  and `FakeZfs`.
- **Batched.** One `zpool list -j` for all pools and one `zfs list -j -p -r -o <the properties ZFSBootMenu reads>` per
  pool build the snapshot; the boot code reads properties from it, not with one `zfs get` each.
- **Mounting without programs:** `mount(2)` and `umount2(2)` with `zfsutil`, behind the `Mounts` port, with
  `mount.zfs` as the fallback.
- The text fallback goes when the oldest supported distribution ships OpenZFS 2.3.

### The system boundary

Each operation is placed in one of three layers:

| Layer | What | How |
| --- | --- | --- |
| System calls | Mounting, `kexec_file_load` and `reboot(LINUX_REBOOT_CMD_KEXEC)`, `setsid` and the controlling terminal, pipes, `sync`, dropping caches | `rustix`, or `zbm-sys` where `rustix` lacks the call; no process |
| Rust in place of C, in `zbm-unlock` only | LUKS2 key slots and dm-crypt mappings, TPM2 commands, FIDO2, YubiKey challenge-response, GnuPG key files, the Clevis TPM2 pin ([C dependencies](#c-dependencies)) | Behind the same port traits as the programs they replace, with those programs as the fallback; no C library is linked, so every process stays linked to glibc only and a missing library cannot stop the boot |
| Programs | `zfs` and `zpool`, `udevadm`, `modprobe`, `blkid`, `gpg` with `scdaemon` and `pcscd`, `clevis`, `kexec` (device trees, user hooks such as Xen), brush, uutils; `cryptsetup` and `tpm2-tools` for the recovery shell | Behind typed ports; every call has a time-out and a cap on its captured output, and runs without a shell wrapper |

- **Process plumbing.** `Runner` gains `spawn() -> Child` with kill and wait, a default time-out per adapter, a cap on
  captured output, and one pipe for standard output and error instead of `sh -c 'exec "$0" "$@" 2>&1'`, which today
  doubles the processes of 15 call sites and makes Bash a dependency of the Rust runtime.
- **Fakes are models, not scripts.** `FakeZfs`, `FakeLuks` (key slots, tokens) and `FakeTpm` (PCRs, lock-out) model
  behaviour; contract tests run one suite against each fake and, in a VM, against the real adapter. `FakeRunner` stays
  only for the CLI adapters' own tests, against recorded fixtures.

### C dependencies

`zbm` links no C library but glibc. The C code is in the programs the images run and in the host's tools. Each one
that a Rust implementation can replace without losing what users need is replaced, behind the same port as the
program, which stays as the fallback until the VM suites, and for the LUKS2 and TPM2 code the security review (SE13),
have passed it. Differential tests run the Rust code and the program on the same volumes and devices; parsers of
on-disk or device data take `Untrusted` input and are fuzzed; each new crate is a decision 1 entry and goes through
`cargo vet`. The image's size (B1) and its process count (B7) show the gain.

| C dependency | Used for | Rust replacement | Item |
| --- | --- | --- | --- |
| glibc's calls in `zbm-sys` (`ioctl`, `signal`, `mmap`, `mlock`, `poll`, `tcsetpgrp`) | System calls without a safe wrapper | `rustix`; glibc itself stays | SE6 |
| `stty`, `tput` (ncurses) | Terminal set-up | `rustix` termios and escape sequences | PM11 |
| `setsid`, `dmesg`, `lsblk`, `mount`, `umount`, `hostid` (util-linux) | Sessions, the kernel log, block devices, mounts | System calls, `/dev/kmsg`, `/sys/block`, `/etc/hostid` | SB1, SB3, CD2 |
| `openssl`, `sha256sum`, `base64`, `cut`, `tr`, `wc` | Hashing, encoding and text in the LUKS and TPM2 paths | RustCrypto's `sha2` and `hmac`, the `base64` crate | CD3 |
| `setfont` (kbd) | The kernel console's font | The `KDFONTOP` ioctl with the PSF parser | CD5 |
| `ykchalresp`, `ykinfo` (ykpers, at its end of life upstream) | YubiKey challenge-response | HID feature reports over `hidraw` | SB11 |
| `modprobe`, `insmod`, `modinfo`, `depmod` (kmod) | Kernel modules | `finit_module` with `modules.dep`, `modules.alias` and `modules.softdep` | CD4 |
| `kexec` (kexec-tools) | Loading the kernel | `kexec_file_load`; kexec-tools stays for device trees, Xen and old kernels | SB4 |
| `gpg`, `scdaemon` (libgcrypt) | GnuPG key files | The `pgp` crate (rPGP, MIT or Apache-2.0); `gpg` stays for OpenPGP cards | CD6 |
| `fido2-assert`, `fido2-token` (libfido2) | FIDO2 hmac-secret | CTAP2 over `hidraw`, with Mozilla's `authenticator` crate if it covers hmac-secret | CD7 |
| `clevis`, `jose`, `luksmeta`, `curl` | Clevis pins | The TPM2 pin natively, then Tang; `luksmeta` (LUKS1 only) goes | CD8, CD11 |
| `cryptsetup`, `dmsetup` (libcryptsetup, libdevmapper) | Opening LUKS2 volumes | Key slots with RustCrypto, the mapping with the `devicemapper` crate; `cryptsetup` stays for enrolment and as the fallback | CD9 |
| `tpm2_*` (tss2-esys) | Unsealing keys, extending PCRs | The TPM2 commands the key methods use, over `/dev/tpmrm0`; `tpm2-tools` stay in the recovery shell | CD10 |
| `objcopy` (binutils) | Assembling the EFI image | PE sections written by `zbm-host` | CD12 |
| dracut, mkinitcpio, `cpio` | Building the initramfs | The native image backend | SB12 |

What stays in C, and why:

- **glibc:** Rust's standard library needs a libc on Linux; libc-free Rust is still experimental.
- **`zfs`, `zpool`, `zdb`, `mount.zfs`:** there is no Rust ZFS, and libzfs is ruled out (decision 11); SB3 needs
  `mount.zfs` only as a fallback.
- **`udevadm` and `systemd-udevd`:** no Rust udev is mature, and `cryptsetup` relies on the device-mapper rules.
- **`pcscd` and `scdaemon`:** OpenPGP cards (Nitrokey, YubiKey) are reached through PC/SC; the Rust card crates use it
  too.
- **The systemd EFI stub:** distributions ship and sign it for Secure Boot; a Rust stub would gain little.
- **`zstd` and `xz`:** the Rust compressors are weaker; the kernel decompresses the image.
- **Bash and the text tools** (`sed`, `grep`, `awk`, `ps`, `less`): for users' hooks and the recovery shell.

### The shell

ZFSBootMenu runs shell code for the recovery shell, user hooks and SSH logins. Rust images run it on brush
(decision 9); `zbm.shell=bash` and the recovery shell's fallback lead back to Bash.

- **Lighter brush.** Depend on `brush-core`, `brush-builtins` and `brush-interactive` instead of `brush-shell`, whose
  command-line layer brings `clap`, `human-panic`, `toml` and more; measure with B1.
- **No legacy payload.** Rust images stop installing `legacy/zfsbootmenu/{lib,libexec,bin}` and `fzf`. The recovery
  shell's functions come from a generated `zfunc` library that calls `zbm` applets (a test compares its list with the
  legacy functions), and the recovery tools are menu views and applets (R6 of the old plan).
- **A typed hook API.** User hooks stay executables, with the same environment, plus `ZBM_HOOK_API=2` and a read-only
  JSON context file (`$ZBM_HOOK_CONTEXT`) for hooks not written in a shell.
- **Patches** to brush go to `patches/` and upstream (`patches/README.md`).

### The screen

`zbm console` becomes the owner of the seat: the display, the keyboards and the virtual terminal. It shows
**surfaces**, and the menu draws its cells on one natively instead of through a terminal.

- **Two kinds of surface,** a closed enum: `Surface::Cells`, a grid of styled cells that a client fills, for the menu;
  `Surface::Terminal`, today's Alacritty terminal on a pseudo-terminal, for the shell, hooks and `/dev/console`. Layers
  on top: the status line, search, kernel messages (C77).
- **One renderer,** `zbm_display::render::CellRenderer`: a cell cache keyed by character and style (C71), the buffer
  mapped once (C72), several damage rectangles (C73), double buffering on vertical blank (C74). The demo's own pixel
  backend in `zbm-tui` goes.
- **The client channel.** A socket pair the supervisor hands the session: from the client, `Cells`, `Cursor`,
  `Input { secret }`, `SwitchLayout(name)`, `Announce(text)`; from the console, `Caps` (cells, true colour, which
  characters the font has), `Key(KeyPress)` with the layout's name, `Resize`, `Bell`. The secret flag travels in the
  same ordered stream as the keys, and passphrases no longer cross a pseudo-terminal, whose kernel buffer outlives them.
- **The seat as typestate:** `Seat<Held>` → `release()` → `Released` → `reattach()`, so nothing draws on a display that
  was handed back before `kexec`.
- **One key type.** `KeyPress` from the console, or decoded from terminal bytes for serial and SSH, is the menu's
  `Message::Key`; the menu's own key tables for the unlock dialog give way to the image's XKB keymaps, and Ctrl-K
  switches the console's layout (C105).
- **One front end runtime** for the start-up view, the main menu and the demo: `Frontend<B: Backend, I: Input>`, with a
  backend for the console's cells, one for a terminal (serial, SSH, the kernel console) and Ratatui's test backend. If
  the console goes away, the front end goes on on the kernel console with the same model.
- **Accessible.** The focused surface is exported as text for `brltty` (C85), with `Announce` lines; a line-oriented
  `view` over the same model serves `TERM=dumb`, speakup and serial users (#12's plain mode). The language is part of
  the model, so changing it is a message (#18).
- **Leases instead of hand-overs.** When the menu lends the screen to a console action, dropping the `SurfaceLease`
  gives it back and redraws once.

### The boot image

- **`ImagePlan`.** `zbm-host` computes the image as a value: files, the ELF closure, modules, firmware, udev rules and
  links, per runtime; a builder makes it, and golden files check it under `cargo test`. An `ImageBackend` trait installs
  it: `Dracut` and `Mkinitcpio`, whose `module-setup.sh` and install hook shrink to shims, and later `Native`.
- **Rust images list their own binaries,** without the legacy runtime's (`awk`, `sed`, `od`, `tac`, `openssl`, `fzf`
  and others).
- **A native backend, after the cutover,** for release and container images first: a `cpio` writer, the ELF closure
  with `object`, the module and firmware closure, and a minimal early init in `zbm-startup` that mounts the pseudo
  file systems and starts udev. It is checked against dracut's output with `xtask compare-images`. Dracut stays a
  backend, because remote unlock relies on its modules (`dracut-crypt-ssh` with dropbear). This reopens #13 in that
  narrowed form.
- **Signed and accounted for.** `generate-zbm` signs the unified kernel image itself (with `sbsign` or `ukify`), builds
  reproducibly (#5), and releases publish an SBOM, `cargo auditable` metadata, build provenance and the debug files of
  every binary, indexed by build ID.

### Security architecture

`docs/dev/threat-model.md` stays the reference: attackers, untrusted inputs, properties with their evidence, risks.
The architecture adds:

- **Trust zones** as [above](#processes-and-trust-zones): secrets in one process, hostile parsers in others, renderers
  without privileges.
- **One gate to a shell.** `ShellGate::open(&Authorized)` is the only way to start an interactive shell; `Authorized`
  is a zero-sized token that only `auth::verify` mints (Argon2id, a constant-time comparison, a growing delay), or that
  is minted at once without `zbm.lock`. A crash then leads to the PIN screen, not to a root shell with the volumes open.
- **Untrusted text cannot reach a terminal raw.** `Untrusted<T>` has no `Display`; rendering takes `Printable`, which
  escapes C0, C1 and DEL. Hostile names are fuzzed through `view`.
- **Boot integrity.** `kexec_file_load` whenever lockdown or Secure Boot is on, failing closed; the kernel, initramfs
  and command line measured into a PCR with a TCG event log and a prediction tool (#10); after a TPM-only unlock, the
  import policy is `strict`, a pool whose keys were not unlocked cannot give the default boot environment, and a hook
  root on a pool imported under another host's ID is refused.
- **Salted, encrypted TPM sessions,** so an interposer on a discrete TPM's bus reads nothing.
- **Audited crypto.** The hand-written SHA-256, HMAC, PBKDF2 and base64 of `zbm-luks` give way to the RustCrypto crates
  with `zeroize`; `argon2` for the PIN of #8.
- **Secrets at the edges.** No `String` copy of a typed passphrase (the masked field renders from the `Secret`);
  `Secret::ct_eq`; no implicit `Clone`; the journal keeps fields by an allow-list, not a deny-list; the key cache is
  written with `O_NOFOLLOW | O_EXCL` and mode 0600 and removed before `kexec`; `zbm.console_mirror` exists only in test
  builds.
- **Updates that cannot brick.** A/B images with a backup boot entry, boot counting (#7) with EFI variables written in
  a fixed order, saved logs capped and skipped when the ESP is low on space.
- **The supply chain.** `cargo vet` (importing the audits of Mozilla, Google and the Bytecode Alliance), duplicate
  versions denied with a list of allowed ones, actions pinned by commit, the build container's base pinned by digest.

### Observability

- Structured `tracing` events with domain fields, instead of format strings; the journal (`/zfsbootmenu/journal.jsonl`)
  with fields kept by an allow-list; a simple kernel log format instead of the legacy one.
- Stable error codes, explained by `zbm explain <code>`.
- Each zone reports its health to Z0; `zbm debug` reads every zone; crash reports hold no heap data and resolve with
  `cargo xtask symbolize`; `zbm report` bundles everything without key material, and `xtask replay` turns a bundle into
  a test.

## Rules for refactored code

1. **Formatting and lints:** `rustfmt` style edition 2024 with no options; the width rules (`cargo xtask widths`) are
   part of `xtask lint`; `clippy::all` and `clippy::pedantic` denied, each allow carrying a `reason`; `unsafe_code`
   denied outside `zbm-sys`; `missing_docs`, `rustdoc::broken_intra_doc_links` and `rustdoc::missing_crate_level_docs`
   denied.
2. **Documentation:** every crate starts with what it is for, how it fits the architecture and a short example; every
   public item says what it does, its errors (`# Errors`), panics (`# Panics`, ideally none) and has a doctest when it
   can run without a system. Docs explain intent and behaviour, not which legacy function it was.
3. **Size:** functions of at most 60 lines, modules of at most 600 lines including tests; a test module that grows
   past that moves to `tests.rs` next to the module.
4. **Types over strings:** no `String` or `bool` parameter that stands for a domain concept; `&str` only at the
   boundary (parsing input, printing output).
5. **Errors:** one error enum per crate, written with `thiserror`, carrying context (`#[source]`); `Result` for
   everything that can fail; non-error outcomes are enums. Binaries render the chain.
6. **Observability:** every public operation in a service runs in a `tracing` span named after it, with the domain
   identifiers as fields; every external command is logged once with its arguments (secrets redacted), duration and
   status.
7. **No hidden IO:** domain types and `update` functions take no context and perform no IO; services take a context.
8. **Secrets:** secrets reach programs only on standard input, are captured as `Secret`, and never reach arguments,
   the environment, files, logs or the screen; `zbm-trace` redacts `Secret` fields by type.
9. **Dependencies:** vendored, each with a written reason in `docs/dev/dependencies.md`, checked with `cargo deny`
   (licences, advisories, duplicates). A changed third-party crate is a patch in `patches/`, never an edit of
   `vendor/`.
10. **Everything new is Rust:** no new Bash, Perl or Python, including tests and tooling.

## How the rules are enforced

Every rule is checked by a tool in CI, so that developers and AI tools learn the rules from the build instead of from
review comments. `docs/dev/tooling.md` describes each command.

| Rule | Tool | How |
| --- | --- | --- |
| Lints (1) | `[workspace.lints]` in `Cargo.toml` | `clippy::pedantic`, the `rustdoc` lints and the no-panic lints, denied per crate as the phase that refactors the crate ends |
| No panics in the runtime | `clippy::unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `unreachable`, `todo` | Denied in every crate that runs at boot; tests are exempt (`allow-unwrap-in-tests` and `allow-expect-in-tests` in `clippy.toml`) |
| No hidden IO (7), adapters only | `disallowed-methods` and `disallowed-types` in `clippy.toml` | `std::process::Command`, `std::fs::*`, `std::env::var*`, `std::thread::sleep`, `SystemTime::now`, `Instant::now`, `println!` and `eprintln!` are disallowed; the real implementations allow them once per module, with a reason |
| Layering (front ends never import adapters) | `xtask check-deps` | Fails on any dependency edge that the layer table in `xtask/src/deps.rs` does not allow; `docs/dev/architecture.md` is generated from that table |
| Size (3), types over strings (4), errors (5) | `xtask metrics` | Counts long functions and files, success flags, legacy-named doc lines, shell helpers, tests that run Bash and `unsafe` blocks per crate, against `xtask/metrics-baseline.txt`; a count may only go down |
| Formatting and widths (1) | `cargo fmt --check`, `xtask lint` | `xtask lint` runs `rustfmt` and the width check |
| Dependencies (9) | `cargo deny check` | Licences, advisories, duplicate versions and banned crates (`libzfs` bindings among them, for decision 11) |
| Strict lints stay clean once reached | `xtask lint` with `xtask/strict/crates.txt` | The crates listed there must have no strict finding; each phase adds the crates it finishes |
| Test coverage only goes up | `xtask coverage` with `cargo llvm-cov` | `--check` fails when a crate's line coverage drops below `xtask/coverage-baseline.txt`; a new crate must reach 80% |
| Documentation matches behaviour | `xtask gen-docs --check` | Fails when a man page fragment generated from the option or key binding tables is out of date |

New checks, added by the items that need them:

| Rule | Tool | Item |
| --- | --- | --- |
| Domain crates reach programs only through adapter ports | `xtask check-deps` refuses `zbm_platform::Runner` in domain crates, and front ends that use domain crates other than through `zbm-app` | DM2, DM8 |
| No raw ZFS command lines outside `zbm-zfs` | `xtask metrics` column, ratcheted to 0 | SB2 |
| Names are newtypes in domain signatures | `xtask metrics` column: public domain functions taking a raw string | DM3 |
| Structured logs | `xtask metrics` column: format-string log macros | DM9 |
| No interactive shell outside its gate | `xtask` check: `Command::shell` and the shell programs only in `ShellGate` and tests | SE9 |
| Release images carry no test features | `xtask` check over the release profile's features | SE2 |
| Arithmetic overflow is caught | `overflow-checks = true` in the release profile | SE2 |
| Third-party code is audited | `cargo vet`; `cargo deny` with duplicate versions denied | SE12 |

## Verification

### Layers

| Layer | What | Where | Runs | Root |
| --- | --- | --- | --- | --- |
| Unit | Pure functions, parsers, `update`, state machines | Next to the code | Every `cargo test` | No |
| Component | One service on in-memory fakes (`FakeMachine`, `FakeZfs`, `FakeLuks`, `FakeTpm`) | `crates/*/tests/` | Every `cargo test` | No |
| Golden | Output compared with recorded files: screens, generated configurations, image plans, the results first recorded from the legacy code (`tests/golden/legacy`, now expected results that a deliberate change may change) | `tests/golden/`, `crates/*/tests/` | Every `cargo test` | No |
| Property | Parsers, orderings, message round trips, `Printable` never emitting control characters, start-up always ending in a menu or a shell | Next to the code | Every `cargo test` | No |
| Fault injection | Seeded schedules of failures, time-outs, `ENOSPC`, `EIO` and signals through `FaultyRunner` and `FaultyFiles` | Next to the code | Every `cargo test` | No |
| Fuzz | Every parser of untrusted input in the threat model, the message decoders, hostile names through `view` | `fuzz/` | Nightly CI, with ASan | No |
| Contract | One suite against each fake and its real adapter | `crates/*/tests/contract/` | VM | Yes |
| Device | Real `cryptsetup`, swtpm, GnuPG on loop devices | `crates/zbm-luks/tests/devices/`, `crates/zbm-crypt/tests/` | `cargo xtask test devices`, CI | Yes |
| System | Images booted in QEMU (`zbm-vmtest`), in three tiers: smoke (about 15 scenarios, CI with KVM), nightly, release (the full hour, the maintainer's machine) | `crates/zbm-vmtest` | `cargo xtask test vm` | Host with KVM |

Mutation testing (`cargo-mutants`) runs on the code where a missed case costs most: `zbm-luks`, `zbm-crypt`, the
command lines, the shell gate and the message codecs. Coverage counts test code apart from the code it tests.

### Conventions that make tests easy for people and for AI tools

- **One behaviour per test,** named as a sentence: `a_wrong_pin_counts_towards_the_lockout`.
- **Arrange, act, assert,** separated by blank lines; the arrange part uses builders such as
  `FakeMachine::new().with_luks_volume("/dev/sda2", "pw")`.
- **Tables for variations:** `cases!` or an array of structs with a `name` field, so a failure names the case.
- **Golden files instead of inline expectations** for anything longer than three lines, updated with
  `INSTA_UPDATE=always` or `xtask bless`.
- **Readable failures:** `pretty_assertions` diffs; `assert_ran!`, `assert_not_ran!` and
  `assert_no_secret_leaked!`.
- **Determinism:** no wall clock, sleep, network or randomness in unit and component tests.
- **Self-contained:** a test does not depend on another test's state, the host's tools or the legacy tree.
- **Discoverable:** `docs/dev/testing.md` maps features to test files.
- **Fast:** the whole default suite under one minute; `cargo nextest` profiles for CI.

### Benchmarks

The Rust runtime should be faster where it matters and no slower anywhere. These benchmarks turn that into numbers,
measured against the legacy runtime on the same machine. Until they run, no speed claim is made.

| # | What | How it is measured | Compared with | Budget |
| --- | --- | --- | --- | --- |
| B1 | Image size: the EFI image and the initramfs, and their parts (`zbm`, `brush`, fonts, keyboard layouts, uutils) | `generate-zbm` builds both images from one configuration; `cargo xtask bench size` lists the parts | The legacy image | Set from the first baseline; a rise of more than 10% needs a reason in the record |
| B2 | Time to the menu: from the kernel's start to the countdown's first frame | Kernel log timestamps and the journal's spans, in a VM with KVM | The legacy countdown; the kernel console and `zbm console` | No slower than legacy |
| B3 | Time to boot: from QEMU's start to `BE-INIT-START`, with the stage times from the journal | The countdown boot of a `runtime` test, five runs, median and spread | The legacy image | No slower than legacy |
| B4 | Key to screen: moving the selection, typing a filter, opening the snapshots and a dialog | In process on the fake machine, and end to end over the serial console in a VM | `fzf` and its previews | A frame (16 ms) in process; legacy's median end to end |
| B5 | Many snapshots: 100, 1,000 and 10,000 of one environment; opening, filtering, scrolling, memory | A pool built by the VM test, and in process with `FakeZfs`; UI10's performance test | The legacy menu | Matching and redraw within a frame at 10,000 |
| B6 | Bytes per redraw over a serial console, for the actions of B4 | The bytes the serial console receives in a VM, at 80×24 | The legacy menu | Each action's redraw within 100 ms at 115200 baud (1,150 bytes) |
| B7 | Processes started: during start-up, per menu action and per boot | The command journal and `/proc` counts in a VM | The legacy runtime, from `bash -x` traces | Fewer than legacy for every action; none for moving the selection |
| B8 | Memory: the peak resident memory of start-up, the menu, `zbm console` and the recovery shell | `/proc/<pid>/status` sampled in a VM | `bash` and `fzf` in the legacy image | Set from the first baseline |
| B9 | The console on the display: time to its first frame, `cat` of a 10 MB file, and a scrolling flood | VM tests with the console's mirror and timestamps (C112) | The kernel console | No slower than the kernel console once UI4 is done |
| B10 | Unlocking: from Enter to the volume open, without the key derivation | The `secrets` and `keys` VM tests' journal spans | The legacy runtime | The overhead no slower than legacy |
| B11 | Scale: start-up with 4 pools, 24 devices and 12 LUKS volumes | A `complex` VM layout grown for the benchmark | The legacy runtime | No slower than legacy |
| B12 | Building: an image with `generate-zbm`, and `cargo xtask test` | On the build machine and in the build container | The last release with the Perl `generate-zbm` | No slower than the Perl tool; `cargo xtask test` under a minute |

`cargo xtask bench` runs what needs no VM (B1, B4 and B5 in process, B12) and writes `target/bench.json`;
`zbm-vmtest` gets `bench-*` scenarios for the rest. Counts (bytes, processes, sizes) are checked in CI against
budgets; times are recorded with their median and spread over five runs, compared only on the same machine, in a
Benchmarks table of `docs/luks/testing.rst`. They run for a baseline, then at the exits of phases 3 and 4 and at
the beta (RL1), and for B9 at the ends of UI4 and UI9.

### Release gates

A release that makes the Rust runtime the default (RL1) needs:

- every property of the threat model reading "holds", with its evidence;
- the full VM suite passing on the release commit, and the hardware list of RL4;
- two builders producing the same bytes (#5);
- the SBOM, provenance and signatures published, and `cargo vet` clean;
- each fuzz target run for its agreed CPU hours without a finding;
- the independent security review (SE13) done, its findings fixed or accepted in writing;
- no benchmark worse than the legacy runtime's without a recorded reason.

## Work items

Each item has an identifier, the benefit it brings (**scal**ability, **mod**ularity, **perf**ormance, **sec**urity,
**stab**ility), an effort (**S** under a week, **M** a few weeks, **L** a few months, for one developer who knows the
code) and what it needs first. Code in the images changes with most items, so their VM runs need rebuilt images.

### PM: the process model

| ID | Item | Benefit | Effort | Needs |
| --- | --- | --- | --- | --- |
| PM1 | ADR for the process model and concurrency (decision 10): one reactor per process, workers own nothing global, typed messages, no async runtime | mod, stab | S | |
| PM2 | The ports become `Send + Sync`, the fakes use `Mutex`; `spawn_traced` carries tracing and the kernel log into workers; logging per process, not per thread | scal | M | PM1 |
| PM3 | One signal facility: `signalfd` and `pidfd` over `rustix`, a typed `SignalSet`; the global "last signal" slot and `signal-hook` go | stab | S | |
| PM4 | The reactor's parts in `zbm_platform::event`: `Mailbox`, `Reactor`, `CancelToken`, `MemoryBudget`, time-outs through `pidfd` instead of a watchdog thread per command | scal, perf | M | PM2, PM3 |
| PM5 | `flock` locks for the state directory (`luks-mappings`, `active`, the key cache) as RAII `StateLock`s: no lock taken anyway after 5 s, no stale PID | stab | S | |
| PM6 | The supervisor anew: an event loop over `signalfd` and `pidfd`, a table of services with restart policies and back-off, `setsid` and the controlling terminal in `pre_exec` (C89) | stab | M | PM3 |
| PM7 | The console's control socket (C91): release, reattach, layout, font and status as typed requests with replies; the secret flag in band | stab | S/M | |
| PM8 | The `MachineService` actor owns `App`; questions to the user become requests with one-shot replies (`Interaction`); actions can be cancelled | perf, stab | L | PM4, DM2 |
| PM9 | One session: the menu runs in the session process, the start-up view and the menu are one model; the `--countdown` child and the second countdown go | perf, stab | M/L | PM8, UI3 |
| PM10 | The session socket: `Takeover` and `Yielded` for SSH instead of `SIGTERM`, then the menu over SSH as a remote front end | stab, sec | M, then L | PM9 |
| PM11 | Fewer processes: `tput` and `stty` become `rustix` termios and escape sequences; the start-up chain of `exec`s shortens (B7) | perf | S/M | |

### DM: crates and the domain model

| ID | Item | Benefit | Effort | Needs |
| --- | --- | --- | --- | --- |
| DM1 | Tidy: `zbm-menu` goes, `zbm-coreutils` folds into `zbm-platform`, the PSF parser into `zbm-display`; `docs/dev/architecture.md` and `crates/DESIGN.md` agree with the code; the new metric columns | mod | S | |
| DM2 | One `Context`, and the `Interaction` port: the console leaves the domain, the `zbm-boot` → `zbm-luks` edge goes | mod | M | |
| DM3 | The value types (`KernelCommandLine`, `HostId`, `Timeout`, `LogLevel`, `RetryDelay`, questions and answers) and typed options, converted crate by crate: `zbm-pool` (its `PoolRef` goes), `zbm-boot`, `zbm-luks` | mod, sec | M | |
| DM4 | `KernelSelector`, a compiled regular expression, replaces a shell per kernel for `org.zfsbootmenu:kernel`; fuzzed | perf, sec | S/M | |
| DM5 | (merged into SB2) | | | |
| DM6 | The unlocker's core anew: the `LuksVolumes` port, the typestate unlocker, one `Result`-based registry of key methods with the token handlers in it, every attempt kept; the pilot goes | mod, stab | L | DM2, DM3 |
| DM7 | `zbm-bootenv` split out of `zbm-boot`; graphics policy to start-up, `report` to `zbm debug`, prompts to the front ends; a narrow public API with `#[non_exhaustive]` | mod | L | DM2 |
| DM8 | `zbm-app`: a required `Runtime` instead of five optional fields, one `apply`, the `Application` trait with `App` and `FakeMachine`; front ends reach the system only through it | mod | M | DM2 |
| DM9 | Errors with their causes, `Step` returning `Result`, structured `tracing` fields, a simple kernel log format, stable error codes and `zbm explain` | stab | M | |
| DM10 | `zbm-host` anew: `generate-zbm` as configuration → plan (pure) → build (adapters) → install (atomic), with argument vectors instead of `sh -c`; `zbm-kcl` on `KernelCommandLine`; the Perl and Bash emulation goes; new golden files | mod, stab | L | SB8 |
| DM11 | `zbm-core` dissolves; shell helpers in configuration parsing go to 0 | mod | S | DM3 |

### SB: the system boundary and the image

| ID | Item | Benefit | Effort | Needs |
| --- | --- | --- | --- | --- |
| SB1 | Process plumbing: one pipe instead of the `sh -c` wrapper, `setsid` in `pre_exec`, time-outs for every wait on hardware, a cap on captured output | perf, stab | S | |
| SB2 | The boot path on the `Zfs` port, batched: one listing per pool, `DatasetName` instead of `&str`; `ZfsCommand` and `ZpoolCommand` go | perf, mod | M | DM3 |
| SB3 | The `Mounts` port: `mount(2)` and `umount2(2)` with `zfsutil`, `mount.zfs` as the fallback, `Mounted` unmounting on drop | perf | M | |
| SB4 | The `KernelLoader` port: `kexec_file_load` and `reboot`, with `kexec-tools` for device trees, `ENOSYS` and hooks | sec, perf | M | |
| SB5 | Built-in hooks as native `BootStep`s, the hook API version 2, the generated `zfunc` library | mod, sec | M | DM7 |
| SB6 | The recovery tools as menu views and `zbm` applets (`zfs-chroot`, `zsnapshots`, `zkexec`, `mount_esp`, `reboot`, `firmware-setup`, `logs`, the help viewer); `fzf` leaves Rust images | mod | L | UI10 |
| SB7 | Rust images without the legacy payload (`legacy/zfsbootmenu/{lib,libexec,bin}`) and with their own list of binaries | sec, perf | S | SB5, SB6 |
| SB8 | `ImagePlan` and the `ImageBackend` trait; the dracut module and the mkinitcpio hook become shims; the mkinitcpio hook runs `zbm` without Bash | mod | M | |
| SB9 | brush from its library crates instead of `brush-shell`; measured with B1 | perf, sec | S/M | |
| SB10 | `zbm-unlock` hosts the key methods, natively where CD6 to CD10 are done and through the programs otherwise: TPM2 first, then LUKS2 (#4's errors, fewer processes), then FIDO2 (the PIN in the menu, cancellation for #2) | sec, perf | L | DM6, decision 12 |
| SB11 | YubiKey challenge-response over `hidraw` (HID feature reports, slot 1 or 2, `ykinfo`'s serial), without `ykchalresp` and `ykinfo`, which upstream no longer maintains | sec, perf | S/M | DM6 |
| SB12 | The native image backend and a minimal early init, for release and container images first, checked against dracut's output (#13 reopened in this form) | perf, sec | L | RL2 |
| SB13 | Releases publish the debug files of every binary, indexed by build ID | stab | S | |

### UI: the menu, the console and the display

| ID | Item | Benefit | Effort | Needs |
| --- | --- | --- | --- | --- |
| UI1 | The unlock dialog's key positions from the image's XKB keymaps, so `gb` and variants such as `de(nodeadkeys)` translate; the hand-written tables go | stab | S | |
| UI2 | One key type, `KeyPress`, for the menu (`Message::Key`), from the console or decoded from terminal bytes | mod | S | |
| UI3 | One front end runtime, `Frontend<B: Backend, I: Input>`, for the start-up view, the menu and the demo | mod | M | UI2 |
| UI4 | `CellRenderer`: the console's milestone M5 (C71, C72, C73, C75, C76), done once for every surface; C112 measures it | perf | M/L | |
| UI5 | The seat as typestate on the control socket (PM7); milestone M4 on it: hotplug as "choose the output again, resize every surface" (C61), every monitor (C62), rotation (C63), a chosen mode (C60), blanking (C64), pixel formats (C65), another program on the display (C66), backlight (C67), scaling (C68), double buffering (C74) | stab | M, then M4's items | PM7, UI4 |
| UI6 | The client channel, cell surfaces and the menu's backend for them: the menu draws natively on the console; C104 becomes unnecessary (the capabilities come in the handshake), C105 is done (`SwitchLayout`), C52's secret detection travels in band, C96 and C110 shrink, C89 is a reconnect | perf, sec | L | UI3, UI5 |
| UI7 | Surfaces and focus: a second terminal (C55, now M), the shown terminal and a layout for each (C56), kernel messages as a layer (C77), terminal switches (C57); M8's mouse and selection on surfaces (C48 to C51) | mod | M | UI6 |
| UI8 | Accessibility: the focused surface as text for `brltty` and `Announce` lines (C85); a line view for `TERM=dumb`, speakup and serial users (#12's plain mode) | mod | M | UI6 |
| UI9 | The serial console too (C81): one session driving the console's surface and a serial terminal from one model | mod | M | UI6 |
| UI10 | The rest of the menu (steps 5 to 7 of R5 in the old plan): VM tests of each snapshot change, the previews of a diff and of package changes, the palette's entries; kernels and the command line editor, pools and LUKS volumes (unlock and import from the menu), help, power; the exit: the key binding section of the man page generated from `keymap.rs`, the 10,000-snapshot test (decision 7), the hostile-names fuzz target, the pseudo-language at 52×20, `CHANGES-FROM-LEGACY.md` as a migration guide | mod | L | PM8 for pools and LUKS |
| UI11 | The menu design's exit (phase D of the old plan): usability sessions with at least five users, one on a serial console; the design review; decision 8 | | maintainer | |
| UI12 | The console's other items, unchanged in substance: M6's C79, C80, C82, C106; M7's C45, C46, C92 to C100 (C110 with SE3); M8's C7, C13, C15, C32, C33; M9's C101, C102, C107, C111, and C103, the default, after UI6 | | per item | |

### SE: security

| ID | Item | Benefit | Effort | Needs |
| --- | --- | --- | --- | --- |
| SE1 | The threat model gains attackers A6 (the network: SSH, tailscale) and A7 (the supply chain), the logs and reports saved on the ESP, and a diagram of zones and data flows | sec | S | |
| SE2 | `overflow-checks` in release builds; actions pinned by commit, the container's base by digest; the checks for the shell gate, test features and `zbm.console_mirror` | sec | S | |
| SE3 | The edges of secrets: the masked field renders from the `Secret`, `Secret::ct_eq`, no implicit `Clone`, the journal's allow-list, the key cache written safely and removed before `kexec` | sec | S | |
| SE4 | The strict lints (no panics) for `zbm`, `zbm-boot`, `zbm-zfs` and `zbm-state`, the crates that run as PID 1 or parse hostile input | stab, sec | M | |
| SE5 | `Untrusted<T>` and `Printable`; fuzz targets for files in `/boot` and `os-release`, hostile names through `view`, the message decoders, key input and the console's replies | sec | M | |
| SE6 | `zbm-sys` on `rustix`, with a sanitizer job | sec | M | |
| SE7 | The RustCrypto crates replace `zbm-luks/src/crypto.rs`; `argon2` for the PIN of #8 | sec | S/M | decision 1 entry |
| SE8 | Salted, encrypted TPM sessions with today's `tpm2-tools` (a salt key for `tpm2_startauthsession`, `tpm2_sessionconfig --enable-encrypt`) | sec | S | |
| SE9 | `RecoveryPolicy` and the `ShellGate` typestate, then the locked menu (#8) | sec | M | PM6 |
| SE10 | Measured boot (#10), `kexec_file_load` under lockdown failing closed, and the strict defaults after a TPM-only unlock | sec | M | SB4 |
| SE11 | The keyholder process (with SB10), the unprivileged menu and the sandboxed console: sandbox profiles as a sealed trait, `Command::sandboxed(profile)` | sec | L | PM9, UI6 |
| SE12 | The supply chain: `cargo vet`, `cargo auditable`, the SBOM, provenance, image signing in `zbm-host`, duplicate versions denied | sec | M | |
| SE13 | The independent security review against the threat model, before RL1 | sec | L | phase 5 |

### CD: C dependencies

The Rust replacements of [C dependencies](#c-dependencies) that no other item covers. Each keeps the program as the
fallback behind the same port, until its VM tests (and for CD9 and CD10 the security review) pass.

| ID | Item | Benefit | Effort | Needs |
| --- | --- | --- | --- | --- |
| CD1 | The guard: CI fails when `zbm` links a library other than glibc's (`readelf -d`); an `xtask metrics` column counts the C programs in the Rust image's list of binaries, ratcheted down; `docs/dev/dependencies.md` lists them with their item | sec, perf | S | |
| CD2 | util-linux out of the Rust runtime: `dmesg` from `/dev/kmsg`, `lsblk` from `/sys/block`, the host ID from `/etc/hostid`; `setsid` (SB1) and `mount` (SB3) as system calls | perf | S | |
| CD3 | Hashing and encoding in Rust: `openssl`, `sha256sum`, `base64`, `cut`, `tr` and `wc` leave the LUKS and TPM2 paths (`sha2`, `hmac`, `base64`) | sec, perf | S | SE7 |
| CD4 | Kernel modules without kmod: `finit_module` with `modules.dep`, `modules.alias` and `modules.softdep`, compressed modules through `MODULE_INIT_COMPRESSED_FILE`, kmod as the fallback on older kernels | perf | S/M | |
| CD5 | The kernel console's font through `KDFONTOP` with the PSF parser, without `setfont` | perf | S | DM1 |
| CD6 | GnuPG key files with `pgp` (rPGP): symmetric and public-key encrypted files, passphrase-protected secret keys from the image's keyring, the secret key wiped after use; `gpg` and `scdaemon` stay for OpenPGP cards | sec, perf | M | DM6, decision 1 entry |
| CD7 | FIDO2 hmac-secret natively: CTAP2 over `hidraw`, with `authenticator` if it covers hmac-secret and stays small, otherwise our own; the PIN from the menu; `fido2-assert` as the fallback | sec, perf | M | DM6, decision 1 entry |
| CD8 | The Clevis TPM2 pin natively: the LUKS2 token's JWE, the unseal (CD10, or `tpm2-tools` until then), the A256GCM decryption with `aes-gcm`; `clevis` stays for Tang and `sss`; `luksmeta` leaves the image | sec, perf | M | CD3, DM6 |
| CD9 | LUKS2 natively: the header and its JSON read as `Untrusted` and fuzzed, PBKDF2 and Argon2, the anti-forensic merge, the key slot decrypted (`aes`, `xts-mode`), the digest checked, the volume key locked in memory, the mapping created with `devicemapper`; `cryptsetup` for enrolment, LUKS1 and anything the Rust code does not read; differential tests against `cryptsetup` in the device suites | sec, perf, stab | L | DM6, SE5, decision 1 entry |
| CD10 | TPM2 natively: the commands the key methods use (`StartAuthSession` salted and encrypted, `PolicyPCR`, `PolicyAuthValue`, `Load`, `Unseal`, `FlushContext`, `GetCapability`, `PCR_Read`, `PCR_Extend` for #10) marshalled in Rust over `/dev/tpmrm0`, tested against `swtpm`; supersedes SE8; `tpm2-tools` stay in the recovery shell | sec, perf | L | DM6, SE8 |
| CD11 | The Clevis Tang pin natively: the advertisement's signature checked, the McCallum-Relyea exchange (`p521`, `p256`), a small HTTP client; for images that bring up the network (dracut's network modules) | sec | M/L | CD8 |
| CD12 | The EFI image without `objcopy`: `zbm-host` writes the PE sections (`.osrel`, `.cmdline`, `.linux`, `.initrd`, `.splash`) itself, checked against `objcopy`'s output | mod | M | DM10 |

### VE: verification and tooling

| ID | Item | Benefit | Effort | Needs |
| --- | --- | --- | --- | --- |
| VE1 | Coverage without test code; the baseline measured again once | stab | S | |
| VE2 | Fault injection: `FaultyRunner` and `FaultyFiles`, and the property that start-up always ends in a menu or a shell | stab | M | |
| VE3 | Contract tests of the fakes against the real adapters, in a VM | stab | M | |
| VE4 | Mutation testing on the code where a missed case costs most | stab, sec | M | |
| VE5 | The VM tiers: smoke on CI with KVM, nightly, release | stab | M | |
| VE6 | The first benchmark baseline: `cargo xtask bench` and the `bench-*` scenarios (B1 to B12) | perf | M | |
| VE7 | The upstream test VMs as `zbm-vmtest` scenarios; `scripts/` and the build container's scripts as `xtask` commands (R7 of the old plan) | mod | L | |
| VE8 | The developer guides (R8 of the old plan): the boot flow, how to add a key method, a menu view and a boot step, how to debug a boot, the ADRs, `cargo doc` published | mod | M | |
| VE9 | `xtask replay` of report bundles, `xtask doctor`, `cargo machete`, a machine-readable test map | stab | M | |

### FE: features for users

The features keep their numbers. Each needs the items named, and its full proposal (why, how, when done) is in
[the old feature list](docs/dev/history/features-roadmap.md) until it is written into the docs.

| # | Feature | State | Needs |
| --- | --- | --- | --- |
| 1 | Wipe secrets from memory | Done | |
| 2 | Unlock with whatever arrives first: one racer per key method, hot-plug through uevents | Proposed | PM4, PM8, SB10 |
| 3 | Parallel start-up: boot environments scanned and volumes unlocked in parallel, within a memory budget | Proposed | PM2, PM4, SB2 |
| 4 | Clear unlock errors: one message and hint per failure | Prototyped | DM6, SB10 |
| 5 | Reproducible images | Done | |
| 6 | Check the boot environment's ZFS before booting | Prototyped | UI10, DM7 |
| 7 | Boot counting with automatic fallback, and boot once | Prototyped | DM7, the ESP safety of SE |
| 8 | Locked menu | Proposed | SE9 |
| 9 | Saved boot log and bug-report bundle | Done | |
| 10 | Measured boot | Proposed | SE10 |
| 11 | Re-seal the TPM after firmware updates | Prototyped | SB10 |
| 12 | Built-in menu interface | In progress | UI10, UI8 |
| 15 | Reclaim space from snapshots | Proposed | UI10 |
| 16 | Pool health in detail | Proposed | UI10, SB2 |
| 17 | Graphics drivers for AMD and Intel GPUs: the tests on real hardware remain | Partly done | RL4 |
| 18 | Spanish, French, Dutch and Brazilian Portuguese | Partly done | UI10 (the language in the model) |
| 19 | Own console in Rust: step 5, the default, is C103 | In progress | UI4 to UI9, UI12 |
| 20 | Console fonts: Spleen and 256-glyph subsets | Done | |
| 21 | Restore files and see what an upgrade changed | Prototyped | UI10 |

Numbers 13 (an own init) and 14 (a built-in SSH server) were dropped; 13 comes back in a narrower form as SB12.

### RL: release

| ID | Item | Effort | Needs |
| --- | --- | --- | --- |
| RL1 | The Rust runtime becomes the default, with the legacy runtime selectable for one release (decision 6) | S | the release gates |
| RL2 | The legacy runtime goes: `legacy/`, the recordings that no longer guard anything, the compatibility shims; `CHANGES-FROM-LEGACY.md` stays as the migration guide | M | RL1 plus one release |
| RL3 | Release tooling for the Rust builds (`scripts/release/`), and release notes for distribution packagers (the Rust toolchain, vendored crates, no Perl) | M | |
| RL4 | Real hardware: UEFI x86_64 and aarch64, Secure Boot enabled, a BIOS machine through syslinux, the GPUs of #17, laptops with internal and external displays, a serial-only server | M | |

## Phases

The phases order the work; they overlap, and an item starts as soon as what it needs is done. Each phase ends with
every suite green, its exit met, and its docs written. Features (FE) go in whenever their items are done.

### Phase 0: the design and the quick fixes

The ADRs of the target architecture, and the small fixes that need no design.

- ADRs: the process model (PM1, decision 10), the trust zones (decision 13), the crate map (decision 14), the client
  channel (decision 15), C dependencies (decision 12), the image plan (SB8).
- SE1, SE2, SE3, SE7, SE8, SB1, DM1, UI1, UI2, PM5, VE1, CD1, CD2, CD3, and the
  [fixes left from the review](#fixes-left-from-the-review-of-27-october-2026).
- The VM runs of the October 2026 fixes, and of M3 (`runtime-rust-console-comfort`).

**Exit:** the ADRs accepted; the quick fixes in, with their VM runs passing.

### Phase 1: ports and types

PM2, PM3, PM4, DM2, DM3, DM4, SB2, SB3, DM9, DM11, SE4, SE5, SE6.

**Exit:** no domain crate takes a `Runner` or a console; no ZFS command line outside `zbm-zfs`; the new metric
columns at 0; the strict lints on every crate that runs at boot.

### Phase 2: the session

PM6, PM7, PM8, PM9, PM10 (the handshake), PM11, UI3, DM8.

**Exit:** one session process with the menu in it; no PID file, signal or poll between components; the menu redraws
and cancels during long work; C89 and C91 done; a takeover never kills work in progress.

### Phase 3: the menu and the screen

UI10 (the rest of the menu, built on the session), UI4, UI5 (M4), UI6, UI7, DM6, DM7.

**Exit:** the old R5 exit (the VM tests drive every core task through the menu, also on a serial console; snapshot
tests of every view at three sizes, in colour, monochrome and ASCII; the task measures and the redraw budget met; the
10,000-snapshot test; the hostile-names fuzz target; no string the user reads outside the catalogue), and the menu
drawn natively on the console.

### Phase 4: the boundary and the image

SB4, SB5, SB6, SB7, SB8, SB9, SB13, DM10, VE7, VE8, CD4, CD5, CD12.

**Exit:** Rust images without the legacy payload or `fzf`, and without kmod or `setfont`; the dracut module and the
mkinitcpio hook as shims; `zbm-host` with no Perl or Bash emulation and no `objcopy`; no Bash, Perl or Python under
`tests/`, `scripts/` or `packaging/container/`.

### Phase 5: isolation and boot integrity

SB10, SB11, SE9, SE10, SE11, UI8, UI9, CD6, CD7, CD8, CD9, CD10.

**Exit:** secrets only in `zbm-unlock`; the key methods in Rust, with their programs as the fallback; the menu and
the console unprivileged and sandboxed; the locked menu (#8) and measured boot (#10) with their VM tests.

### Phase 6: features

The features of FE whose items are done, and UI12. Continuous from phase 1 on; #2, #3, #4 and #11 come after
phase 5's SB10, #6, #7, #15, #16 and #21 after phase 3.

### Phase 7: release

VE2 to VE6, VE9, SE12, SE13, RL1, RL3, RL4; then, one release later, RL2.

**Exit:** the [release gates](#release-gates); the Rust runtime is the default; then the legacy runtime is gone.

### After the release

SB12, the native image backend and minimal early init; CD11, the Tang pin. The Rust LUKS2 and TPM2 code (CD9,
CD10) becomes the default once SE13 has reviewed it and one release has used it.

## Decisions

Decided, and kept from the old plans:

1. **Dependencies** (22 October 2026, with additions since): for the runtime, `ratatui`, `crossterm`,
   `nucleo-matcher`, `serde`, `serde_json`, `thiserror`, `tracing`, `tracing-subscriber` and `uucore`; for tests,
   `insta`, `proptest` and `pretty_assertions`; and since then `drm`, `alacritty_terminal`, `rustix`, `signal-hook`,
   `swash`, `kbvm`, `fluent-bundle`, `unic-langid`, `brush-shell`, with the VM harness's `vt100` and `regex`. Each has
   its reason in `docs/dev/dependencies.md`; a new one is an entry here and there.
2. **MSRV:** 1.88, checked in CI; it rises only when an agreed crate needs it.
3. **Menu:** the Nucleo and Ratatui menu replaces `fzf`; `fzf` leaves Rust images with the recovery tools (SB6).
4. **uutils** ships as the image's coreutils, beside `zbm`, which does not link its applets.
5. **Errors:** `thiserror`.
9. **The shell:** brush for the Rust runtime's shell work, Bash as the way back.

Decided on 27 October 2026:

11. **No libzfs** (goal G2 of the old plan): ZFS only through `zfs` and `zpool`, which `deny.toml` enforces.
17. **Parity is not a goal.** The legacy code is no longer the reference; behaviour changes are recorded in
    `CHANGES-FROM-LEGACY.md`, and the parity-only parts are gone.

Open:

6. **When the legacy runtime goes.** Proposed: one release with the Rust runtime as the default and the legacy one
   selectable, then removal (RL1, RL2); no beta in which the legacy image is the reference.
7. **Nucleo or its matcher:** decided by the 10,000-snapshot test (UI10).
8. **A classic key map:** decided by the usability sessions (UI11).
10. **One concurrency model:** threads and channels, one reactor per process, no async runtime (PM1).
12. **C dependencies:** proposed, no C library linked: the key methods in Rust in `zbm-unlock` (CD6 to CD10), the C
    programs as the fallback and in the recovery shell. Linking libcryptsetup, tss2-esys and libfido2 is the
    alternative, kept only if the Rust LUKS2 or TPM2 code fails its review (SE13).
13. **Trust zones:** the four zones of [the target architecture](#processes-and-trust-zones).
14. **The crate map** of [crates and layers](#crates-and-layers).
15. **The menu draws natively on the console** through the client channel (UI6), with a terminal only for programs
    that need one.
16. **A native image backend after the cutover** (SB12), with dracut kept as a backend.

Decisions 10 and 12 to 16 are decided by their ADRs in phase 0.

## Fixes left from the review of 27 October 2026

The review of all the code before M4 found 34 groups of bugs; 1 to 9, 30 and 31 are fixed. The rest, by their
numbers in that review, go in phase 0 unless an item above replaces them:

| # | Fix | Where |
| --- | --- | --- |
| 10 | The PC speaker sounds on after the console ends | `zbm-console/src/host/status.rs` |
| 11 | A keyboard device that also sends mouse events can freeze the programs' output | `zbm-console/src/host.rs` |
| 12 | The kernel console stays raw when the console dies while relaying | `zbm-console/src/relay.rs`; supervisor |
| 13 | A stale `console-released` record survives a dead console | goes with PM7 |
| 14 | The font keys race `zbm console font` | goes with PM7 |
| 15 | Copies of the screen in freed memory (`Console::text`, screenshot pixels) | `zbm-console` |
| 16 | Slow work in the console's loop: compression at level 9, full repaints for bells, the status line rebuilt each pass | `zbm-console`; UI4 |
| 17 | Confirmation dialogs can cut off their "Type … to confirm" line | `zbm-tui/src/menu/view/dialogs.rs` |
| 18 | Changing the sort order moves the selection to another snapshot | `zbm-tui/src/menu/update/snapshots.rs` |
| 19 | A failed boot from the command line editor leaves a blank screen; `o` does nothing in the main view; start-up dialogs' timers run fast on a noisy serial line | `zbm-tui` |
| 20 | Duplicating into a locked parent asks for no key | `zbm-app/src/changes/needs.rs` |
| 21 | Options on a hook's `#!` line are dropped | `zbm-tools/src/programs.rs`; SB5 |
| 22 | Compiled hooks are handed to the shell | `zbm-platform/src/shell.rs`; SB5 |
| 23 | The initramfs of prefixes other than `vmlinuz` changed name without a warning; `contrib/syslinux-update.sh` | `generate-zbm`; DM10 |
| 24 | `zbm-kcl` rewrites the live EFI image in place | `zbm-kcl`; DM10 |
| 25 | Versions such as `git-abc` are never pruned; firmware trimming under a symlinked build directory; `config.yaml` rewritten in place | `generate-zbm`; DM10 |
| 26 | `org.zfsbootmenu:devicetree` accepts `..` | `zbm-boot/src/commandline.rs` |
| 27 | `org.zfsbootmenu:rootprefix` can add kernel arguments | `zbm-boot`; DM3 |
| 28 | One bad pool name hides every pool; a failed read/write import leaves the pool exported; the countdown ends at once at end of input; `zbm.retry_delay=0` with a missing device spins; the wrong crash report is named | `zbm-zfs`, `zbm-boot`, `zbm-startup`, `zbm` |
| 29 | A FIFO as a key file hangs the boot; `zluks close` racing `zluks open`; `memfd_secret` without close-on-exec; mapping names that start with `-` | `zbm-luks`, `zbm-sys`, `zbm-types`; PM5 |
| 32 | A parity test reads the host's `/etc/hostid` | `zbm-boot/src/commandline/tests.rs` |
| 33 | Checks that can pass by mistake: an old screen dump, keys QEMU refused, `assert_ran!` joining arguments, the log secret check per write, an empty parity test, a fuzz target that returns at once | `zbm-vmtest`, `zbm-testkit`, `fuzz/` |
| 34 | `cargo xtask test devices` skips `zbm-crypt`'s device tests; the secret search skips `/dev` and stops at an unreadable page; two VM patterns match half a line | `xtask`, `zbm`, `zbm-vmtest` |

## Not planned

- **Bindings to `libzfs`** (decision 11).
- **Changes that break compatibility** with upstream ZFSBootMenu's command line options, configuration or properties,
  except through a deprecation with warnings for one release.
- **A built-in SSH server** (#14): remote unlock stays with dropbear and `dracut-crypt-ssh`.
- **Forks or rewrites of brush, Alacritty's terminal, `drm` or `swash`.** They are contained (privilege separation,
  sandboxing, fuzzing, `cargo vet`) and patched upstream.
- **An async runtime** (decision 10).
- **Ligatures in the console** (C14), and anything that breaks the threat model: a clipboard shared with the booted
  system, scrollback or selection while a secret prompt is open, answers that read the screen back.

## Identifiers of the old plans

Code, tests and commits cite the old plans' identifiers. Where each went:

| Old identifier | Now |
| --- | --- |
| Port phases 1 to 4, packages A and B | Done ([history](docs/dev/history/port-roadmap.md)) |
| Port phase 5 (minimal runtime), 6 (packages C to F) | Done, or in UI10 (C, D, E) and SB6 (F) |
| Port phases 7 (beta) and 8 (cutover) | RL1, RL2, RL4 |
| Refactor phases R0 to R4 | Done ([history](docs/dev/history/refactor-roadmap.md)); shadow mode on testers' machines dropped |
| D (the menu design) | Done; its exit is UI11 |
| R5 steps 1 to 4; steps 5 to 7 | Done; UI10 |
| R6 (recovery tools, built-in hooks in Rust, image assembly, debug files) | SB6, SB5, SB8, SB13 |
| R7 (upstream test VMs, scripts in Rust) | VE7 |
| R8 (developer documentation) | VE8 |
| R9 (cutover, removal, security review) | RL1, RL2, SE13 |
| Goals G1 to G11 | Principles 1 to 4 and the rules; G2 is decision 11; G6 is decision 17 |
| Proposals 1 (trustworthy fakes), 2 (offline debugging), 3 (developer experience), 4 (error codes), 5 (supply chain), 6 (deprecation policy), 7 (one concurrency model), 8 (fewer crates), 9 (API guidelines), 10 (version matrix), 11 (small steps), 12 (log levels), 13 (`xtask doctor`), 14 (static checks), 15 (upstream sync), 16 (metrics for rules 2 and 4) | VE3, VE9, VE8, DM9, SE12, principle 5, PM1, DM1, principle 3, VE5, principle of phases, DM9, VE9, VE9, kept as a practice, DM1 |
| Benchmarks B1 to B12 | [Benchmarks](#benchmarks), unchanged |
| Decisions 1 to 9 | [Decisions](#decisions), same numbers |
| Features #1 to #21 | [FE](#fe-features-for-users), same numbers; full proposals in [history](docs/dev/history/features-roadmap.md) |
| Console milestones M1 to M3 | Done ([history](docs/dev/history/console-roadmap.md)) |
| Console M4, M5, M6 to M9; items C1 to C112 | UI5, UI4, UI6 to UI9 and UI12; the items keep their numbers |
| Findings F1 to F8 of the refactoring | Done ([history](docs/dev/history/refactor-roadmap.md)) |
