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
  [the menu](#the-menu-a-workspace), [C dependencies](#c-dependencies), [the boot image](#the-boot-image),
  [security](#security-architecture),
  [observability](#observability)
- [Rules for refactored code](#rules-for-refactored-code) and [how the rules are enforced](#how-the-rules-are-enforced)
- [Verification](#verification), with [benchmarks](#benchmarks) and [release gates](#release-gates)
- [Work items](#work-items), by workstream
- [Phases](#phases): the order of the work, and [the timeline](#timeline)
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
| Console on the display | `zbm.console=drm`: Alacritty's terminal drawn through DRM, TrueType fonts, XKB layouts, scrollback wiped at secret prompts, status line, bells. Milestones M1 to M3 done, with their VM tests passing |
| Features done | #1 secrets in locked memory, #5 reproducible images, #9 saved boot logs and `zbm report`, #20 console fonts |
| Verification | About 1,670 Rust tests without root, LUKS device suites, 7 fuzz targets, about 900 VM checks (the maintainer's machine, with KVM), metrics and coverage ratchets. On 27 October 2026 every VM test passed on the Rust runtime, after fixes to brush's reading of long and continued lines and to three tests, and again on 28 October 2026 with the security hardening of phase 0 |
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

27 crates become 24 ([ADR 0003](docs/dev/adr/0003-crate-map.md), accepted on 29 October 2026). Dependencies point
down only; `cargo xtask check-deps` enforces the table, and two new rules join it: orchestration does not use
adapters, and front ends reach the system only through `zbm-app`. The second holds since DM8; the first waits for
start-up's steps over `zbm-app`, as `zbm-startup` still runs `zfs`, `udevadm`, `setfont`, hooks and shells through
`zbm-zfs` and `zbm-tools`.

| Layer | Crates | Responsibility |
| --- | --- | --- |
| unsafe | `zbm-sys` | The few system calls `rustix` lacks: `memfd_secret`, seccomp, landlock, `kexec_file_load`; everything else moves onto `rustix` (29 `unsafe` blocks to about 6) |
| types | `zbm-types` | Domain values with no I/O: names, `KernelCommandLine`, `KernelSelector`, `HostId`, `Timeout`, `LogLevel`, questions and answers, the message types' building blocks |
| foundation | `zbm-trace` | Structured events, the journal, the kernel log |
| platform | `zbm-platform`, `zbm-display` | The ports and their real implementations, one `Context` (no console in it), the reactor's building blocks; the display, fonts (the PSF parser moved here from `zbm-types`, DM1) and keyboards |
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
- **The `Zfs` port on every path.** The raw `ZfsCommand` and `ZpoolCommand` builders are private to `zbm-zfs` (SB2):
  the boot path asks through `ZfsText`, typed calls that run the legacy text commands. Next, every call goes through
  `trait Zfs`, with `ZfsCli` (JSON with `-j -p`, the `-H -p` text fallback for OpenZFS 2.2)
  and `FakeZfs`.
- **Batched.** One `zpool list -j` for all pools and one `zfs list -j -p -r -o <the properties ZFSBootMenu reads>` per
  pool build the snapshot; the boot code reads properties from it, not with one `zfs get` each.
- **Mounting without programs:** `mount(2)` for ZFS and `umount2(2)`, through the `Files` port (SB3), with `mount`
  and `mount.zfs` as the fallback; a `Mounted` unmounts when dropped.
- The text fallback goes when the oldest supported distribution ships OpenZFS 2.3.

### The system boundary

Each operation is placed in one of three layers:

| Layer | What | How |
| --- | --- | --- |
| System calls | Mounting, `kexec_file_load` and `reboot(LINUX_REBOOT_CMD_KEXEC)`, `setsid` and the controlling terminal, pipes, `sync`, dropping caches | `rustix`, or `zbm-sys` where `rustix` lacks the call; no process |
| Rust in place of C, in `zbm-unlock` only | LUKS2 key slots and dm-crypt mappings, TPM2 commands, FIDO2, YubiKey challenge-response, GnuPG key files, the Clevis TPM2 pin ([C dependencies](#c-dependencies)) | Behind the same port traits as the programs they replace, with those programs as the fallback; no C library is linked, so every process stays linked to glibc only and a missing library cannot stop the boot |
| Programs | `zfs` and `zpool`, `udevadm`, `modprobe`, `blkid`, `gpg` with `scdaemon` and `pcscd`, `clevis`, `kexec` (device trees, user hooks such as Xen), brush, uutils; `cryptsetup` and `tpm2-tools` for the recovery shell | Behind typed ports; every call has a time-out and a cap on its captured output, and runs without a shell wrapper |

- **Process plumbing.** `Runner` gains `spawn() -> Child` with kill and wait. Done in SB1 (October 2026): a default
  time-out per adapter, a cap on captured output (64 MiB, 8 MiB for a secret), and one pipe for standard output and
  error instead of `sh -c 'exec "$0" "$@" 2>&1'`, which doubled the processes of 15 call sites and made a shell a
  dependency of the Rust runtime.
- **Fakes are models, not scripts.** `FakeZfs`, `FakeLuks` (key slots, tokens) and `FakeTpm` (PCRs, lock-out) model
  behaviour; contract tests run one suite against each fake and, in a VM, against the real adapter. `FakeRunner` stays
  only for the CLI adapters' own tests, against recorded fixtures.

### The menu: a workspace

The menu becomes a workspace for people who know ZFS, without losing those who do not. Eight mockups of 27 October
2026 show it: the boot workspace, drill-down columns for kernels, the snapshot timeline, the command line, the
rollback review, pools, the start-up timeline, and the same workspace at 80×24 in ASCII. UI13 turns them into the
design pages of `docs/dev/menu-design/` and the prototype's wireframes; UI10 builds it.

- **Tabs and columns instead of separate screens.** Numbered tabs (Boot, Snapshots, Pools, Log) that `Tab` also
  cycles; in each, drill-down columns (pool › environment › kernels or snapshots) that `h` and `l` or the arrows move
  through, so a selection keeps its context. The inspector wraps long values instead of cutting them.
- **What needs attention, in view.** A sidebar names degraded pools, missing devices, foreign pools and failing checks,
  each with the key that leads to it; the boot history, from the saved logs (#9), shows which kernel last reached a
  login.
- **A command line with a dry run.** `:` opens a command line with fuzzy completion, grouped as actions, items and
  help, each with its key and what it costs. Before anything runs, it shows the plan: the steps and the commands
  each step runs (`zfs snapshot …`, `zfs rollback …`), the effect and how to undo it. Actions are plans of steps
  (PM8) whose adapters describe the command they would run (SB2, SB3), so the dry run is the plan itself.
- **Review pages for what cannot be undone.** A rollback, a forced import or a destroy opens a whole page, not an
  alert: what is lost, what is kept, the steps with their commands, the undo, a safer alternative, and the typed
  confirmation.
- **Start-up decides nothing silently, and never stops silently.** A problem gets choices with the commands they run,
  and a safe default that runs after a time-out, such as continuing without a foreign pool that no boot environment
  needs.
- **The same workspace everywhere.** At 80×24 over a serial line, in ASCII and reverse video without colour, the
  layout keeps its parts, shorter.

Keys work on every keyboard ([decision 18](#decisions)):

1. **No action needs one character only.** Arrows, `Enter`, `Esc`, `Tab` and the command line reach everything;
   `j` `k` `h` `l` move as the arrows do, and the digit row only shortens the way to a tab.
2. **No action depends on case.** One letter, one action; the risky ones open their review page.
3. **Keys by position on the display console.** There the menu gets key positions (evdev, the image's XKB keymaps):
   when a key types a character the map does not bind, as on a Cyrillic, Greek or Hebrew layout, or as AZERTY's digit
   row types `&é"'`, the key in the same place of the US layout counts. Over a serial line only characters arrive, and
   rule 1 covers it.
4. **The command line knows every language.** It matches the names of the catalogue in the menu's language and the
   English command names.

### C dependencies

Decided by [ADR 0005](docs/dev/adr/0005-c-dependencies.md), accepted on 29 October 2026. `zbm` links no C library but
glibc. The C code is in the programs the images run and in the host's tools. Each one
that a Rust implementation can replace without losing what users need is replaced, behind the same port as the
program, which stays as the fallback until the VM suites, and for the LUKS2 and TPM2 code a security review of its
own, have passed it. Differential tests run the Rust code and the program on the same volumes and devices; parsers of
on-disk or device data take `Untrusted` input and are fuzzed; each new crate is a decision 1 entry and goes through
`cargo vet`. The image's size (B1) and its process count (B7) show the gain.

| C dependency | Used for | Rust replacement | Item |
| --- | --- | --- | --- |
| glibc's calls in `zbm-sys` (`ioctl`, `signal`, `mmap`, `mlock`, `poll`, `tcsetpgrp`) | System calls without a safe wrapper | `rustix`; glibc itself stays | SE6 |
| `stty`, `tput` (ncurses) | Terminal set-up | `rustix` termios and escape sequences | PM11 |
| `setsid`, `dmesg`, `lsblk`, `mount`, `umount`, `hostid` (util-linux) | Sessions, the kernel log, block devices, mounts | System calls, `/dev/kmsg`, `/sys/class/block` with udev's records, `/etc/hostid`; done but for `mount` and `umount` (SB3), with `dmesg` and `lsblk` as the fallback until the VM suites pass | SB1, SB3, CD2 |
| `openssl`, `sha256sum`, `base64`, `cut`, `tr`, `wc` | Hashing, encoding and text in the LUKS and TPM2 paths | RustCrypto's `sha2` and `pbkdf2`, the `base64` crate; Rust images leave out the first three (October 2026), and `cut`, `tr` and `wc` stay for the recovery shell's legacy commands until SB6 | CD3 |
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

- **`ImagePlan`** ([ADR 0006](docs/dev/adr/0006-image-plan.md), accepted on 29 October 2026). `zbm-host` computes the
  image as a value: files, the binaries' paths, modules, firmware, udev rules and links, per runtime; golden files
  check it under `cargo test`. An `ImageBackend` trait installs it: `Dracut` and `Mkinitcpio`, whose `module-setup.sh` and install hook shrink to shims that read the plan as a manifest of
  tab-separated lines and keep resolving shared libraries, and later `Native`.
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
- **Audited crypto.** The hand-written SHA-256, HMAC, PBKDF2 and base64 of `zbm-luks` gave way to the RustCrypto
  crates with `zeroize` (SE7); `argon2` comes with the PIN of #8 (SE9), which is the first to need it.
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
| Size (3), types over strings (4), errors (5) | `xtask metrics` | Counts long functions and files, success flags, legacy-named doc lines, shell helpers, shell starts, tests that run Bash, `unsafe` blocks, ZFS command lines outside `zbm-zfs`, string parameters of domain functions and format-string log macros per crate, against `xtask/metrics-baseline.txt`; a count may only go down |
| Formatting and widths (1) | `cargo fmt --check`, `xtask lint` | `xtask lint` runs `rustfmt` and the width check |
| Dependencies (9) | `cargo deny check` | Licences, advisories, duplicate versions and banned crates (`libzfs` bindings among them, for decision 11) |
| Strict lints stay clean once reached | `xtask lint` with `xtask/strict/crates.txt` | The crates listed there must have no strict finding; each phase adds the crates it finishes |
| Test coverage only goes up | `xtask coverage` with `cargo llvm-cov` | `--check` fails when a crate's line coverage drops below `xtask/coverage-baseline.txt`; a new crate must reach 80% |
| Documentation matches behaviour | `xtask gen-docs --check` | Fails when a man page fragment generated from the option or key binding tables is out of date |

New checks, added by the items that need them:

| Rule | Tool | Item |
| --- | --- | --- |
| Domain crates reach programs only through adapter ports | `xtask check-deps` refuses `zbm_platform::Runner` in domain crates, and front ends that use domain crates other than through `zbm-app` | DM2, DM8 |
| No raw ZFS command lines outside `zbm-zfs` | `xtask metrics` column `zfs_commands` (DM1, 25 on 28 October 2026), ratcheted to 0 | SB2 |
| Names are newtypes in domain signatures | `xtask metrics` column `string_params`: public domain functions taking a raw string (DM1, 99; 73 after DM3, 41 of them in `zbm-luks` for DM6) | DM3, DM6 |
| Structured logs | `xtask metrics` column `format_logs`: format-string log macros (DM1, 262), ratcheted to 0 (DM9, done) | DM9 |
| No interactive shell outside its gate | `xtask metrics` column `shell_starts` (SE2), ratcheted down to `ShellGate` alone | SE9 |
| `zbm` links glibc alone; the Rust image's C programs go down | `cargo xtask check-links` in CI (`readelf -d`); `xtask metrics` row `rust-image`, column `c_programs` (45 on 28 October 2026) (CD1, done) | CD1 |
| Release images carry no test features | `xtask lint` step "test features": no feature but a test one turns one on, and no file that builds releases sets features (SE2, done) | SE2 |
| Arithmetic overflow is caught | `overflow-checks = true` in the release profile (SE2, done) | SE2 |
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
| PM1 | Done: [ADR 0001](docs/dev/adr/0001-process-model.md), accepted on 28 October 2026: the process model and concurrency (decision 10): one reactor per process, workers own nothing global, typed messages, no async runtime | mod, stab | S | |
| PM2 | Done: the ports of the system and their fakes are `Send + Sync` (`Mutex` instead of `RefCell`; `zbm_testkit::Shared` for what a test shares with a responder); `zbm_platform::event::spawn_traced` carries the kernel log and the current span into workers, and clippy refuses `std::thread::spawn` elsewhere. Left to PM8: `Interaction`, which the menu answers on its thread | scal | M | PM1 |
| PM3 | Done: one signal facility: `signalfd` and `pidfd` over `rustix`, a typed `SignalSet`; the global "last signal" slot and `signal-hook` go | stab | S | |
| PM4 | Done: the reactor's parts in `zbm_platform::event`: `Mailbox` (an `eventfd` and a channel of 256), `Reactor` (one `poll` over sources and deadlines), `CancelToken`, `WorkPool`, `MemoryBudget`; a command's time-out and cancel token are enforced by the thread that runs it, in a `poll` with its output, its `pidfd` and the token, with no watchdog thread, and a child left holding the output pipe no longer keeps the runner reading. Nothing uses the reactor yet: PM8 and PM9 do; the supervisor keeps a `poll` of its own (PM6) | scal, perf | M | PM2, PM3 |
| PM5 | Done: `flock` locks for the state directory (`luks-mappings`, `active`, the key cache) as RAII `StateLock`s: no lock taken anyway after 5 s, no stale PID | stab | S | |
| PM6 | Done: the supervisor is one event loop (`supervisor/running.rs`): a `poll` over the `pidfd`s of its children, the hang-up of the console's terminal and, as process 1, a `signalfd` for `SIGCHLD`, which tells of orphans; each wake collects every child that ended (`waitpid(-1, WNOHANG)`), and every wait is a deadline in the time-out. Its two services keep their own states: the start-up program and the recovery shell (fallback, retry, `--resume`, as before), and the console on the display, which now starts again after 1, 2 and 4 s (`Backoff`; a minute's run starts the count again), only between programs, and is then given up. It keeps its own `poll` rather than PM4's `Reactor`, which reads the time through the `Clock` port and wakes on input, as the console's terminal must not. `setsid` and the controlling terminal in `pre_exec` (C89) came with SB1b | stab | M | PM3 |
| PM7 | Done: the console's control socket (C91), `/run/zfsbootmenu/console.sock`, root peers only: `release`, `reattach`, `layout`, `font` and `status` are requests with replies once the console's loop did them (`zbm_app::console`, `zbm_console::control`), and the menu's secret prompt is a `secret` request held open while it shows; `SIGUSR1`, `SIGUSR2`, the layout and font request files and the secret prompt's record are gone. The flag moves into the cell stream with UI6 | stab | S/M | |
| PM8 | Done: the menu's machine is a service (`zbm_app::service`): a worker owns `App`, the actions that leave the console alone run on it while the menu draws and counts, and Escape cancels them, the program waited for killed through a `CancellingRunner`; start-up's stages run on a worker, and each question to the person is a request with a one-shot reply (`zbm_platform::event::reply`), the view serving them from its mailbox; the questions and answers that borrow nothing are types of `zbm_types::interaction`; `Interaction`, `Operator` and `Recovery` are `Send + Sync` with `&self` receivers (the last two fold into the first with PM9's one front end); a change is a plan of steps (`App::plan`, `Action::Plan`): the change run against `zbm_zfs::DryRun`, which reads the machine and records each `ZfsChange` with the command `ZfsCli` would run, for the menu's dry run (UI14) | perf, stab | L | PM4, DM2 |
| PM9 | Done: one session: on a console it can draw on, the start-up program runs the menu in its own process (`InSession`, `zbm_startup::program::menu::SessionMenu`), on the screen and the model of the start-up view (`zbm_tui::menu::screen::SessionScreen`), each round of the recovery loop going on with them; the session blocks the takeover signals before any thread starts; the `--countdown` child goes, and so does the second countdown after a menu that could not start: on a console the session cannot draw on, start-up counts down itself before the menu program, so there is one countdown either way | perf, stab | M/L | PM8, UI3 |
| PM10 | The handshake is done: the session listens on `/run/zfsbootmenu/session.sock` (`zbm_app::session` on `zbm_sys::socket`: `SOCK_SEQPACKET`, root peers only by `SO_PEERCRED`, versioned JSON, fuzzed), and a `zfsbootmenu` over SSH asks it to end its menu (`Takeover`), taking the lock once it says `Yielded`; signals stay for a menu the session does not hold or that is `busy`. Left: the menu over SSH as a remote front end | stab, sec | M, then L | PM9 |
| PM11 | Done: the Rust runtime starts no `tput` and no `stty`: the console port changes the screen with ECMA-48 sequences (`Screen`: clear, show and hide the cursor, move it) and the terminal's settings, size and a serial terminal's size with `termios` (`rustix`); the console's size after `setfont` is the Files port's `terminal_size`. The start-up chain stays: `zfsbootmenu-preinit`'s shell sources `/etc/profile`, which the image's build steps write, and an `exec` starts no process, so B7 does not count it | perf | S/M | |

### DM: crates and the domain model

| ID | Item | Benefit | Effort | Needs |
| --- | --- | --- | --- | --- |
| DM1 | Tidy: `zbm-menu` goes, `zbm-coreutils` folds into `zbm-platform`, the PSF parser into `zbm-display`; `docs/dev/architecture.md` and `crates/DESIGN.md` agree with the code; the new metric columns | mod | S | |
| DM2 | Done: one `Context` (the `System` without its console, its runner traced), and the `Interaction` port without console closures: the plain console is `zbm-tools`'s `ConsoleInteraction`, `zbm-luks` and `zbm-pool` take a `Context`, which `check-deps` enforces in domain crates; the `zbm-boot` → `zbm-luks` edge is gone. Left: `zbm-boot`'s prompts (DM7), `Recovery` and `Operator` into `Interaction` (PM8) | mod | M | |
| DM3 | Done: the value types (`KernelArg` and `KernelCommandLine`, `HostId`, `Timeout`, `LogLevel`, `RetryDelay`, `FilesystemName`) and typed options, converted crate by crate: `zbm-pool` (its `PoolRef` goes), `zbm-boot`, `zbm-luks`'s entry points; bug 27 fixed. Left to the items that rebuild the code: the key methods' names (DM6), the command line pipeline (DM11), questions and answers (PM8) | mod, sec | M | |
| DM4 | Done: `KernelSelector`, a compiled regular expression, replaces a shell per kernel for `org.zfsbootmenu:kernel`; fuzzed | perf, sec | S/M | |
| DM5 | (merged into SB2) | | | |
| DM6 | The unlocker's core anew: the `LuksVolumes` port, the typestate unlocker, one `Result`-based registry of key methods with the token handlers in it, every attempt kept, on `MappingName` and `DevicePath` inside as at its entry points (DM3); the pilot goes | mod, stab | L | DM2, DM3 |
| DM7 | `zbm-bootenv` split out of `zbm-boot`; graphics policy to start-up, `report` to `zbm debug`, prompts to the front ends (the resume question, the boot message and key loading, so that `zbm-boot` takes a `Context` and leaves `check-deps`'s console exception, `xtask/src/deps/console.rs`); a narrow public API with `#[non_exhaustive]` | mod | L | DM2 |
| DM8 | Done: `zbm-app`'s `App` always has a `Runtime` (the options, the state directory, the layouts, how to boot) instead of five optional fields, and a LUKS session when the front end has one; one `apply(action, Through)` replaces `act`, `act_quietly` and `act_on_console`; the menu's machine reaches it through the `Application` trait (`snapshot`, `read_kernels`, `apply`); the volumes' actions alone are `Volumes`', and the pilot's `pilot`. `check-deps` refuses a crate behind `zbm-app` (adapters, domain, orchestration) as a dependency of `zbm-tui` or `zbm-console`. The tests of `App` run on `FakeSystem`, as before | mod | M | DM2 |
| DM9 | Done: every log message a constant template with its values as fields (`fmt logs` at 0), the journal keeping the template and the fields that name things; stable error codes (`ZBM-E0412`) on every error enum of the domain, the adapters and start-up through `zbm_types::error_codes!`, which matches every variant, and `zbm explain`; causes kept as `#[source]` and shown with `zbm_types::code::chain` (`SaveError` keeps its `io::Error`); `Step::run` returns `Result<StepOutcome, StartupError>`, a failed stage logged with its code and shown failed (the `zbm.hookroot` import's error, dropped before, and a ZFS module that does not load); `zbm` returns `ExitCode`; one meaning per log level (`crates/DESIGN.md`). Left: the legacy unlock path's attempts and `LuksError::Open`'s cause go with the unlocker's rewrite (DM6); the kernel log keeps the legacy format, which the parity tests and the recovery shell's error printer read | stab | M | |
| DM10 | `zbm-host` anew: `generate-zbm` as configuration → plan (pure) → build (adapters) → install (atomic), with argument vectors instead of `sh -c`; `zbm-kcl` on `KernelCommandLine`; the Perl and Bash emulation goes; new golden files | mod, stab | L | SB8 |
| DM11 | Done: `zbm-core` dissolves (its parts into `zbm-platform`, `getopts` into `zbm-kcl`); shell helpers in configuration parsing at 0; the command line pipeline works on `KernelCommandLine`; `zbm-kcl` keeps the legacy tokenizer until DM10 | mod | S | DM3 |

### SB: the system boundary and the image

| ID | Item | Benefit | Effort | Needs |
| --- | --- | --- | --- | --- |
| SB1 | Process plumbing: one pipe instead of the `sh -c` wrapper, `setsid` in `pre_exec`, time-outs for every wait on hardware, a cap on captured output | perf, stab | S | |
| SB2 | Done, first half: no ZFS command line outside `zbm-zfs` (25 before); the boot path, the start-up and `zbm-kcl` ask through `ZfsText`, typed (`DatasetName`, `PoolName`, `FilesystemName`) with the legacy command lines unchanged, and `Import` moved into `zbm-zfs`; `ZfsCommand` and `ZpoolCommand` private to it. Left: the boot path on `trait Zfs`, batched (one listing per pool, properties read from it), which changes the command lines and the legacy golden files | perf, mod | M | DM3 |
| SB3 | Done: `zbm-tools`'s `Mount` mounts ZFS filesystems with `mount(2)` and unmounts with `umount2(2)`, through the `Files` port (`mount_zfs`, `unmount`, which only `RealFiles` makes, so tests never mount), with the programs as the fallback; a `Mounted` unmounts when dropped (boot environments read for kernels, key sources, hook filesystems, the EFI partition for saved logs); the mount command builders of `zbm-zfs` go. Left: block devices, FAT and `efivarfs` still by `mount`, which needs the file system type; `umount -R` | perf | M | |
| SB4 | The `KernelLoader` port: `kexec_file_load` and `reboot`, with `kexec-tools` for device trees, `ENOSYS` and hooks | sec, perf | M | |
| SB5 | Built-in hooks as native `BootStep`s, the hook API version 2, the generated `zfunc` library | mod, sec | M | DM7 |
| SB6 | The recovery tools as menu views and `zbm` applets (`zfs-chroot`, `zsnapshots`, `zkexec`, `mount_esp`, `reboot`, `firmware-setup`, `logs`, the help viewer); `fzf` leaves Rust images | mod | L | UI10 |
| SB7 | Rust images without the legacy payload (`legacy/zfsbootmenu/{lib,libexec,bin}`) and with their own list of binaries | sec, perf | S | SB5, SB6 |
| SB8 | Decided by [ADR 0006](docs/dev/adr/0006-image-plan.md), accepted: `ImagePlan` and the `ImageBackend` trait; the dracut module and the mkinitcpio hook become shims; the mkinitcpio hook runs `zbm` without Bash | mod | M | |
| SB9 | brush from its library crates instead of `brush-shell`; measured with B1 | perf, sec | S/M | |
| SB10 | `zbm-unlock` hosts the key methods, natively where CD6 to CD10 are done and through the programs otherwise: TPM2 first, then LUKS2 (#4's errors, fewer processes), then FIDO2 (the PIN in the menu, cancellation for #2) | sec, perf | L | DM6, decision 12 |
| SB11 | YubiKey challenge-response over `hidraw` (HID feature reports, slot 1 or 2, `ykinfo`'s serial), without `ykchalresp` and `ykinfo`, which upstream no longer maintains | sec, perf | S/M | DM6 |
| SB12 | The native image backend and a minimal early init, for release and container images first, checked against dracut's output (#13 reopened in this form) | perf, sec | L | RL2 |
| SB13 | Releases publish the debug files of every binary, indexed by build ID | stab | S | |

### UI: the menu, the console and the display

| ID | Item | Benefit | Effort | Needs |
| --- | --- | --- | --- | --- |
| UI1 | Done: the unlock dialog's key positions from the image's XKB keymaps, so `gb` and variants such as `de(nodeadkeys)` translate; the hand-written tables go | stab | S | |
| UI2 | Done: one key type, `KeyPress`, for the menu (`Message::Key`), from the console or decoded from terminal bytes | mod | S | |
| UI3 | Done: one front end runtime, `zbm_tui::menu::frontend::Frontend<B: Surface, I: Input>`: it draws the model, follows the size, reads keys (woken by the machine's mailbox), counts time on a `Clock`, records secret prompts for the console on the display, logs the keys of the usability sessions, lends the terminal and keeps the last frame; `Frontend::run` is the menu's loop on a terminal (`terminal::run`), on the display's pixels (`display::run`, for any machine) and on Ratatui's test backend, and the start-up view is a model on a `Frontend` | mod | M | UI2 |
| UI4 | `CellRenderer`: the console's milestone M5 (C71, C72, C73, C75, C76), done once for every surface; C112 measures it | perf | M/L | |
| UI5 | The seat as typestate on the control socket (PM7); milestone M4 on it: hotplug as "choose the output again, resize every surface" (C61), every monitor (C62), rotation (C63), a chosen mode (C60), blanking (C64), pixel formats (C65), another program on the display (C66), backlight (C67), scaling (C68), double buffering (C74) | stab | M, then M4's items | PM7, UI4 |
| UI6 | The client channel, cell surfaces and the menu's backend for them: the menu draws natively on the console; C104 becomes unnecessary (the capabilities come in the handshake), C105 is done (`SwitchLayout`), C52's secret detection travels in band, C96 and C110 shrink, C89 is a reconnect | perf, sec | L | UI3, UI5 |
| UI7 | Surfaces and focus: a second terminal (C55, now M), the shown terminal and a layout for each (C56), kernel messages as a layer (C77), terminal switches (C57); M8's mouse and selection on surfaces (C48 to C51) | mod | M | UI6 |
| UI8 | Accessibility: the focused surface as text for `brltty` and `Announce` lines (C85); a line view for `TERM=dumb`, speakup and serial users (#12's plain mode) | mod | M | UI6 |
| UI9 | The serial console too (C81): one session driving the console's surface and a serial terminal from one model | mod | M | UI6 |
| UI10 | The rest of the menu, built as the workspace of UI13 (steps 5 to 7 of R5 in the old plan): tabs and drill-down columns, the attention sidebar, the boot history (#9), review pages for rollback, forced import and destroy; VM tests of each snapshot change, the previews of a diff and of package changes, the palette's entries; kernels and the command line editor, pools and LUKS volumes (unlock and import from the menu), help, power; the exit: the key binding section of the man page generated from `keymap.rs`, the 10,000-snapshot test (decision 7), the hostile-names fuzz target, the pseudo-language at 52×20, `CHANGES-FROM-LEGACY.md` as a migration guide | mod | L | PM8 for pools and LUKS |
| UI11 | The menu design's exit (phase D of the old plan): usability sessions with at least five users, one on a serial console and one with a non-US layout, on the workspace prototype of UI13; the design review; decisions 8 and 18 | | maintainer | |
| UI12 | The console's other items, unchanged in substance: M6's C79, C80, C82, C106; M7's C45, C46, C92 to C100 (C110 with SE3); M8's C7, C13, C15, C32, C33; M9's C101, C102, C107, C111, and C103, the default, after UI6 | | per item | |
| UI13 | The workspace design: the ADR for it and its key map (decisions 8 and 18), `docs/dev/menu-design/` rewritten from the eight mockups, new task measures, and the main and serial screens in `zbm-tui --demo` for the sessions of UI11 | mod | S/M | |
| UI14 | The command line: completion grouped by kind with keys and costs, history, and the dry run of a plan's steps and commands, translated through the catalogue | mod, sec | M | PM8, UI13 |
| UI15 | Keys on every keyboard: the four rules of [the menu](#the-menu-a-workspace), with the snapshot and VM tests typing on `us`, `de`, `fr`, `uk`, `ru` and Dvorak | stab | S/M | UI2, UI13 |

### SE: security

| ID | Item | Benefit | Effort | Needs |
| --- | --- | --- | --- | --- |
| SE1 | The threat model gains attackers A6 (the network: SSH, tailscale) and A7 (the supply chain), the logs and reports saved on the ESP, and a diagram of zones and data flows | sec | S | |
| SE2 | Done: `overflow-checks` in release builds; actions pinned by commit, the container's base by digest; the checks for the shell gate, test features and `zbm.console_mirror` | sec | S | |
| SE3 | Done: The edges of secrets: the masked field renders from the `Secret`, `Secret::ct_eq`, no implicit `Clone`, the journal's allow-list, the key cache written safely and removed before `kexec` | sec | S | |
| SE4 | Done: the strict lints (no panics) for `zbm`, `zbm-boot`, `zbm-zfs` and `zbm-state`, the crates that run as PID 1 or parse hostile input | stab, sec | M | |
| SE5 | Done: `zbm_types::Untrusted<T>` (no `Display`) and `Printable` (C0, C1, DEL and the bidi controls escaped, with a property test); the menu's text from pools, `/boot`, crypttab and programs is `Untrusted` in `zbm-app`'s snapshot and drawn as `Printable`, as are device labels in prompts, kernel paths in boot messages, the errors the recovery shell reads back, and `zbm debug`'s and `zluks status`'s output (threat model P13); five fuzz targets: `boot_files`, `menu_names`, `key_input`, `console_replies`, `messages`. Found and fixed: the console's replies (`OSC`) typed into the menu; Enter lost or doubled at the edges of broken terminal input | sec | M | |
| SE6 | Done: `zbm-sys` on `rustix`: `poll`, `waitpid`, the process group and terminal calls, `setsid`, `prctl`, `mlockall`, the page size and `ftruncate` as safe calls, the memory mappings and the terminal and input `ioctl`s through `rustix`'s typed calls with opcodes built for the target (the hand-encoded ones were wrong on PowerPC, MIPS and SPARC); 8 C declarations left of 24 (signal dispositions, the signal mask, `signalfd`, `memfd_secret`, `explicit_bzero`, the unwinder); descriptors borrowed (`AsFd`, `BorrowedFd`) instead of raw numbers in its API; on the strict lints; its tests and `zbm-platform`'s under AddressSanitizer with leak detection in CI (`sanitizer` job, nightly) | sec | M | |
| SE7 | The RustCrypto crates replace `zbm-luks/src/crypto.rs`; `argon2` for the PIN of #8, with SE9 | sec | S/M | decision 1 entry |
| SE8 | Done: Salted, encrypted TPM sessions with today's `tpm2-tools` (a salt key for `tpm2_startauthsession`, `tpm2_sessionconfig --enable-encrypt`) | sec | S | |
| SE9 | `RecoveryPolicy` and the `ShellGate` typestate, then the locked menu (#8), its PIN hashed with `argon2` (from SE7) | sec | M | PM6, decision 1 entry |
| SE10 | Measured boot (#10), `kexec_file_load` under lockdown failing closed, and the strict defaults after a TPM-only unlock | sec | M | SB4 |
| SE11 | The keyholder process (with SB10), the unprivileged menu and the sandboxed console: sandbox profiles as a sealed trait, `Command::sandboxed(profile)` | sec | L | PM9, UI6 |
| SE12 | The supply chain: `cargo vet`, `cargo auditable`, the SBOM, provenance, image signing in `zbm-host`, duplicate versions denied | sec | M | |
| SE13 | The independent security review against the threat model, before RL1 | sec | L | phase 5 |

### CD: C dependencies

The Rust replacements of [C dependencies](#c-dependencies) that no other item covers. Each keeps the program as the
fallback behind the same port, until its VM tests pass. CD9 and CD10 come after the release (see
[the timeline](#timeline)) and become the default only after a security review of their own, like SE13.

| ID | Item | Benefit | Effort | Needs |
| --- | --- | --- | --- | --- |
| CD1 | The guard: CI fails when `zbm` links a library other than glibc's (`readelf -d`); an `xtask metrics` column counts the C programs in the Rust image's list of binaries, ratcheted down; `docs/dev/dependencies.md` lists them with their item | sec, perf | S | |
| CD2 | util-linux out of the Rust runtime: `dmesg` from `/dev/kmsg`, `lsblk` from `/sys/block`, the host ID from `/etc/hostid`; `setsid` (SB1) and `mount` (SB3) as system calls | perf | S | |
| CD3 | Hashing and encoding in Rust: `openssl`, `sha256sum`, `base64`, `cut`, `tr` and `wc` leave the LUKS and TPM2 paths (`sha2`, `pbkdf2`, `base64`); Rust images leave out the first three, and the text tools stay for the shell's legacy commands (`firmware-setup` runs `tr`) | sec, perf | S | SE7 |
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
| VE5 | The VM tiers: smoke on CI with KVM, nightly, release; in phase 1, so that VM runs stop waiting on one machine | stab | M | |
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
| 22 | Compare a kernel with the default: modules, firmware and initramfs, before booting it | Proposed | UI10 |
| 23 | Snapshot space over time: what each month of snapshots holds, in the timeline | Proposed | UI10, SB2 |

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
  channel (decision 15), C dependencies (decision 12), the image plan (SB8), the menu workspace and its keys (UI13,
  decisions 8 and 18). Drafted for review on 28 October 2026 in [docs/dev/adr/](docs/dev/adr/), but for UI13's; the
  process model, the trust zones and the client channel accepted the same day, the crate map, C dependencies and the
  image plan on 29 October.
- UI13. SE2, SE3, SE8, PM5, SE7 and CD3 are done, with their VM runs passing (28 October 2026, at `ec817dde`); SB1
  and CD2 too (28 October 2026, at `a054ad23`, all four lanes); SE1, DM1, CD1, VE1, UI1 and UI2 too (28 October 2026,
  at `f8374569`, all four lanes); phase 1's DM4, PM3, SE4 and DM3 too (28 October 2026, at `5f2d61b0`, all four
  lanes); DM11, DM2, SB2 and SB3 too (28 October 2026, at `1e5a5f6b`, all four lanes); PM2, PM4 and DM9 too (28
  October 2026, at `a2fd0246`, all four lanes). The VM run of SE6, SE5, PM6, PM8, UI3, PM9, PM10's handshake, PM7 and PM11 is left.
- The [fixes left from the review](#fixes-left-from-the-review-of-27-october-2026): in on 27 October 2026, but for
  those that go with later items, with their VM runs passing (all but the two `oldzfs` tests, which did not run).
- The VM runs of the October 2026 fixes, and of M3 (`runtime-rust-console-comfort`): done on 27 October 2026, every
  test passing on the Rust runtime.

**Exit:** the ADRs accepted; the quick fixes in, with their VM runs passing; the workspace prototype ready for the
sessions.

### Phase 1: ports and types

PM2, PM3, PM4, DM2, DM3, DM4, SB2, SB3, DM9, DM11, SE4, SE5, SE6, VE5; the usability sessions of UI11 on the
workspace prototype, in parallel.

**Exit:** the smoke tier of the VM suite runs on CI with KVM; no domain crate takes a `Runner` or a console; no ZFS
command line outside `zbm-zfs`; the new metric columns at 0; the strict lints on every crate that runs at boot.

### Phase 2: the session

PM6, PM7, PM8, PM9, PM10 (the handshake), PM11, UI3, DM8.

**Exit:** one session process with the menu in it; no PID file, signal or poll between components; the menu redraws
and cancels during long work; C89 and C91 done; a takeover never kills work in progress.

### Phase 3: the menu and the screen

UI10 (the rest of the menu, built on the session as the workspace), UI14, UI15, UI4, UI5 (M4), UI6, UI7, DM6, DM7.

**Exit:** the old R5 exit (the VM tests drive every core task through the menu, also on a serial console; snapshot
tests of every view at three sizes, in colour, monochrome and ASCII; the task measures and the redraw budget met; the
10,000-snapshot test; the hostile-names fuzz target; no string the user reads outside the catalogue), the menu
drawn natively on the console, every risky action with its dry run and review page, and the key tests passing on the
six layouts of UI15.

### Phase 4: the boundary and the image

SB4, SB5, SB6, SB7, SB8, SB9, SB13, DM10, VE7, VE8, CD4, CD5, CD12.

**Exit:** Rust images without the legacy payload or `fzf`, and without kmod or `setfont`; the dracut module and the
mkinitcpio hook as shims; `zbm-host` with no Perl or Bash emulation and no `objcopy`; no Bash, Perl or Python under
`tests/`, `scripts/` or `packaging/container/`.

### Phase 5: isolation and boot integrity

SB10, SB11, SE9, SE10, SE11, UI8, UI9, CD6, CD7, CD8.

**Exit:** secrets only in `zbm-unlock`; the key methods in Rust but LUKS2 and TPM2, which stay with `cryptsetup` and
`tpm2-tools`, and the programs as the fallback; the menu and
the console unprivileged and sandboxed; the locked menu (#8) and measured boot (#10) with their VM tests.

### Phase 6: features

The features of FE whose items are done, and UI12. Continuous from phase 1 on; #2, #3, #4 and #11 come after
phase 5's SB10, #6, #7, #15, #16, #21, #22 and #23 after phase 3.

### Phase 7: release

VE2, VE3, VE4, VE6, VE9, SE12, SE13, RL1, RL3, RL4; then, one release later, RL2.

**Exit:** the [release gates](#release-gates); the Rust runtime is the default; then the legacy runtime is gone.

### After the release

CD9 and CD10, LUKS2 and TPM2 in Rust, moved here to shorten the critical path; they become the default after a
security review of their own and one release behind a setting. SB12, the native image backend and minimal early
init; CD11, the Tang pin.

## Timeline

An estimate made on 27 October 2026, to be revised at the end of each phase.

### Effort

The [work items](#work-items) give their effort for one developer who knows the code. Counted as S = 0.6 weeks,
S/M = 1.5, M = 3, M/L = 6 and L = 10, with 3 weeks for the review fixes and 2 for the ADRs:

| Phase | What weighs most | Developer-weeks |
| --- | --- | --- |
| 0 | The ADRs, 14 small items (UI13 among them), the review fixes | ~16 |
| 1 | Ten M items: ports, value types, the ZFS port, mounts, the strict lints, fuzzing, the VM tiers (VE5) | ~36 |
| 2 | PM8 (L), PM9 (M/L), the supervisor, the sockets | ~31 |
| 3 | UI10 as the workspace, UI6, DM6, DM7 (L each), UI4, UI5, UI14, UI15 | ~67 |
| 4 | SB6, DM10, VE7 (L each), `kexec_file_load`, the hooks, the image plan, CD4, CD5, CD12 | ~50 |
| 5 | SB10, SE11 (L each), CD6 to CD8, the locked menu, measured boot | ~43 |
| 6 | About twelve features (#22 and #23 among them), and the console's other items (UI12) | ~41 |
| 7 | Fault injection, mutation tests, benchmarks, the security review (SE13), real hardware | ~35 |
| **To RL1** | | **~319**: about six years for one developer, eight with review and rework |
| After the release | RL2, CD9, CD10 and their review, SB12, CD11 | ~40 |

The critical path, the chain that cannot run in parallel, is the ADRs → DM2 and DM3 → PM2 and PM4 → PM8 → PM9 →
UI3 and UI5 → UI6 → DM6 → SB10 → SE11 → SE13 → RL1: about 80 developer-weeks, so even a team needs well over a
year. The workspace (UI10's growth, UI13 to UI15, #22 and #23) is off that path; it adds about 15 weeks of work.

### Calendar

The code is written with an AI assistant, which takes away most of the typing. What sets the pace is what only
people can do: the maintainer's review and VM runs (assumed about 10 hours a week), the hardware of RL4, the usability
sessions of UI11, the independent review of SE13 (six to eight weeks of the reviewer's calendar) and one release
between RL1 and RL2. Phases 4 and 6 overlap the others.

| Phase | Weeks | Ends about |
| --- | --- | --- |
| 0: the design and the quick fixes | 4 | late November 2026 |
| 1: ports and types | 7 | late January 2027 |
| 2: the session | 6 | mid-March 2027 |
| 3: the menu and the screen | 12 | June 2027 |
| 4: the boundary and the image | 8, overlapping 3 and 5 | July 2027 |
| 5: isolation and boot integrity | 7 | August 2027 |
| 6: features | alongside 1 to 5 | |
| 7: release | 8 to 10 | **October to November 2027: RL1** |
| RL2 | one release later | early to mid 2028 |
| CD9, CD10 (default after their review), SB12, CD11 | after RL2 | 2028 |

Two changes shortened the path: VE5 moved from phase 7 to phase 1, so VM runs no longer wait on one machine, and
CD9 and CD10, about 20 weeks of security-critical code, moved after the release, where `cryptsetup` and `tpm2-tools`
keep doing their work until then. The workspace moves RL1 by about a month: a week more in phase 0 for its design
and prototype, and two to three more in phase 3, which builds it once instead of building the old design first. Its
extras (#22, #23) wait for phase 6 and move nothing.

### Risks to the dates

- **VM runs:** until VE5 is done, every change to the images waits on the maintainer's machine.
- **The design risks,** UI6 (the client channel) and PM8 (the machine actor): their ADRs in phase 0 settle them, and
  a wrong turn there costs a phase.
- **The workspace in the sessions:** if UI11 rejects it, phase 3 builds the current design, as planned before; the
  prototype in phase 0 is there to learn that before any of it is built.
- **Outside calendars:** the security review, the usability sessions, and the hardware of RL4.
- **The estimate itself:** the history in git was squashed, so there is no measured pace yet. Phase 0's actual length
  is the first measure; the calendar is revised from it.

## Decisions

Decided, and kept from the old plans:

1. **Dependencies** (22 October 2026, with additions since): for the runtime, `ratatui`, `crossterm`,
   `nucleo-matcher`, `serde`, `serde_json`, `thiserror`, `tracing`, `tracing-subscriber` and `uucore`; for tests,
   `insta`, `proptest` and `pretty_assertions`; and since then `drm`, `alacritty_terminal`, `rustix`, `signal-hook`,
   `swash`, `kbvm`, `fluent-bundle`, `unic-langid`, `brush-shell`, with the VM harness's `vt100` and `regex`, and
   RustCrypto's `sha2` and `pbkdf2` with the `base64` crate for the key methods (SE7, 28 October 2026). Each has its
   reason in `docs/dev/dependencies.md`; a new one is an entry here and there.
2. **MSRV:** 1.88, checked in CI; it rises only when an agreed crate needs it.
3. **Menu:** the Nucleo and Ratatui menu replaces `fzf`; `fzf` leaves Rust images with the recovery tools (SB6).
4. **uutils** ships as the image's coreutils, beside `zbm`, which does not link its applets.
5. **Errors:** `thiserror`.
9. **The shell:** brush for the Rust runtime's shell work, Bash as the way back.

Decided on 29 October 2026:

12. **C dependencies** ([ADR 0005](docs/dev/adr/0005-c-dependencies.md)): no C library linked, `zbm` stays linked to
    glibc only; programs replaced by Rust behind the same port where that keeps what users need, the program as the
    fallback until the VM suites pass the Rust code, and never after a secret was tried; the key methods in Rust in
    `zbm-unlock` (CD6 to CD10), LUKS2 and TPM2 behind `zbm.native=` until SE13's review, the others the default once
    their VM tests pass; release images keep the fallback programs until the release after, then only on request;
    FIDO2 with `authenticator` if it covers hmac-secret without its async parts, otherwise our own CTAP2. Linking
    libcryptsetup, tss2-esys and libfido2 stays the way back if the Rust LUKS2 or TPM2 code fails its review.
14. **The crate map** of [crates and layers](#crates-and-layers) ([ADR 0003](docs/dev/adr/0003-crate-map.md)): 24
    crates in 11 layers, dependencies pointing down only, with two new rules for `check-deps`: orchestration does not
    use adapters, and front ends reach the system only through `zbm-app`. The keyholder is an applet of `zbm`
    (`zbm unlock serve`), the host tools' crate is `zbm-host` (DM10), and `zbm-bootenv` is a crate of its own (DM7).
16. **The image plan, and a native image backend after the cutover** ([ADR 0006](docs/dev/adr/0006-image-plan.md)):
    `zbm-host` computes an `ImagePlan`, checked by golden files under `cargo test`; dracut and mkinitcpio install it
    through shims that read it as a manifest of tab-separated lines (SB8), and the mkinitcpio hook runs `zbm` without
    Bash. A native backend writes the initramfs after the cutover (SB12), for release and container images first,
    checked against dracut's output; the build container keeps dracut until it passes the VM suites. Dracut stays a
    backend for remote unlock and distribution packages, mkinitcpio for Arch; the legacy runtime's plan goes with the
    legacy runtime.

Decided on 28 October 2026:

10. **One concurrency model:** threads and channels, one reactor per process, no async runtime
    ([ADR 0001](docs/dev/adr/0001-process-model.md), PM1): a mailbox bounded at 256 messages, deadlines in the poll's
    time-out, `async` only as `brush`'s own in `zbm-brush`.
13. **Trust zones:** the four zones of [the target architecture](#processes-and-trust-zones)
    ([ADR 0002](docs/dev/adr/0002-trust-zones.md)): Z0 the supervisor, Z1 the keyholder with every secret, Z2 the
    session that parses and decides, Z3 the unprivileged front ends, talking over typed messages on Unix sockets; the
    session first (PM9), the keyholder and the unprivileged front ends in phase 5 (SB10, SE11). The recovery shell
    stays in Z0's care behind the shell gate (SE9), hooks run as root in Z2, and an SSH login is a front end in Z3.
15. **The menu draws natively on the console** ([ADR 0004](docs/dev/adr/0004-client-channel.md)): a control socket
    for requests to the console, with replies (PM7; JSON, at `/run/zfsbootmenu/console.sock`, root only), and a client
    channel from the supervisor's socket pair on which the menu sends cells and receives key presses, with the secret
    flag in band (UI6; a binary encoding of our own, one cell client at a time); one front end runtime with the
    console's cells, a terminal and the test backend as its backends (UI3); a terminal only for the programs that
    need one.

Decided on 27 October 2026:

11. **No libzfs** (goal G2 of the old plan): ZFS only through `zfs` and `zpool`, which `deny.toml` enforces.
17. **Parity is not a goal.** The legacy code is no longer the reference; behaviour changes are recorded in
    `CHANGES-FROM-LEGACY.md`, and the parity-only parts are gone.

Open:

6. **When the legacy runtime goes.** Proposed: one release with the Rust runtime as the default and the legacy one
   selectable, then removal (RL1, RL2); no beta in which the legacy image is the reference.
7. **Nucleo or its matcher:** decided by the 10,000-snapshot test (UI10).
8. **The key map:** proposed, the workspace keys of UI13 (`j` `k` `h` `l` and the arrows to move, numbered tabs, one
   letter per action, `:` for the command line), with the classic `MOD+key` aliases of the `fzf` menu; this changes
   rule 1 of `docs/dev/menu-design/keymap.md`, which gave `j` and `k` to actions. Decided by the usability sessions
   (UI11), which compare it with the current map.
18. **Keys that work on every keyboard:** proposed, the four rules of [the menu](#the-menu-a-workspace): nothing needs
    one character only, nothing depends on case, keys by position on the display console, and a command line that
    knows every language (UI15).

Decision 18 is decided by UI13's ADR in phase 0, still to be drafted; the others of phase 0 are accepted
([docs/dev/adr/](docs/dev/adr/): 10, 13 and 15 on 28 October 2026, 12, 14 and 16 on 29 October). Decision 8 is
decided by the sessions that follow.

## Fixes left from the review of 27 October 2026

The review of all the code before M4 found 34 groups of bugs. Phase 0 fixed 1 to 12, 15, 17 to 20, 26 and 28 to 34,
and the part of 16 that needs no design (the screenshot's compression and the status line); DM3 fixed 27. The rest go
with the items that replace the code they are in:

| # | Fix | Where | With |
| --- | --- | --- | --- |
| 13 | A stale `console-released` record survives a dead console | `zbm-console`, supervisor | PM7 |
| 14 | The font keys race `zbm console font` | `zbm-console` | PM7 |
| 16 | The full repaint for a bell | `zbm-console` | UI4 |
| 21 | Options on a hook's `#!` line are dropped | `zbm-tools/src/programs.rs` | SB5 |
| 22 | Compiled hooks are handed to the shell | `zbm-platform/src/shell.rs` | SB5 |
| 23 | The initramfs of prefixes other than `vmlinuz` changed name without a warning; `contrib/syslinux-update.sh` | `generate-zbm` | DM10 |
| 24 | `zbm-kcl` rewrites the live EFI image in place | `zbm-kcl` | DM10 |
| 25 | Versions such as `git-abc` are never pruned; firmware trimming under a symlinked build directory; `config.yaml` rewritten in place | `generate-zbm` | DM10 |

The fixes of phase 0 are in `CHANGELOG.md`, under "Fixes from the bug review before the console's M4". Every Rust VM
test passed on them on 27 October 2026 (`docs/luks/testing.rst`), but for the two `oldzfs` tests, which need the old
image and did not run.

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
