# Refactoring Roadmap

This roadmap turns the ZFSBootMenu code base into an idiomatic Rust project that developers can read, change, debug
and test, and on which a new menu built with [Nucleo] and [Ratatui] is straightforward to write. It is
based on an analysis of the whole tree in October 2026, during Phase 6 of [PORT-ROADMAP.md](PORT-ROADMAP.md), and it
changes that plan: fundamental changes to structure and behaviour are allowed, and every line of code and every test
ends up in Rust.

The menu is redesigned from TUI and UX design principles, not rebuilt screen for screen from the `fzf` menu of the
legacy runtime: [The menu redesign](#the-menu-redesign) sets out the principles and the structure, phase
[D](#d-menu-design-3-to-4-weeks-alongside-r3-and-r4) designs it and tries it on a prototype while R3 and R4 prepare
the code, and R5 builds it.

[Nucleo]: https://github.com/helix-editor/nucleo
[Ratatui]: https://ratatui.rs

## Contents

- [Status](#status): [next](#next), [done outside the phases](#done-outside-the-phases),
  [carried over](#carried-over)
- [Where the code stands](#where-the-code-stands)
- [Goals](#goals)
- [Findings](#findings)
- [Target architecture](#target-architecture)
- [Rules for refactored code](#rules-for-refactored-code)
- [How the rules are enforced](#how-the-rules-are-enforced)
- [Testing strategy](#testing-strategy)
- [Using uutils coreutils](#using-uutils-coreutils)
- [The ZFS boundary](#the-zfs-boundary)
- [Threat model](#threat-model)
- [Phases](#phases)
- [Relation to PORT-ROADMAP.md](#relation-to-port-roadmapmd)
- [Risks](#risks)
- [Decisions](#decisions)
- [Proposals for later](#proposals-for-later)
- [Record of finished work](#record-of-finished-work), and [what the pilots taught](#what-the-pilots-taught)

## Status

In October 2026, on branch `claude/zbm-refactor`:

| Phase | State | In short |
| --- | --- | --- |
| R0 | Done | `cargo xtask`, the agreed crates and MSRV 1.88, `cargo deny`, the layers, the debt and coverage ratchets, two pilot slices |
| R1 | Done | `zbm-types`, `zbm-trace` and the runtime journal, `zbm-testkit` with the legacy results as golden files, the threat model and fuzz targets, crash reports, typed errors, shadow mode |
| R2 | Done | `zbm-platform`, the ZFS port on JSON, the `zbm-crypt` adapters, uutils in images, adapters only, the LUKS device suites in Rust |
| R3 | Done | `zbm-pool`, `zbm-startup`, the `zbm-luks` unlocker, `zbm-config` and the generated man page sections, the size rules, the VM harness in Rust |
| D | In progress | Design, prototype and wireframes done; the usability sessions and the review of the design remain |
| R4 | Done | Application layer: the vocabulary in `zbm-app`, the snapshot, the LUKS use cases and start-up events, `zluks` over `zbm-app` and no front end on an adapter, the `zbm debug` reports and `zbm-debug(8)`, key sources decided in Rust and compared by shadow mode with zero differences |
| R5 to R9 | Not started | Two built-in hooks of R6 were done early |

The [record of finished work](#record-of-finished-work) says what each phase did; [What the pilots
taught](#what-the-pilots-taught) lists what R0's pilots changed in the plan.

### Next

- **R4**, the application layer: `zbm-app` with the `Snapshot`, `Action` and event lists of
  `docs/dev/menu-design/interfaces.md`, the applets as thin front ends over it, the `zbm debug` reports, and shadow
  mode for the key sources of encryption roots.
- **The exit of D**, alongside: the usability sessions on the prototype (at least five people, one on a serial
  console), which the maintainer runs; the review of the design documents; decision 8.

### Done outside the phases

Work on this branch that no phase planned, or that a later phase planned and was done early:

- R6's built-in hooks `30-console-autosize` and `80-luks-close` are `zbm console-autosize`, which sizes the font from
  the display's EDID (HiDPI), and `zluks close`, behind two-line shims.
- ROADMAP.md #20: the console fonts of Rust images are 256-glyph subsets (`zbm console-fonts`, `zbm_types::psf`), and
  the Spleen fonts are an option (`zfsbootmenu_console_font`, `Global.ConsoleFont`); the VM tests
  `runtime-rust-autosize` and `runtime-rust-colours` check them on real kernels.
- The menu prototype goes beyond D's core tasks, with prototypes of ROADMAP.md #4, #6, #7, #11 and #21 (boot checks,
  boot counting and boot once, file restore and package changes, the TPM2 re-seal, the keyboard layout at unlock).
  They are designs for after R5's core tasks, not part of D's exit.
- `cargo xtask test vm` runs the harness as root with `sudo -E`, as `docs/luks/testing.rst` says.
- ROADMAP.md #19, step 1: `zbm-display` (platform layer) draws the menu prototype through DRM with the image's PSF
  fonts and reads the keyboards' input devices; `zbm_tui::menu::display` is its Ratatui backend and loop; the VM test
  `runtime-rust-drm` runs it on `virtio_gpu` and `simpledrm`, and passes.
- ROADMAP.md #19, step 2: `zbm-console` (front ends) and the `zbm console` applet run the Rust runtime's start-up
  program and recovery shell on Alacritty's terminal, drawn through DRM, with `zbm.console=drm`; the supervisor runs
  the programs on its pseudo-terminal and falls back to the kernel console, and `zbm-boot` has it hand the display
  back before `kexec`. The VM test `runtime-rust-console` passes (10 checks).
- ROADMAP.md #19, step 3: the console on the display draws with DejaVu Sans Mono through `swash`, in cells as high as
  the console font `zbm console-autosize` chooses, with box drawing and block characters drawn to fill their cells;
  `zbm.console_font=psf` keeps the console fonts. `runtime-rust-console` checks both, and passes (13 checks).
- ROADMAP.md #19, step 4: keyboard layouts on the console on the display, with `kbvm` (decision 1): XKB keymaps
  compiled when the image is built (`zbm keyboard-layouts`), `zbm.keyboard_layout`, and `zbm console layout`, which
  switches layouts while the console runs. `runtime-rust-console` checks them, and passes (19 checks, with `ê` at the recovery shell's prompt).

### Carried over

What the finished phases left open, and the phase that takes it:

| Item | From | To |
| --- | --- | --- |
| The menu timeout is the text of `/etc/zfsbootmenu.conf` (`+5`); it becomes a type when the menu counts it down | R1 | R5 |
| The start-up's pool import and `zbm-boot` run `zfs` and `zpool` with the legacy command lines rather than the typed port, so that their parity recordings compare | R2, R3 | R5, where behaviour may change, or R9, when parity ends |
| Fixtures of OpenZFS 2.2 and 2.3, and the two `oldzfs` VM tests, which need images with an older ZFS | R2 | When such images exist ([proposal](#proposals-for-later) 10) |
| Warnings for deprecated option names, a change of behaviour | R3 | R5 |
| The build options are read by the shell scripts of the dracut module and the mkinitcpio hook | R3 | R6 |
| The release each option appeared in | R3 | Not planned: the table would have to guess |
| Debt in crates no phase has refactored yet: `zbm-kcl` and `zbm-state` (two long functions, two long files), success flags in `generate-zbm`, `zbm-kcl` and `zbm-zfs` (8), doc lines naming legacy functions (161, most in `zbm-luks`, `zbm-boot` and `generate-zbm`) | R1 to R3 | The phase that next changes each crate; R9 at the latest, when the legacy code they name is gone |
| Line coverage below the 80% of new crates: `zbm` 79%, `xtask` 78%, `zbm-vmtest` 60% (its scenarios run only in VMs) | R0, R3 | Ratcheted; never lowered |
| `zbm-menu`, an empty crate | R0 | R5, which builds the menu in `zbm-tui` |

## Where the code stands

| Part | Language | Size | State |
| --- | --- | --- | --- |
| Host tools: `generate-zbm`, `zbm-kcl` | Rust | 8,200 lines | Done (port phases 1 to 3); `generate-zbm` split in R3 |
| Types and foundation: `zbm-types`, `zbm-core`, `zbm-trace`, `zbm-sys` | Rust | 5,800 lines | R1, R2 |
| Platform: `zbm-platform`, `zbm-coreutils` | Rust | 4,100 lines | R2 |
| State and configuration: `zbm-config`, `zbm-state` | Rust | 7,300 lines | R3 (`zbm-config`) |
| Adapters: `zbm-zfs`, `zbm-crypt`, `zbm-tools` | Rust | 5,900 lines | R2 |
| Domain: `zbm-luks`, `zbm-pool` | Rust | 13,100 lines | R3 |
| Application: `zbm-app` | Rust | 450 lines | The pilot's; R4 |
| Start-up, boot and applets: `zbm-boot`, `zbm-startup`, `zbm` | Rust | 12,100 lines | R3; Rust images boot their default environment |
| Menu: `zbm-tui` | Rust | 11,600 lines | The prototype of D; R5 |
| Test kit: `zbm-testkit` | Rust | 4,700 lines | R1 |
| Developer tools: `xtask`, `zbm-vmtest` | Rust | 18,300 lines | R0 to R3; 46 VM tests |
| Legacy runtime (`legacy/zfsbootmenu`) | Bash | 8,400 lines | Frozen; the menu, boot environments, snapshots and recovery tools are not ported yet (R5, R6) |
| Legacy host tools (`legacy/bin`) | Perl, Bash | 1,800 lines | Replaced; kept to record parity results until R9 |
| LUKS unit suites (`tests/luks/unit`) | Bash | 1,300 lines | Kept while the legacy runtime is: they are its only device suites. The Rust port is `crates/zbm-luks/tests/devices` |
| Upstream test VMs (`tests/*.sh`, `tests/helpers`) | Bash, Perl | 2,800 lines | Interactive; R7 |
| Packaging and scripts (`packaging/`, `scripts/`) | Bash, Python | 2,300 lines | R6, R7 |

The Rust code is 91,500 lines in 24 crates. `cargo xtask test` runs about 1,060 tests in under 20 seconds without
root; six fuzz targets cover the parsers of untrusted input; `xtask metrics` counts 2 long functions, 2 long files, 8
success flags, 161 legacy-named doc lines, 24 uses of the shell helpers (21 in configuration parsing, where they are
allowed) and 16 `unsafe` blocks, all in `zbm-sys`.

## Goals

The goals as given, numbered once, with the measure each phase is checked against:

| # | Goal | Measured by |
| --- | --- | --- |
| G1 | Rust-native idioms, Rust design patterns, [The Rust Style Guide], rustdoc as the documentation standard | `rustfmt` (style edition 2024), `clippy::pedantic` with a short, reasoned allow list, `missing_docs` and `rustdoc::all` denied, every public item documented with an example |
| G2 | No direct binding to libzfs | No FFI to `libzfs`, `libzfs_core` or `libnvpair`; ZFS is reached only through the `zfs` and `zpool` programs (see [The ZFS boundary](#the-zfs-boundary)) |
| G3 | Maintainability and readability for developers | No function over 60 lines, no module over 600 lines, no crate without an architecture page, domain types instead of strings, developer documentation in `docs/dev/` |
| G4 | Modularity, so that a Nucleo + Ratatui menu and other features are easy to add | Front ends depend on one application crate and never on adapters; a new screen touches only `zbm-tui`; a new key method or boot step is one new type registered in one place |
| G5 | Debuggability | Structured `tracing` spans for every stage, volume, pool and external command; a command journal; `zbm debug` reports; every failure carries its cause chain; every panic leaves a crash report that `xtask symbolize` turns into function names and lines |
| G6 | Fundamental changes allowed | Behaviour changes are listed in `CHANGES-FROM-LEGACY.md` with the reason; the legacy runtime is not the reference after the cutover |
| G7 | Unit tests that AI tools work well with | Deterministic, self-describing tests; a machine-readable test map; one behaviour per test; fixtures and golden files instead of shelling out to Bash |
| G8 | Unit tests that people find easy to use | `cargo test` (or `cargo nextest run`) runs everything without root in under a minute; builders for test machines; readable failure diffs; `docs/dev/testing.md` |
| G9 | Heavy use of uutils coreutils | Images ship uutils as their only coreutils; Rust code uses `uucore` where it reimplements coreutils behaviour; tests compare with uutils, not GNU (see [Using uutils coreutils](#using-uutils-coreutils)) |
| G10 | All code and tests in Rust from the start | No Bash, Perl or Python in the tree except the dracut and mkinitcpio entry points that those tools require to be shell, and user hook examples; refactored code is written in Rust first, never ported later |
| G11 | A menu designed from TUI and UX principles, not rebuilt from the `fzf` screens | The task measures of [The menu redesign](#the-menu-redesign): keystrokes for each core task, every screen usable at 52×20, in monochrome and over a 115200-baud serial console, a redraw budget, usability sessions with users before and after R5 |

[The Rust Style Guide]: https://doc.rust-lang.org/stable/style-guide/

## Findings

The analysis at the start of R0 found these, in the code of that time, which this table describes; each finding names
the goals it touches and an example, and ends with where it stands now.

| Part | Language | Size | Tests | State |
| --- | --- | --- | --- | --- |
| `generate-zbm`, `zbm-kcl` (host tools) | Rust | 7,400 lines | 51 unit, 2 parity suites | Done (port phases 1 to 3) |
| `zbm-core`, `zbm-sys`, `zbm-system`, `zbm-log`, `zbm-state`, `zbm-cmdline`, `zbm-zfs` | Rust | 7,900 lines | 125 | Shared libraries |
| `zbm-boot`, `zbm` (start-up, supervisor, applets) | Rust | 8,600 lines | 98 | Port packages A and B |
| `zbm-luks` | Rust | 7,200 lines | 76 | Port package B |
| `zbm-menu` | Rust | 9 lines | 0 | Placeholder |
| `zbm-testutil` | Rust | 2,200 lines | 22 | Fakes and Bash parity harness |
| Legacy runtime (`legacy/zfsbootmenu`) | Bash | 9,200 lines | via suites | Frozen; menu, boot environments, snapshots, recovery tools not yet ported |
| Legacy host tools (`legacy/bin`) | Perl, Bash | 1,800 lines | — | Replaced, kept for parity tests |
| LUKS unit suites (`tests/luks/unit`) | Bash | 1,100 lines | 255 checks | Run against both runtimes; ported to Rust in R2 (`crates/zbm-luks/tests/devices`) |
| LUKS VM tests (`tests/luks/vm`) | Python | 2,700 lines | ~700 checks | Run against both runtimes |
| Upstream test VMs (`tests/*.sh`, `tests/helpers`) | Bash, Perl | 2,800 lines | manual | Interactive |
| Packaging and scripts (`packaging/`, `scripts/`) | Bash, Python | 2,400 lines | partly | Container build, release, dev tools |

The Rust code was in good health by most measures: formatted, clippy-clean with `-D warnings`, documented, `unsafe`
confined to `zbm-sys` (13 blocks), every system call behind a trait with a fake. It was, however, written to *be* the
legacy code in another language, and that showed.

### F1. The code is shaped like the Bash it replaced (G1, G3)

- Functions are named and documented after the shell functions they port: 229 doc comment lines name the legacy
  function they replace, as in "as `luks_xxx` does". A developer must read the Bash to understand the Rust.
- Shell semantics are emulated rather than replaced: `command_substitution`, `read_lines`, `test_integer`,
  `read_first_line`, `trim_ifs` and `echo` are used 71 times outside tests. They exist so that output matches the
  legacy code byte for byte.
- Parsers reproduce the quirks of `sed` and `awk` instead of parsing the format: `zbm_luks::tokens::last_match`
  imitates a greedy `sed` expression over JSON, and `parse_json_metadata` tracks brace depth like the legacy `awk`
  program.
- Values are stringly typed: `Mapping::auto` is `"1"` or `"0"`, `Settings::retry_delay` and log levels are `String`,
  start-up stages are matched by name (`"45-luks-unlock" =>` in `zbm_boot::startup`), booleans travel as `"true"` and
  `""` between functions.

**Now:** the phases R1 to R3 took most of it. The shell helpers are gone outside the parsing of legacy configuration
(71 uses to 24, 21 of them in `zbm-config`, where they are allowed); LUKS2 metadata is read with `serde`; stages,
policies, host IDs and outcomes are types. 161 doc comment lines still name a legacy function (from 229).

### F2. Errors and outcomes are invisible (G5)

- 50 functions outside tests return a `bool` that only says whether they succeeded (`unlock_all`, `export_pool`,
  `cryptsetup_open`, the `try_*` key methods). A failed unlock, import or mount says "no" and logs a line; the cause
  is gone by the time a caller decides what to do.
- Outcomes that are not errors (the user skipped a volume, a device is absent, a token is unsupported) are encoded as
  `false`, `None` or ad-hoc enums, differently in each module.

**Now:** `zbm-luks` and `zbm-boot` return typed errors and outcomes (R1). 8 success flags are left, in `generate-zbm`,
`zbm-kcl` and `zbm-zfs` (from 50).

### F3. Coupling makes new features expensive (G4)

- `zbm_luks::Session` is one type whose behaviour is spread over eleven `impl Session` blocks in eleven files. Every
  key method can reach every other method and every field; there is no seam to add or reorder a method.
- `zbm-boot` mixes orchestration (`Startup`), policy (import retries), IO and presentation (`TimedPrompt` drawing with
  `tput`). The start-up program decides, prints and runs commands in the same functions.
- There is no application layer. The menu (`zbm-menu`, empty) would have to call into `zbm-boot` internals, and
  `crates/DESIGN.md` plans it around `fzf` callbacks as separate processes, which a Ratatui menu does not need.
- `import.rs` and `generate.rs` are 1,300 lines each; `boot.rs` 1,100. Several functions exceed 100 lines
  (`Session::open_device`, `Session::unlock_all`, `Session::tpm2_native_unseal`, `zbm_kcl::run`).

**Now:** `Session` is `Unlocker` with `KeyMethod` strategies and a `TokenHandler` registry (R3); pool import and the
start-up program left `zbm-boot` for `zbm-pool` and `zbm-startup` (R3); `zbm-app` exists from the pilot and is built in
R4. Every crate but `zbm-kcl` and `zbm-state` is within the size limits.

### F4. The ZFS boundary is text tables (G2, G4)

`zbm-zfs` offers `Zfs::get_value(&str, &str) -> Option<String>` and `parse_table` over `-H` output. There are no
`Pool`, `Dataset`, `Snapshot` or `BootEnvironment` types, so every caller re-interprets strings. OpenZFS 2.3 and later
print JSON (`zfs list -j`, `zfs get -j`, `zpool status -j`, `zpool list -j`), which removes most parsing code.

**Now:** done in R2: `trait Zfs` with typed pools, datasets, snapshots and properties, from JSON with a text fallback.
The start-up's import still runs the legacy command lines, so that its parity recordings compare
([Carried over](#carried-over)).

### F5. Debugging means reading the kernel log (G5)

Messages go to `/dev/kmsg` in the legacy formats, filtered by a numeric `loglevel` string. There are no spans, no
correlation between a message and the stage, volume or command that produced it, no record of external commands with
their durations and statuses, and no way to dump the runtime's state. `ztrace` and `zlogtail` are Bash.

**Now:** `tracing` spans, the runtime journal and crash reports (R1), and the `zbm debug` reports (R4, documented in
`zbm-debug(8)`): `crash`, `config`, `state`, `journal`, `commands`, `luks` and `pools`. `zbm debug journal` and
`zbm debug commands` do what `zlogtail` and `ztrace` do for the legacy runtime, from the journal.

### F6. The tests depend on Bash and are hard to scan (G7, G8, G10)

- 71 Rust tests run the legacy Bash or Perl code to compare results. They need `bash`, the legacy tree and Perl
  modules, take seconds each, and stop working when the legacy code is deleted.
- Fake programs are written as closures that match command lines; a reader must simulate the closure to know what
  the machine looks like. Several tests check five or more behaviours at once.
- The suites that exercise real devices are Bash (unit) and Python (VM); the upstream test VMs are interactive Bash.
- There are no snapshot tests, property tests or fuzz targets, and no index from a feature to its tests.

**Now:** no Rust test runs Bash or Perl (R1); `FakeMachine` builders and `cases!` (R1); the device suites (R2) and
the VM harness (R3) are Rust; snapshot, property and fuzz tests exist. Left: the upstream test VMs (R7) and the test
map (`xtask test-map`).

### F7. Coreutils are used implicitly (G9)

Rust code shells out to `cp -a`, `rm -rf`, `chmod -R` and `sort -V` and emulates GNU `sort -V` for kernel ordering,
while images built in the build container (Ubuntu 26.10) already ship uutils, whose `sort -V` differs in corner cases
that `zbm_boot::kernels` documents.

**Now:** done in R2: version order is `uucore`'s `version_cmp`, `zbm-coreutils` copies and removes trees in process,
and images ship the uutils multi-call binary.

### F8. What is good and stays

- The system interfaces (`Runner`, `Files`, `Clock`, `Console`, `Processes`, `Environment`) and their fakes: the core
  of testability. They are kept and sharpened.
- `Command` as a pure description of a program run, with `Input::Secret` and `Stream::CaptureSecret`.
- `Secret`, wiped on drop, and the rule that secrets reach programs only on standard input.
- The supervisor's no-panic discipline and the multi-call `zbm` binary.
- Recorded fixtures (`crates/*/tests/fixtures`) and the VM test scenarios themselves.

## Target architecture

Ports and adapters (hexagonal architecture): the domain and the application layer know nothing about processes,
terminals or files; adapters implement narrow traits; front ends drive the application layer.

```text
                 ┌────────────────────┐  ┌──────────────────┐  ┌──────────────────┐
  front ends     │ zbm-tui            │  │ zbm-cli          │  │ zbm-startup      │
                 │ Ratatui + Nucleo   │  │ zluks, zkexec,   │  │ stages, supervisor│
                 │ (Elm architecture) │  │ zsnapshots, ...  │  │ glue             │
                 └─────────┬──────────┘  └────────┬─────────┘  └────────┬─────────┘
                           └──────────────┬───────┴─────────────────────┘
  application              ┌──────────────▼──────────────┐
                           │ zbm-app: use cases, events, │
                           │ state store, Action enum    │
                           └──────────────┬──────────────┘
  domain services   ┌──────────────┬──────┴───────┬──────────────┬──────────────┐
                    │ zbm-luks     │ zbm-pool     │ zbm-bootenv  │ zbm-kexec    │
                    │ KeyMethod    │ import       │ discovery,   │ kernel load, │
                    │ strategies   │ policies     │ kernels, kcl │ teardown     │
                    └──────┬───────┴──────┬───────┴──────┬───────┴──────┬───────┘
  domain types             └──────────────┴──── zbm-types ──────────────┘
                                   (newtypes, enums, no IO)
  ports (traits)    zbm-platform: Exec, Fs, Clock, Terminal, Signals, Env   zbm-trace
  adapters          zbm-zfs (CLI, JSON)  zbm-crypt (cryptsetup, tpm2, fido2, gpg, ykchalresp)
                    zbm-coreutils (uutils)  real platform impls  zbm-sys (the only unsafe)
```

### Crates after the refactor

| Crate | Replaces | Responsibility | Now |
| --- | --- | --- | --- |
| `zbm-types` | strings across crates | `PoolName`, `DatasetName`, `SnapshotName`, `MappingName`, `DevicePath`, `KernelVersion`, `Stage`, `ImportPolicy`, `LuksMode`, `Timeout`; parsing via `FromStr`, display via `Display`; no IO | Exists (R1); the menu timeout is typed in R5, the LUKS mode is in `zbm-config` |
| `zbm-platform` | `zbm-core::exec`, `zbm-system` | The ports (`Exec`, `Fs`, `Clock`, `Terminal`, `Signals`, `Env`), their real implementations, `Command`, `Secret` | Exists (R2) |
| `zbm-sys` | `zbm-sys` | All `unsafe` (signals, `waitpid`, `poll`, `explicit_bzero`, `kexec_file_load`), nothing else | Exists; `kexec_file_load` comes with `zbm-kexec` |
| `zbm-trace` | `zbm-log`, `KernelLog` | `tracing` subscriber layers: kmsg (legacy line format), JSON journal file, console; command journal; secret redaction | Exists (R1) |
| `zbm-config` | `zbm-cmdline`, crypttab parsing, `/etc/zfsbootmenu.conf` | One `Config` value built from the kernel command line, the configuration file and the crypttab, with typed errors and the source of every value | Exists (R3) |
| `zbm-zfs` | `zbm-zfs` | `trait Zfs` with `ZfsCli` (JSON output, text fallback) and `FakeZfs`; `Pool`, `Dataset`, `Snapshot`, properties | Exists (R2) |
| `zbm-crypt` | command building in `zbm-luks` | Typed wrappers around `cryptsetup`, `tpm2_*`, `fido2-*`, `gpg`, `clevis`, `ykchalresp`; JSON metadata via `serde` | Exists (R2) |
| `zbm-coreutils` | `cp`, `rm`, `chmod`, `sort -V` calls | In-process operations through `uucore` and `std`, and the uutils multi-call binary where a program is needed | Exists (R2) |
| `zbm-luks` | `zbm-luks` | `Unlocker` with a `KeyMethod` strategy chain, `VolumeSet`, mapping state | Exists (R3) |
| `zbm-pool` | `zbm-boot::import`, `hostid` | Import policies as types, the retry loop as a state machine | Exists (R3) |
| `zbm-bootenv` | `zbm-boot::{kernels, commandline}`, port package C | Boot environments, kernels and their order, command lines and caches, native encryption keys and key sources | Planned (R5); the code is in `zbm-boot` |
| `zbm-kexec` | `zbm-boot::boot` | Loading kernels, teardown, exporting pools | Planned (R5); the code is in `zbm-boot` |
| `zbm-app` | — (new) | Use cases (`boot`, `unlock`, `snapshot`, `rollback`, `clone`, `set_default`, `edit_cmdline`, `chroot`, `diff`), an `Action` enum, an event stream, the shared state store | The pilot's `Snapshot` and use cases; built in R4 |
| `zbm-tui` | `zbm-menu`, `lib/zfsbootmenu-ui.sh`, `fzf` | The menu: Elm architecture over `zbm-app`, `nucleo-matcher` for matching, Ratatui widgets per screen | The prototype of D; built in R5 |
| `zbm-startup` | `zbm-boot::{startup, modules, devices, hooks, preinit}` | The start-up pipeline as a list of `Step` values; hook running | Exists (R3) |
| `zbm` | `zbm` | The multi-call binary: thin applets that build a context and call `zbm-app` | Exists |
| `zbm-generate`, `zbm-kcl` | `generate-zbm`, `zbm-kcl` | Host tools, split into library and binary like the runtime | Exist as `generate-zbm` (library and binary) and `zbm-kcl` |
| `zbm-testkit` | `zbm-testutil` | Fakes, machine builders, golden files, assertion helpers; no Bash | Exists (R1) |
| `xtask` | `scripts/`, `packaging/container/*.sh` | Developer automation in Rust: build images, record fixtures, run suites, release | Exists (R0 to R3); `build-image`, `release` and the container's entry point in R7 |
| `zbm-vmtest` | `tests/luks/vm`, `tests/*.sh` | The VM test harness and scenarios in Rust | Exists (R3); the upstream test VMs in R7 |

Crates of today that the table does not list: `zbm-core` (foundation: file tests, option parsing, the shell semantics
that configuration parsing keeps, release information), `zbm-state` (state files), `zbm-tools` (adapters for the
system's programs, R2), and `zbm-menu`, an empty placeholder that R5 removes.

### Patterns, and where they apply

| Pattern | Where | Why |
| --- | --- | --- |
| Newtype | `zbm-types` | Invalid names cannot be built; signatures say what they take |
| Strategy, chain of responsibility | `KeyMethod` (key script, key file, tokens, cached passphrase, YubiKey, prompt); `TokenHandler` per token type | Order is data (`Vec<Box<dyn KeyMethod>>`); a new method is one type |
| State machine | Import retry loop, passphrase prompt, supervisor, start-up pipeline | Each state is an enum variant; transitions are pure functions with tests |
| Builder | `Command`, `Import`, `Config`, test machines | Readable construction with defaults |
| Typestate | `Mapping<Locked>` → `Mapping<Open>` in `zbm-crypt` | Closing an unopened mapping does not compile |
| Command (pattern) | `Action` in `zbm-app` | Front ends send actions; the application performs them and emits events |
| Elm architecture (Model, Message, `update`, `view`) | `zbm-tui` | `update` is pure and testable; `view` is snapshot-testable with Ratatui's `TestBackend` |
| Observer | Events from `zbm-app` (`Unlocked`, `PoolImported`, `Booting`) | The menu, the start-up program and logs react without coupling |
| RAII guards | Mounted key devices, GnuPG homes, TPM sessions, temporary directories | Clean-up happens on every path, including `?` |
| Dependency injection through a context | `Context { platform, zfs, crypt, trace }` passed by reference | No globals; fakes for every test |

### The menu on Nucleo and Ratatui

- `zbm-app` exposes a `Snapshot` of the world (pools, boot environments, kernels, snapshots, LUKS volumes, health)
  and accepts `Action`s; it is the only thing the menu talks to.
- `zbm-tui` keeps a `Model` (current screen, selection, filter text, pending dialog), turns terminal events into
  `Message`s, and has one pure `update(model, message) -> (Model, Vec<Action>)`. Each view of the
  [redesigned menu](#the-menu-redesign) (start-up, main view, command palette, snapshots, kernels, command line
  editor, pools and LUKS volumes, recovery tools, help) is a module with a `view` function and its own messages.
- `nucleo-matcher`, the matcher of Nucleo without its thread pool, scores the filter text against the list of the
  current screen inside `update`, so matching is as pure and testable as the rest of `update`; the list widget
  renders the highlighted positions. The lists of a boot menu hold tens to hundreds of entries, which this matches in
  well under a frame. `nucleo` itself adds `rayon` and `parking_lot` to run matching on a thread pool, built for
  editors that search millions of files; the menu does not take it unless the R0 pilot measures a need. The
  command palette matches across every list at once; the performance test of R5 (10,000 snapshots) decides whether
  that still holds (open [decision](#decisions) 7).
- Key bindings are a table (`keymap.rs`) that the help screen renders, so documentation and behaviour cannot drift.
- Tests: `update` with table-driven cases; `view` with `TestBackend` and `insta` snapshots at 80×25, 120×40 and
  52×20; one end-to-end test per screen through `zbm-app` with fakes.

### The menu redesign

The `fzf` menu grew screen by screen around one tool: a list per screen, key bindings that differ between screens,
start-up and unlock prompts printed on the console before the menu exists, and destructive actions behind a single key
and a yes/no question. The new menu is designed from the tasks users come to do, with these principles, each with the
way it is applied and checked:

| Principle | Applied as | Checked by |
| --- | --- | --- |
| Task first | The core tasks (boot the default, boot another environment or kernel, roll back or boot a snapshot, unlock volumes, edit the command line once, recover a failed boot) shape the layout; booting the default needs no key | Keystrokes per core task, measured on the prototype and in R5 |
| One place, details on demand | One main view: the boot environment list with a detail pane (kernels, snapshots, command line, encryption, pool health); other views open from it and return to it | Every core task starts from the main view |
| Visible state and feedback | A status bar always shows the countdown, pools, LUKS volumes and health; every action reports progress and its outcome as a notification, without blocking | Snapshot tests of each state; no silent action |
| Keyboard first and discoverable | One key map for every view (`Enter` primary action, `Esc` back, `/` filter, `?` help, `:` command palette); a footer shows the keys of the focused element; the palette finds every environment, snapshot, kernel and action by fuzzy match | The key map is a table (`keymap.rs`) that drives input, footer, help and the man page |
| Safe by default | Rollback, destroy and promote show what will be lost and need a typed confirmation; a safety snapshot before a rollback when the pool is writable | A test per destructive action that it cannot run without confirmation |
| Errors that say what to do | Every error view names what failed, why, and the next steps offered (retry, other method, shell) | Each error variant reachable from the menu has a view test |
| Works on every console | 52×20 at least; 16 colours, monochrome and `NO_COLOR`; ASCII borders when the terminal cannot draw lines; meaning never carried by colour alone; terminal capabilities detected at start | Snapshot tests at 80×25, 120×40 and 52×20, in colour, monochrome and ASCII |
| Fast | The first frame shortly after the menu starts; only changed cells are redrawn; matching and redraw fit a frame | Redraw budget over 115200 baud; 10,000 snapshots in the performance test |
| The whole boot in one interface | Start-up progress, the countdown, passphrase, PIN and security key prompts, and pool import problems are views of the menu, through the `Prompter` and `Operator` ports of R3; a plain console fallback when the terminal cannot run the TUI | VM tests drive unlock and import problems through the menu |
| Secrets stay secret | Secret fields never echo, never enter the model as text, and are wiped after use | `assert_no_secret_leaked` over the model, the journal and the screen |

The structure of the menu:

- **Start-up view:** the stages of `zbm-startup` as a progress list; unlock dialogs per volume, offering the key
  methods that apply; import problems with their choices (retry, force, match the host ID, shell).
- **Main view:** the boot environment list and the detail pane (to the right, or below on narrow terminals); the
  countdown in the status bar, stopped by any key.
- **Command palette:** fuzzy matching with `nucleo-matcher` over environments, snapshots, kernels and actions.
- **Views opened from the main view:** snapshots (diff, clone, rollback, boot), kernels, the command line editor
  (validated with the types of `zbm-config`), pools and LUKS volumes (unlock, import, status), recovery tools (logs,
  chroot, shell, firmware setup, power), help generated from the key map.

The design (users and tasks, wireframes, the visual system, the key map) is written in phase
[D](#d-menu-design-3-to-4-weeks-alongside-r3-and-r4) under `docs/dev/menu-design/`; its wireframes, as text, become
the first snapshot tests of R5. Behaviour changes against the `fzf` menu are recorded in `CHANGES-FROM-LEGACY.md`,
written as a migration guide.

### Options defined once

Today each kernel command line option, configuration key and crypttab option is defined three times: a parser in
`zbm-cmdline` or `zbm-luks`, a section in a hand-written man page (`docs/man/zfsbootmenu.7.rst`,
`zfsbootmenu-crypttab.5.rst`, `generate-zbm.5.rst`) and, for the menu, a help file rendered from RST. They drift.

After the refactor, `zbm-config` holds one table of option specifications (`zbm_config::spec`), one `OptionSpec`
each: name, aliases, where it is set, the values it takes, default, a one-line summary, what reads it, its manual
page entry, deprecation and examples. From that table:

- the parsers look options up by their specification, so an option without one cannot be read (outside
  `zbm-config`, the lookup by name is a disallowed method);
- `xtask gen-docs` writes the option sections of the man pages between marker comments in each page, and CI checks
  them; the pages keep their hand-written titles, introductions and examples;
- `zbm debug config` prints each value with its specification and where it came from (`zbm_config::Config`);
- a deprecated name logs a warning naming the option, as the legacy runtime does;
- a table-driven test reads every example of every specification as its option and checks its value.

The menu's key bindings work the same way: `zbm-tui`'s `keymap.rs` table drives the input handling, the help screen
and a generated section of the man page. CI runs `xtask gen-docs --check`, which fails when a generated fragment is
out of date.

### Crash reports

The runtime was built with `strip = true`, so a panic on a user's machine produced a backtrace of bare addresses that
nobody could read. Since R1:

- **Release builds keep their debug information outside the image.** They keep line tables
  (`debug = "line-tables-only"`, no `strip`), and `zbm` links with a GNU build ID; image builds move the debug
  information to `debug/<build-id>.debug` with `cargo xtask split-debug`, so the image size does not change. Releases
  publish the debug files with the images in R6.
- **A panic hook** (`zbm_platform::CrashReporter`) writes a crash report to the journal and to
  `/zfsbootmenu/crash-<time>-<pid>.txt`: the panic message (secrets cannot appear, since `Secret` has a redacted
  `Debug`), the location, the build ID, the open `tracing` spans (`zbm_trace::crash::SpanStack`) and the stack as
  offsets from the binary's load address (`zbm_sys::return_addresses`), so that address space randomisation does not
  matter. The supervisor records a child killed by a signal the same way, then opens the recovery shell with the path
  of the report.
- **`zbm debug crash`** shows the latest report on the machine; **`cargo xtask symbolize <report>`** finds the debug
  file by build ID and prints function names, files and lines with binutils' `addr2line` (the `addr2line` crate is not
  among the agreed crates). The VM test `runtime-rust-crash` checks the whole chain.

## Rules for refactored code

1. **Formatting and lints:** `rustfmt` style edition 2024 with no options; the width rules (`cargo xtask widths`) are
   part of `xtask lint`; `clippy::all` and `clippy::pedantic` denied, each allow carrying a `reason`; `unsafe_code`
   denied outside `zbm-sys`; `missing_docs`, `rustdoc::broken_intra_doc_links` and `rustdoc::missing_crate_level_docs`
   denied.
2. **Documentation:** every crate starts with what it is for, how it fits the architecture diagram and a short
   example; every public item says what it does, its errors (`# Errors`), panics (`# Panics`, ideally none) and has a
   doctest when it can run without a system. Docs explain intent and behaviour, not which legacy function it was.
3. **Size:** functions of at most 60 lines, modules of at most 600 lines including tests; a test module that grows
   past that moves to `tests.rs` next to the module.
4. **Types over strings:** no `String` or `bool` parameter that stands for a domain concept; `&str` only at the
   boundary (parsing input, printing output).
5. **Errors:** one error enum per crate, written with `thiserror`, carrying context
   (`#[source]`); `Result` for everything that can fail; non-error outcomes are enums (`Unlock::Opened`,
   `Unlock::Skipped(Reason)`, `Unlock::Unavailable(Reason)`). Binaries render the chain.
6. **Observability:** every public operation in a service runs in a `tracing` span named after it, with the domain
   identifiers as fields; every external command is logged once with argument list (secrets redacted), duration and
   status.
7. **No hidden IO:** domain types and `update` functions take no context and perform no IO; services take a
   `&Context`.
8. **Secrets:** unchanged rule: secrets reach programs only on standard input, are captured as `Secret`, and never
   reach arguments, the environment, files, logs or the screen; `zbm-trace` redacts `Secret` fields by type.
9. **Dependencies:** vendored, each with a written reason in `docs/dev/dependencies.md`, checked with `cargo deny`
   (licences, advisories, duplicates).
10. **Everything new is Rust:** no new Bash, Perl or Python, including tests and tooling.

## How the rules are enforced

A rule that only a reviewer checks is forgotten within a few pull requests. Every rule above is checked by a tool in
CI, so that developers and AI tools learn the rules from the build instead of from review comments.

| Rule | Tool | How |
| --- | --- | --- |
| Lints (1) | `[workspace.lints]` in `Cargo.toml` | The workspace already denies `unsafe_code` and warns on `missing_docs` and `clippy::all`. The refactor adds `clippy::pedantic`, the `rustdoc` lints and the no-panic lints below. Each lint starts as `warn` and becomes `deny` per crate when the phase that refactors the crate ends (`[lints] workspace = true` plus a crate-level `#![deny(...)]`) |
| No panics in the runtime | `clippy::unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `unreachable`, `todo` | Denied in every crate that runs at boot; tests are exempt (`allow-unwrap-in-tests` and `allow-expect-in-tests` in `clippy.toml`) |
| No hidden IO (7), adapters only (R2 exit) | `disallowed-methods` and `disallowed-types` in `clippy.toml` | `std::process::Command`, `std::fs::*`, `std::env::var*`, `std::thread::sleep`, `std::time::SystemTime::now`, `Instant::now`, `println!` and `eprintln!` are disallowed everywhere. The real implementations in `zbm-platform` and `zbm-sys`, and `xtask`, allow them in one module each with `#[expect(clippy::disallowed_methods, reason = "...")]` |
| Layering (front ends never import adapters) | `xtask check-deps` | Reads `cargo metadata` and fails on any dependency edge that the architecture diagram does not allow (for example `zbm-tui` → `zbm-zfs`, or any domain crate → `zbm-platform`'s real implementations). The allowed edges are a table in `xtask` that `docs/dev/architecture.md` is generated from, so the picture and the check cannot drift |
| Size (3), types over strings (4), errors (5) | `xtask metrics` | Counts, per crate: functions over 60 lines, modules over 600 lines, public functions returning `bool`, `Result`-returning functions, doc comments that name a legacy function, calls to the shell-semantics helpers, tests that run Bash. It writes `target/metrics.json` and a table in the CI summary, and fails when a crate that a finished phase has refactored goes above its limits |
| Formatting and widths (1) | `cargo fmt --check`, `xtask lint` | `xtask lint` runs `rustfmt` and the width check (`cargo xtask widths`) |
| Dependencies (9) | `cargo deny check` | Licences, advisories, duplicate versions and banned crates (`libzfs` bindings among them, for G2) |
| Strict lints stay clean once reached | `xtask lint` with `xtask/strict/crates.txt` | The crates listed there (from R0: `xtask` and the pilot crates) must have no finding of `cargo xtask lint --strict`, and `lint` fails otherwise. Each phase adds the crates it finishes, so a refactored crate cannot slip back while the others are still being worked on |
| Test coverage only goes up | `xtask coverage` with `cargo llvm-cov` | Line coverage per crate has a baseline of its own (`xtask/coverage-baseline.txt`), with the opposite direction to the debt metrics: `--check` fails when a crate's coverage drops below its baseline (to the whole percent), and `--bless` raises it. A new crate must reach 80%. It is a command of its own, run by CI's coverage job, because it runs every test in an instrumented build |
| Documentation matches behaviour | `xtask gen-docs --check` | Fails when a man page fragment generated from the option or key binding tables is out of date ([Options defined once](#options-defined-once)) |

`xtask metrics` recorded the starting point in `xtask/metrics-baseline.txt` at the start of R0. Over the crates of
the port (without `xtask`): 29 functions over 60 lines, 19 files over 600 lines, 50 functions returning a `bool`
success flag, 229 doc comment lines naming legacy functions, 71 uses of the shell-semantics helpers, 71 tests that
run legacy code and 13 `unsafe` blocks. `docs/dev/tooling.md` defines each number. The numbers may only go down, and
each phase's exit names the crates whose numbers must reach the limits, so progress is visible in every CI run rather
than claimed at the end.

## Testing strategy

### Layers

| Layer | What | Where | Runs | Root |
| --- | --- | --- | --- | --- |
| Unit | Pure functions, parsers, `update`, state machines | next to the code | every `cargo test` | no |
| Component | One service with `zbm-testkit` fakes (`FakeMachine`) | `crates/*/tests/` | every `cargo test` | no |
| Golden | Output compared with recorded files (screens, generated configs, command lists) | `crates/*/tests/golden/` | every `cargo test` | no |
| Property | Parsers and orderings with `proptest` | next to the code | every `cargo test`, more cases nightly | no |
| Fuzz | crypttab, command line, JSON metadata, `zfs -j` | `fuzz/` | nightly CI | no |
| Device | Real `cryptsetup`, swtpm, GnuPG on loop devices (the LUKS unit suites, in Rust) | `crates/zbm-luks/tests/devices/` | `xtask test devices` and CI | yes, in a container |
| System | Images booted in QEMU (`zbm-vmtest`) | `crates/zbm-vmtest` | `xtask test vm`, CI with KVM | host with KVM |

### Conventions that make tests easy for people and for AI tools

- **One behaviour per test,** named as a sentence: `a_wrong_pin_counts_towards_the_lockout`,
  `skipped_volumes_are_not_offered_again_on_retry`.
- **Arrange, act, assert,** with the three parts separated by blank lines; the arrange part uses builders:
  `FakeMachine::new().with_luks_volume("/dev/sda2", "pw").with_tpm2_token(0, Pin::Required)`.
- **Tables for variations:** a `cases!` macro or an array of structs with a `name` field, so a failure names the case.
- **Golden files instead of inline expectations** for anything longer than three lines, updated with
  `INSTA_UPDATE=always` or `xtask bless`; each golden directory has a `README.md` saying what produced it.
- **Readable failures:** `pretty_assertions` diffs; custom assertions such as
  `assert_ran!(machine, "cryptsetup open --type luks --key-file=- /dev/sda2 v")` and
  `assert_no_secret_leaked!(machine, b"pw")`.
- **Determinism:** no wall clock, sleep, network or randomness in unit and component tests; seeds are fixed and
  printed on failure (`ZBM_TEST_SEED`).
- **Self-contained:** a test does not depend on another test's state, the host's tools or the legacy tree.
- **Discoverable:** `docs/dev/testing.md` maps features to test files; `xtask test-map` writes `target/test-map.json`
  (test name, file, line, feature tags from `#[doc]` comments) for tools and reviewers; each test module starts with a
  comment that lists the behaviours it covers.
- **Fast:** the whole default suite under one minute on a laptop; `cargo nextest` profiles for CI.

### Shadow mode on real machines

Phases R1 to R4 must not change behaviour, and the golden files and VM tests check that on the machines the tests
describe. Users' machines have disks, firmware, pool layouts and option combinations that no test describes. Shadow
mode checks the refactored code on them without letting it act:

- **What it does.** An image built with `zfsbootmenu_shadow=yes` (a `generate-zbm` option) contains `zbm` and a
  `boot-sel.d` hook. That stage runs after the legacy runtime has chosen a boot environment and before it loads the
  kernel, while pools are still imported, so no frozen legacy code changes. The hook runs `zbm shadow`, which works
  out from the same inputs what the Rust code would have decided, and compares the result with what the legacy
  runtime did:
  - the host ID;
  - the pools imported and how (read-only or not, forced or not);
  - the LUKS volumes opened and their mapping names;
  - the default boot environment, when the user did not choose one in the menu;
  - the kernel and initramfs (from `ZBM_SELECTED_KERNEL` and `ZBM_SELECTED_INITRAMFS`);
  - the key sources of encryption roots.

  Each phase adds the decisions it refactors.
- **It cannot act.** `zbm shadow` runs on a `ReadOnlyExec` adapter. That adapter allows only listed read-only
  commands (`zfs list` and `zfs get`, `zpool list`, `zpool get` and `zpool status`, `cryptsetup luksDump` and
  `cryptsetup status`, `blkid`) and refuses everything else with an error, and a unit test pins the list. It never
  prompts, never reads a secret, gives up after a time budget of a few seconds, and its exit status is ignored, so it
  can never stop or slow a boot.
- **Where the result goes.** Each comparison is a `tracing` event with the inputs and both answers. A summary line
  `ZBM-SHADOW: <n> differences` goes to the console and the kernel log. Because the ZFSBootMenu memory file system is
  gone after `kexec`, testers on UEFI machines can set `zbm.shadow=efivar`: only when there are differences, and at
  most once per boot, the summary is written to one EFI variable under a ZFSBootMenu GUID. `zbm shadow report`, run
  on the booted system, reads and clears it.
- **In tests.** The VM harness builds legacy images with shadow mode and reads the summary line from the serial
  console. Every VM scenario must report zero differences.

### Replacing the Bash-based parity tests

Parity with the legacy code stays the safety net until the cutover, but without running Bash at test time:

1. `xtask record-parity` runs each legacy function on the inputs of today's parity tests and on generated inputs, and
   writes the results as golden files (`tests/golden/legacy/<crate>/<module>/<test>.json`, one per test with its
   cases in order, each with a description of its input).
2. The Rust tests compare against those files; they run without `bash`, Perl or the legacy tree.
3. Where a refactor changes behaviour on purpose, the golden file is updated and the change listed in
   `CHANGES-FROM-LEGACY.md`.

## Using uutils coreutils

The build container's images already contain uutils (Ubuntu 26.10 made it the default). The refactor makes it the
coreutils of ZFSBootMenu everywhere and uses it from Rust:

1. **In images:** `generate-zbm` and the dracut and mkinitcpio modules install the uutils multi-call `coreutils`
   binary and its links as the only coreutils, instead of whatever the host has (GNU or BusyBox). One behaviour on
   every distribution; the recovery shell gets the same tools. Since R2 the modules do this for the programs the
   multi-call binary provides when the host has it (`coreutils --list` works), and take the host's programs
   otherwise; dracut's own modules still install the host's.
2. **In Rust code:** `zbm-coreutils` provides the operations the runtime needs in process, through the `Files` port
   (`copy_into`, `clear_mode_bits`, `remove_tree` so far), and version order is `uucore::version_cmp` (in
   `zbm_types::version`, beside the kernel versions it orders, replacing the GNU emulation of `zbm_boot::kernels`).
   Where a separate program is still needed, it calls the uutils multi-call binary explicitly (`coreutils sort ...`)
   through `Exec`, so tests and images agree on the implementation.
3. **Not in `zbm` itself (decided in R2, Decision 4):** `zbm` could link selected `uu_*` crates and dispatch to their
   `uumain` as applets, the way BusyBox does. The spike did that for the 31 coreutils programs images carry: it works
   (`uumain(args) -> i32`, messages and exit codes as the standalone programs), the MSRV is 1.88, but the applets
   come to 9.5 MB stripped next to `zbm`'s 2.2 MB, against 11.5 MB for the full uutils multi-call binary, and bring
   293 crates to vendor and a process-wide start-up hook (`.init_array`, for `SIGPIPE`) into the binary that runs as
   process 1. Images therefore ship the uutils multi-call binary next to `zbm`.
4. **In tests:** expectations for ordering, paths and formatting come from uutils behaviour, and the device and VM
   suites run with uutils in the image.

Constraints: uutils crates follow a recent MSRV (the workspace MSRV moves with it; it is 1.88 since R0),
are vendored and pinned, and are upgraded deliberately with the golden files as the check.

## The ZFS boundary

- **No libzfs.** No FFI to `libzfs`, `libzfs_core` or `libnvpair`, now or later: their ABI changes between OpenZFS
  releases, and the images ship the CLI tools anyway.
- **The CLI with JSON:** `ZfsCli` runs `zfs` and `zpool` with `-j` (OpenZFS 2.3 and later) and parses with `serde`
  into `Pool`, `Dataset`, `Snapshot`, `Property<T>` and `PoolStatus`. For older ZFS (the `oldzfs` tests), a text
  fallback with `-H -p` produces the same types; the choice is made once, from `zfs version -j`.
- **Kernel interfaces read directly** where they are files: `/proc/spl/kstat/zfs`, `/sys/module/zfs/version`,
  `/proc/self/mounts`.
- **One trait, two implementations:** `trait Zfs` with `ZfsCli` and `FakeZfs` (an in-memory pool model that tests
  configure with builders); every command `ZfsCli` runs is recorded in the command journal.
- **Recorded output:** fixtures for each command and ZFS version (2.2, 2.3, 2.4), refreshed by `xtask record-zfs`.

## Threat model

ZFSBootMenu runs before the operating system, holds the keys of encrypted volumes, and reads data that someone other
than its user may control. The rules about secrets say how keys are handled; a threat model says against whom, which
inputs are hostile, and therefore what must be fuzzed, reviewed and tested. It is written in R1 as
`docs/dev/threat-model.md` and kept current by every phase.

It covers at least:

- **Attackers:** someone with physical access to a powered-off machine (a tampered ESP or boot image, the "evil
  maid"); a malicious USB device that claims to be a key or a keyboard; someone who controls the contents of a pool
  the user imports, such as a disk from another machine; and a process in the booted system that reads what
  ZFSBootMenu left behind.
- **Untrusted input:** pool and dataset properties (`org.zfsbootmenu:commandline` and the other
  `org.zfsbootmenu:*` properties, `keylocation`, `bootfs`), names of pools, datasets, snapshots and files in `/boot`,
  hooks found through `zbm.hookroot`, LUKS headers and token JSON, crypttab and key files on removable devices, and
  the output of the programs the runtime runs.
- **Assets:** passphrases, key files, unsealed TPM2 and FIDO2 secrets, the decryption keys in the kernel, and the
  integrity of the kernel, initramfs and command line that `kexec` starts.
- **What must hold:** for each pair of attacker and asset, the property the code keeps (for example: no secret
  outlives `kexec` in ZFSBootMenu's memory or files; a pool property cannot add an unquoted argument to the kernel
  command line except the one documented property; parsing hostile input fails with an error, never a panic or a
  hang) and the test, fuzz target or review that shows it.
- **Display:** the menu draws names and properties that a hostile pool controls. Control characters and escape
  sequences are replaced before anything reaches the terminal, so that a dataset name cannot move the cursor, change
  the title or forge a prompt; a fuzz target renders hostile names through `view`.
- **Out of scope,** stated as such: for example, a compromised firmware, or an attacker who can already run code as
  root before ZFSBootMenu starts.

Each fuzz target of the [testing strategy](#testing-strategy) names the threat it covers. Before R9 removes the
legacy runtime, an independent security review of the Rust runtime against this model is an exit condition.

## Phases

Each phase ends with every suite green on its own branch merge, the measures of its goals met for the code it
touched, and its docs written. Phases R1 to R4 change structure without changing behaviour (the golden files and VM
tests are the proof); behaviour changes start with R5. Phase D changes no behaviour either: its prototype runs on
fakes and the images are unchanged.

### R0. Decisions, scaffolding and pilot slices (3 to 5 weeks)

**State:** done ([record](#done-in-r0)).

- Decide the [open questions](#decisions): dependency set, MSRV, menu replacing `fzf` outright, when the
  legacy runtime goes.
- Add `xtask` with `lint`, `test`, `doc`, `bless`, `check-deps` and `metrics`; add `cargo deny`, `cargo nextest`, and
  the lints of [How the rules are enforced](#how-the-rules-are-enforced) as warnings. Record the starting metrics.
- Vendor the agreed crates; write `docs/dev/dependencies.md`.
- Write `docs/dev/architecture.md` (the diagram above, crate by crate) and `docs/dev/testing.md`.
- **Pilot slices.** Before any crate is migrated, build two thin features through every layer of the target
  architecture, so that the design is tested on real code while it is still cheap to change:
  1. **Unlocking with a key file**, from `zbm-types` (`DevicePath`, `MappingName`) through `zbm-platform`, `zbm-crypt`
     (typed `cryptsetup open`, a typestate mapping), one `KeyMethod` in a new `zbm-luks` module, a `zbm-app` use case,
     `tracing` spans and the command journal, to the `zluks` applet. Tests at every layer use the conventions of
     [Testing strategy](#testing-strategy). The slice runs beside today's code, behind `zluks --pilot`, and the LUKS
     device suite and VM tests run against it.
  2. **The boot environment list of the new menu,** with Ratatui and Nucleo over a `zbm-app` `Snapshot` built from a
     fake machine, with snapshot tests at the three sizes; then in an image booted in QEMU on a serial console only
     (`-nographic`, `TERM=linux` and `TERM=vt100`) to check terminal set-up, key input, resizing and redraw speed in
     the initramfs.
- What the pilots teach is written into this file and `docs/dev/architecture.md`: crate boundaries, the shape of
  `Context`, the error and event types, and whether Ratatui and Nucleo behave on a serial console. The pilot code is
  kept and becomes the template that R1 to R5 copy.
- **Exit:** CI runs `xtask lint`, `xtask test`, `xtask check-deps` and `xtask metrics`; the decisions are recorded in
  this file; both pilots pass their tests, and the menu pilot is usable on a serial console. If a pilot shows that
  part of the architecture does not work, the architecture is changed here before R1 starts.

### R1. Foundations: types, errors, tracing, test kit, crash reports, shadow mode (5 to 6 weeks)

**State:** done ([record](#done-in-r1)).

- Create `zbm-types` and move every domain concept into it, with `FromStr`, `Display` and property tests.
- Create `zbm-trace`: `tracing` with the kmsg layer (keeping the legacy formats for users who read `dmesg`), the
  JSON journal (`/zfsbootmenu/journal.jsonl`), the command journal and redaction; replace `KernelLog` and `zbm-log`.
- Introduce per-crate error enums and convert `bool`-returning functions to `Result` or outcome enums, crate by
  crate, starting with `zbm-luks` and `zbm-boot`.
- Write the [threat model](#threat-model) (`docs/dev/threat-model.md`), and add a fuzz target for each parser of
  untrusted input it lists that R1 touches.
- Turn `zbm-testutil` into `zbm-testkit`: `FakeMachine` builders, assertion macros, golden helpers; record the legacy
  parity results (`xtask record-parity`) and switch the 71 Bash-running tests to golden files.
- [Crash reports](#crash-reports): the panic hook in `zbm-trace`, the supervisor's record of children killed by a
  signal, split debug files with build IDs in `xtask build-image` (replacing `strip = true` in the release profile),
  and `xtask symbolize`.
- [Shadow mode](#shadow-mode-on-real-machines): `zbm shadow`, `ReadOnlyExec`, the `boot-sel.d` hook, the
  `zfsbootmenu_shadow` image option and `zbm.shadow=efivar`, starting with the decisions today's Rust code already
  makes (host ID, pool imports, LUKS volumes, kernel order).
- **Exit:** no Rust test runs Bash, Perl or the legacy tree; every public function of the touched crates returns
  typed results; `clippy::pedantic` and the no-panic lints denied in `zbm-types`, `zbm-trace`, `zbm-testkit`;
  `xtask metrics` shows those crates within the limits; a VM test makes a test-only applet panic and
  `xtask symbolize` names the function and line from the report; every VM scenario reports zero shadow differences.

### R2. Platform and adapters (3 to 4 weeks)

**State:** done ([record](#done-in-r2)); two items carried over ([Carried over](#carried-over)).

- Merge `zbm-core::exec`, `zbm-system` into `zbm-platform`; keep `zbm-sys` as the only `unsafe`.
- Build `zbm-zfs` on JSON with the text fallback, `FakeZfs` and recorded fixtures for ZFS 2.2 to 2.4.
- Build `zbm-crypt`: `cryptsetup` (JSON metadata through `serde`, replacing the `sed` and `awk` emulation),
  `tpm2_*`, `fido2-*`, `gpg`, `clevis`, `ykchalresp`, with typestate mappings and RAII guards.
- Build `zbm-coreutils` on `uucore` and `std`; switch images to the uutils multi-call binary; run the uutils-in-`zbm`
  spike and decide.
- Remove the shell-semantics helpers (`command_substitution`, `read_lines`, `test_integer`, `echo`) from all code
  except `zbm-config`'s parsing of legacy configuration files.
- **Port the LUKS device suites to Rust** (`tests/luks/unit`, 255 checks, to `crates/zbm-luks/tests/devices/`, run by
  `xtask test devices` in the build container as root). This has to happen before R3: the Bash suites reach the Rust
  code through the `test-seams` build and its hidden `zbm-luks-test` applet, which call `Session` methods that R3
  replaces. The Rust suites call `zbm-crypt` and, from R3 on, `zbm-luks` directly. Both suites run side by side until
  the Rust one has every check of the Bash one, against both runtimes; then the Bash suite and `test-seams` are
  removed.
- **Exit:** adapters are the only code that builds `Command`s, which `disallowed-methods` now enforces; every adapter
  has a fake and fixture tests; the LUKS device suite passes in Rust with the same 255 checks; all VM tests pass
  unchanged with zero shadow differences.

### R3. Domain services (4 to 6 weeks)

**State:** done ([record](#done-in-r3)); items carried over to R5, R6 and R9 ([Carried over](#carried-over)).

- `zbm-luks`: replace `Session` by `Unlocker` + `KeyMethod` strategies + `TokenHandler` registry; the passphrase
  prompt becomes a state machine that asks a `Prompter` port; mapping state behind a small repository type.
- `zbm-pool`: import policies as an enum with methods; the retry loop as a state machine whose transitions are unit
  tested; host ID handling as a type.
- `zbm-startup`: the start-up program as `Vec<Box<dyn Step>>` with `Stage` from `zbm-types`; hooks as a `Step`.
- `zbm-config`: one `Config` from command line, configuration file and crypttab, with the origin of each value for
  `zbm debug config`, built on the [option specification table](#options-defined-once); `xtask gen-docs` generates
  the option sections of `zfsbootmenu(7)`, `zfsbootmenu-crypttab(5)` and `generate-zbm(5)`, and CI checks them.
- Split `import.rs`, `boot.rs`, `generate.rs` along these lines; no module over 600 lines.
- **Port the VM test harness to Rust** (`zbm-vmtest`, alongside R3 and R4): the QEMU driver, a serial console read
  through a terminal emulator (the `vt100` crate), so that screens are matched, and snapshotted with `insta`, as the
  text a user sees, `expect`-style matching of whole lines, swtpm and USB hot-plug, disk builders, and the existing
  LUKS scenarios. This has to be done
  before R5: the menu's VM tests are new, and written once, in Rust, instead of in Python and again in R7. Python and
  Rust harnesses run the same scenarios until their results match, then the Python harness is removed.
- **Exit:** the size and type rules hold in these crates (checked by `xtask metrics`); golden files unchanged; the VM
  tests pass in the Rust harness with zero shadow differences; every option in the man pages comes from the table.

### D. Menu design (3 to 4 weeks, alongside R3 and R4)

**State:** in progress ([record](#done-in-d)): everything but the usability sessions and the review of the design.
The prototype also carries prototypes of ROADMAP.md features beyond the core tasks (#4, #6, #7, #11, #21), which R5
builds after the core tasks.

- Users, tasks and scenarios: the core tasks of [The menu redesign](#the-menu-redesign), and the situations they
  happen in (a desktop with one pool, a server reached over a serial console, a machine with LUKS and a security key,
  a failed boot).
- Information architecture and wireframes of every view at 80×25, 120×40 and 52×20, as text files under
  `docs/dev/menu-design/`.
- The visual system: colour roles as Ratatui `Style`s in one `theme.rs`, their monochrome and `NO_COLOR` forms, ASCII
  borders, and terminal capability detection (from [Proposals for later](#proposals-for-later), item 5).
- The key map: the first version of `keymap.rs`, with the classic profile question decided (open
  [decision](#decisions) 8).
- A prototype: `cargo run -p zbm-tui -- --demo` on a fake machine (item 3 of the proposals), built on the Elm skeleton
  of the R0 menu pilot, running every core task.
- Usability sessions on the prototype with at least five people who use ZFSBootMenu, one of them on a serial console;
  findings fixed in the design or recorded.
- The `Snapshot`, `Action` and event lists the design needs, handed to R4.
- **Exit:** the design documents are reviewed; `theme.rs` and `keymap.rs` are in `zbm-tui`; the prototype runs
  every core task on fakes; the lists for `zbm-app` are agreed.

### R4. Application layer (3 to 4 weeks)

**State:** done, in five steps:

1. **Done.** The vocabulary of `docs/dev/menu-design/interfaces.md` (`Snapshot`, `Action`, `Event` and their
   parts) moved from the menu prototype into `zbm_app::model`, and the prototype imports it from there.
   `App::snapshot` reads the machine into it: the pools (health, read-only, checkpoint), the boot environments (the
   pool's default, encryption, the command line and where it was set), the LUKS volumes (mapping, open or locked, the
   key methods their tokens offer), the countdown, the keyboard layouts and the warnings and errors of the journal.
   Kernels, checks, boot tries and snapshots wait for R5 and port package C; start-up stages arrive as events in step
   2. The pilot's small snapshot is `zbm_app::pilot`.
2. **Done.** `App::act` performs the actions `zluks` needs, each as one unlocking run of `zbm-luks`: `UnlockAll`,
   `Open` (a device or tag, with `zbm.luks_key`) and `Close`, with an `UnlockFinished` event for each volume opened,
   skipped or failed, and a `Finished` outcome whose failure lists the error's causes. Start-up's stage runner sends
   `StageStarted` and `StageFinished` to a `Sink` when a front end gives one, and the snapshot fills the stages from
   the state directory; the menu prototype shows both. Other actions end as failed until their use cases exist.
   Answering the prompts of `zbm-luks` (`Prompter`, and the passphrase read from the console) and of `zbm-pool`
   (`Operator`) through `UnlockRequested` and `ImportProblem` needs the menu to answer them, so it moves to R5, with
   import and booting, which need port package C below `zbm-app`.
3. **Done.** `zluks unlock`, `open` and `close` perform `UnlockAll`, `Open` and `Close` through `App::act`, command
   for command as before (the use cases read the state files to report what changed, and run nothing more); `status`
   and `list` stay displays of the front end. `zbm-startup` moved to the orchestration layer, beside `zbm-boot`,
   whose prompts and recovery it uses: it runs the start-up stages, and `zfsbootmenu-init` is its front end.
   `check-deps` now refuses a front end that depends on an adapter (`NOT_THROUGH`), and none does.
4. **Done.** `zbm debug` gained its reports, documented in `zbm-debug(8)`, all of them read-only: `config` (each
   option of the saved command line with its value and origin, from `zbm-config`'s specification table and
   origins), `state` (the state directory without the journal, the crash reports and the cached key files),
   `journal [--level LEVEL]` (the journal, one line per record with its fields and spans), `commands` (the programs
   the runtime ran, with their statuses and durations, from the journal's `command` spans), and `luks` and `pools`
   (`App::snapshot`, with the LUKS settings of `/etc/zfsbootmenu.conf`). They replace `ztrace` and `zlogtail` for the
   Rust runtime; those stay in the frozen legacy runtime. `zfsbootmenu-init` now runs its programs through the
   traced runner (`System::with_runner`), as `zbm-app` did, so the journal records start-up's commands too. On 59834ed,
   `runtime-rust-saved` passes 36 checks with the reports read in its recovery shell, `runtime-rust` 15, and the
   `secrets` tests find no secret with start-up's commands in the journal.
5. **Done.** The Rust code decides where the key of an encryption root comes from
   (`zbm_boot::keys::key_source`: the prompt, a key file in the image, or a key file copied from
   `org.zfsbootmenu:keysource`, in the legacy order; key sources only for a boot chosen in the menu, as the
   countdown's boot skips them) and shadow mode compares it (`key source`). The LUKS mapping
   names that shadow mode expects are those the unlock use cases give (`zbm_luks::device::mapping_name`, a detached
   header included). The use cases of import and booting arrive in R5 with port package C, and shadow mode covers
   them then. A new VM test, `runtime-legacy-keysource`, boots a boot environment whose key comes from its key
   source, from the countdown and from the menu. On 7a25250 every legacy VM test reports zero shadow differences,
   and the Rust `keys` tests and the LUKS unit suites pass.

- Create `zbm-app`: the `Snapshot` of the world, `Action`, events, and the use cases that the start-up program and
  the applets already need (boot the default environment, unlock, close, import). Their shape follows the lists
  agreed in D, including start-up progress events (stages, unlock requests, import problems), so that the menu can
  show start-up and answer its prompts through the ports of R3.
- Rewrite the applets (`zluks`, `zfsbootmenu-init`, the supervisor glue) as thin front ends over `zbm-app`.
- Add `zbm debug` (`config`, `state`, `journal`, `commands`, `luks`, `pools`, `crash`), replacing `ztrace` and
  `zlogtail`.
- Shadow mode covers the use cases of `zbm-app`; ask users to run the shadow images and report differences before R5
  starts changing behaviour.
- **Exit:** no front end imports an adapter crate; `zbm debug` documented in `zbm-debug(8)`; zero shadow differences
  in the VM tests and no open shadow difference reported by users.

### R5. The new menu (10 to 12 weeks)

**State:** not started. Its input is ready: the prototype, the wireframes as snapshot tests, and the lists of
`docs/dev/menu-design/interfaces.md`.

- `zbm-tui` builds the design of D, in the order of the core tasks: the start-up view and countdown, the main view
  with its detail pane, the command palette, snapshots (diff, clone, rollback and boot, with previews and
  confirmations), kernels and the command line editor, pools and LUKS volumes (unlock and import from the menu instead
  of the recovery shell), recovery tools, help, power.
- Start-up runs inside the menu: the `Prompter` of `zbm-luks` and the `Operator` of `zbm-pool` are answered by
  dialogs; a plain console fallback remains for terminals that cannot run the TUI.
- Port package C (boot environments, kernels, native encryption keys, key sources, booting) into `zbm-bootenv` and
  `zbm-kexec` as the main view needs it, and package D (snapshots, rollback, clone, set default) into `zbm-app` use
  cases.
- Record every behaviour change against the `fzf` menu in `CHANGES-FROM-LEGACY.md`, written as a migration guide. The
  key binding section of the man page is generated from `keymap.rs`.
- Every string the user reads comes from a Fluent message catalogue (`locales/en`), never from a literal in the code,
  so ROADMAP.md #18 (Spanish, French, Dutch and Brazilian Portuguese) is translation work, not a rewrite. Titles,
  plurals, numbers and ages are formatted through the catalogue. Logs and the journal stay English and do not use
  it. A pseudo-language that lengthens every string by a third and accents it is drawn in the snapshot tests.
- Usability sessions on the R5 build, as in D; findings fixed or recorded.
- **Exit:** the VM tests in `zbm-vmtest` drive every core task through the menu and pass; snapshot tests cover
  every view at three sizes, in colour, monochrome and ASCII; the task measures and the redraw budget of
  [The menu redesign](#the-menu-redesign) are met; the performance test with 10,000 snapshots passes; the fuzz
  target for hostile names passes; the menu runs without `fzf` in images, including on a serial console; no string the
  user reads is a literal outside the catalogue, and every view fits at 52×20 in the pseudo-language.

### R6. Recovery tools and image assembly in Rust (3 to 4 weeks)

**State:** not started; two built-in hooks were done early (below).

- Port package F (recovery shell commands `zfs-chroot`, `zsnapshots`, `zkexec`, `mount_esp`, `reboot`, `firmware-setup`,
  `logs`, help viewer) as `zbm-app` use cases: views of the menu's recovery tools, and thin `zbm` applets for scripts
  and the recovery shell.
- Built-in hooks (`20-dracut-console-init`, `30-console-autosize`, `80-luks-close`, `90-xhci-unbind`) become Rust
  steps; user hooks stay shell scripts, run by `zbm-startup`. Done early: `30-console-autosize` is
  `zbm console-autosize` (`zbm_startup::autosize`, sizing the font from the display's EDID for HiDPI screens) behind
  a two-line shim, as `80-luks-close` is `zluks close`.
- Image assembly logic moves from `install-helpers.sh` into `zbm-generate` (a library computing the file list); the
  dracut `module-setup.sh` and the mkinitcpio install hook shrink to shims that call it.
- Releases publish the debug files of every binary next to the images, indexed by build ID, so `xtask symbolize`
  can find them for any released version.
- **Exit:** the only shell scripts left in the runtime are the two packaging shims and the user hook examples.

### R7. Remaining tests and tooling in Rust (3 to 4 weeks, can run alongside R5 and R6)

**State:** not started; the LUKS device suites (R2) and the VM harness (R3) are Rust already.

- The LUKS device suites moved in R2 and the VM harness in R3 and R4. What is left: the upstream test VMs
  (`tests/setup.sh`, `tests/run.sh`) as non-interactive `zbm-vmtest` scenarios.
- Replace `scripts/` and `packaging/container/*.sh` with `xtask` subcommands (`build-image`, `record-fixtures`,
  `release`, `compare-images`); the container's entry point becomes a Rust program.
- **Exit:** no Bash, Perl or Python under `tests/`, `scripts/` or `packaging/container/`; CI runs device and VM tests
  through `xtask`.

### R8. Developer documentation (2 to 3 weeks, finishing alongside R7)

**State:** not started; `docs/dev/` has the architecture, testing, tooling, dependencies, shadow mode, threat
model and menu design pages.

- `docs/dev/`: the boot flow from `zbm` start to `kexec`, stage by stage, with links into rustdoc; "how to add a key
  method", "how to add a menu screen", "how to add a boot step", "how to debug a failing boot" (journal, `zbm debug`,
  QEMU serial logs), "how the tests are organised".
- Rustdoc landing pages per crate with the architecture position; `cargo doc --workspace` published by CI.
- **Exit:** a developer who has not worked on the code base adds a key method and a menu screen using only the
  documentation.

### R9. Cutover and removal (with PORT-ROADMAP phases 7 and 8)

**State:** not started; decision 6 is open.

- The Rust runtime becomes the default after a beta in which the legacy image is the fallback.
- Delete `legacy/`, the golden files recorded from it that no longer guard anything, and the compatibility shims.
- An independent security review of the Rust runtime against the [threat model](#threat-model); its findings are
  fixed or accepted in writing before the legacy runtime is removed.
- **Exit:** the tree contains Rust, the packaging shims, documentation and user hook examples only; the security
  review is done.

### Order and overlap

```text
R0 ─ R1 ─ R2 ─ R3 ─ R4 ─ R5 ─ R6 ─ R9
          │    ├────┤    └────┤
          │    │    │         └ R7: upstream test VMs and tooling (alongside R5 and R6)
          │    │    └ VM harness in Rust (alongside R3 and R4, done before R5)
          │    └ D: menu design and prototype (alongside R3 and R4; its lists shape R4)
          └ LUKS device suites in Rust (done before R3)

R0 includes the pilot slices; R8 (docs) runs alongside R7 and ends before R9. D adds about four weeks of work but
little to the length of the plan, since it overlaps R3 and R4; R5 grows by about four weeks.
```

Done: R0 to R4, with the LUKS device suites and the VM harness in Rust. In progress: D. Next: R5.

Two test ports are tied to the code they protect: the LUKS device suites move to Rust in R2, before R3 rewrites
`zbm-luks`, and the VM harness moves during R3 and R4, before R5 adds the menu's VM tests. Package B's VM tests, now
running against the Rust image, are the regression check from R1 on.

## Relation to PORT-ROADMAP.md

- Packages A and B are done (B's complex-pool VM tests wait for the boot environment code of R5). They are
  refactored in R1 to R4, not ported again.
- Packages C, D, E and F are no longer ported in the current architecture: C and D land in R5 and E is replaced by
  the redesigned menu (designed in D, built in R5), F in R6. This avoids writing the menu twice (once around
  `fzf`, once natively).
- PORT-ROADMAP's rule 6 ("no new features") applies until R4 ends; from R5 on, behaviour changes are allowed and
  recorded in `CHANGES-FROM-LEGACY.md`.
- The legacy freeze, `legacy/RUNTIME-CHANGES.md` and the rule that every suite runs against both images stay until
  R9.

## Risks

| Risk | Mitigation |
| --- | --- |
| Behaviour drifts while structure changes (R1 to R4) | Golden files recorded from today's code, the full VM suite after each phase, and shadow mode on testers' machines; no behaviour change before R5 |
| Documentation drifts from behaviour | Options and key bindings are defined once and their documentation generated; `xtask gen-docs --check` in CI |
| Crashes on users' machines cannot be diagnosed | Crash reports with build IDs and published debug files; `xtask symbolize` |
| Shadow mode harms a boot | Read-only adapter with an allowlist, no prompts or secrets, a time budget, an ignored exit status; opt-in images only |
| The new menu is slower or worse on small consoles than `fzf` | The R0 menu pilot on a serial console; snapshot tests at 52 columns; a performance test with 10,000 snapshots; keep `zbm.menu=fzf` available until R9 if users need it |
| New dependencies grow the image and the attack surface | Measure image size per crate (`xtask size`), `cargo deny` advisories, minimal features, vendoring |
| uutils behaviour differs from what users expect on GNU systems | Golden tests for every coreutils behaviour the runtime relies on; document differences |
| MSRV rises with uutils and Ratatui | Decided in R0: 1.88, raised only with the crates; the build container pins the toolchain |
| JSON output is missing in older ZFS | Text fallback with the same types and fixtures from ZFS 2.2 |
| The refactor stalls half way, with two architectures | The R0 pilots prove the architecture before migration; each phase is complete on its own; adapters can wrap old code during transition; crates move one at a time; `xtask metrics` shows how far each crate has come |
| A refactor breaks the tests that should catch it | Test suites move to Rust before the code they protect changes (device suites in R2, VM harness in R3 and R4), and old and new suites run side by side until they agree |
| A hostile disk, pool or USB device attacks the boot | The [threat model](#threat-model), a fuzz target per untrusted input, an independent security review before R9 |
| Rules erode as more people contribute | Every rule is checked by a tool in CI ([How the rules are enforced](#how-the-rules-are-enforced)) |
| The redesign breaks users' habits | A migration guide in `CHANGES-FROM-LEGACY.md`; the keys of the focused element on every screen; a classic key map if decision 8 adopts it; usability sessions with existing users in D and R5 |
| Design work grows without end | D has a fixed exit: the core tasks only; new ideas go to [Proposals for later](#proposals-for-later); R5 builds what D decided |
| Too many abstractions for the size of the code | Patterns only where the table above says; `docs/dev/architecture.md` explains each one where it is used |

## Decisions

Decided on 22 October 2026:

1. **Dependencies:** accepted for the runtime: `ratatui`, `crossterm`, `nucleo-matcher`, `serde`, `serde_json`,
   `thiserror`, `tracing`, `tracing-subscriber` and `uucore`; for tests: `insta`, `proptest` and
   `pretty_assertions`. Their versions and features are pinned in `[workspace.dependencies]`, with default features
   off where the crate allows it, and each is vendored when a crate first uses it. The menu takes `nucleo-matcher`,
   the matcher of Nucleo, rather than `nucleo`, whose thread pool (`rayon`) a boot menu's short lists do not need
   (see [The menu on Nucleo and Ratatui](#the-menu-on-nucleo-and-ratatui)). `nucleo-matcher` is under MPL-2.0, a
   file-level copyleft licence; `deny.toml` allows it for this crate only. `rexpect` is not taken: the VM harness
   gets its own small expect module in R3. `uucore` needs its default `signals` feature. Added in R3 for the VM
   harness `zbm-vmtest` only, a developer tool that never reaches an image: `vt100`, the terminal emulator the pilot
   asked for, and `regex`, for the patterns the scenarios wait for (the Python harness's were regular expressions).
   Added for ROADMAP.md #19, step 1: `drm` (0.15.0), for the display without a terminal in `zbm-display`.
   Added for ROADMAP.md #19, step 2: `alacritty_terminal` (0.26.0, Apache-2.0, without its `serde` feature), the
   terminal of the console on the display in `zbm-console`, with `rustix` (pseudo-terminals, `termios`, signals) and
   `signal-hook` (0.4) used directly; both already came with `drm` and `alacritty_terminal`.
   Added for ROADMAP.md #19, step 3: `swash` (0.2.10), for TrueType fonts in `zbm-display`, rather than `fontdue`, whose
   parser is unmaintained (RUSTSEC-2026-0192).
   Added for ROADMAP.md #19, step 4: `kbvm` (0.1.7, MIT or Apache-2.0, with its `compose` feature), for keyboard
   layouts in `zbm-display`: XKB keymaps compiled when the image is built, and dead keys through a compose table.
   Chosen over `xkbcommon-rs` (0.1.2, last released in February 2025, with a non-standard licence and `lalrpop`,
   `nix` 0.28 and pinned `phf`), and over reading the kernel's keymap, which holds one layout only.
2. **MSRV:** 1.88, what `ratatui` 0.30, `uucore` 0.12 and `proptest` 1.12 need, checked in CI with `cargo +1.88`; it
   rises only when an agreed crate needs it. The workspace now uses let chains, which 1.88 brought.
3. **Menu:** the Nucleo and Ratatui menu, redesigned in D, replaces `fzf` outright in R5; `fzf` leaves the images
   then.
4. **uutils in `zbm`** (decided by the R2 spike): images ship the uutils multi-call binary next to `zbm`, which does
   not link `uu_*` applets (see [Using uutils coreutils](#using-uutils-coreutils), item 3).
5. **Errors:** `thiserror`, which only runs at build time and adds nothing to the binaries.

Still open:

6. **When the legacy runtime goes:** keep the PORT-ROADMAP beta (phase 7) with the legacy image as fallback, or cut
   over directly after R6; decided before R9.
7. **Nucleo or its matcher:** decision 1 stands (`nucleo-matcher` without the thread pool) unless the command palette
   misses its frame budget in the performance test of R5 (10,000 snapshots); then `nucleo` is taken with its worker
   thread, and `rayon` and `parking_lot` are measured against the image size budget. Decided in R5.
8. **A classic key map:** whether `keymap.rs` offers a second profile close to the `fzf` menu's bindings for users who
   know them; decided at the end of D, from its usability sessions. Proposed in D: no second profile; the
   `MOD+key` bindings work as aliases in their old views ([keymap.md](docs/dev/menu-design/keymap.md)).

## Proposals for later

Improvements that were suggested during review and kept for a later revision of this roadmap. None of them is part of
the plan yet; each would be added to a phase when it is adopted.

1. **Trustworthy fakes:** contract tests that run one suite against both `FakeZfs` and `ZfsCli` (in a VM), and the same
   for the `zbm-crypt` adapters; mutation testing with `cargo-mutants` to measure whether tests catch faults; state
   machine property tests (`proptest-state-machine`) for the import retry loop and the unlocker against a reference
   model.
2. **Debugging failures offline:** `zbm debug bundle` writes a redacted archive of the journal, the commands run, the
   configuration and the pool layout; `xtask replay <bundle>` turns it into a fake machine, so a reported boot
   failure becomes a `cargo test`; boot timelines exported as Chrome trace files for Perfetto. Not adopted; the
   bundle is ROADMAP.md #9.
3. **Developer experience:** a demo mode for the menu (`cargo run -p zbm-tui -- --demo`) on a fake machine
   (adopted: the prototype of phase D);
   Architecture Decision Records in `docs/dev/adr/`; runnable `examples/` per crate; an `AGENTS.md` per crate with its
   invariants, commands and test conventions for AI tools.
4. **Stable error codes:** `ZBM-E0412: no key method could open /dev/sda2`, explained by `zbm explain <code>` and one
   documentation page per code.
5. **Boot environment specifics:** terminal capability detection with ASCII borders, 16 colours and `NO_COLOR`
   (adopted in D and R5);
   budgets for boot time, image size and memory with CI gates; `zeroize` or `secrecy` instead of the hand-written
   `Secret`, `mlock` for secret memory and no core dumps; `cargo vet` and `cargo auditable` for the supply chain.
   Capability detection is adopted; secret wiping and `mlock` are ROADMAP.md #1.
6. **A deprecation policy** for kernel command line options and configuration files: old names keep working with a
   warning for one release and `zbm debug config` lists them; possibly a typed TOML configuration with
   `zbm config check`.
7. **One concurrency model:** threads and channels with a single event loop, no async runtime, documented once for
   the countdown, USB key hot-plug, and matching should a list ever need a worker thread.
8. **Fewer crates, with a rule for splitting:** split a crate only when it has its own dependencies or a second user,
   otherwise use a module; track build time in CI (`cargo build --timings`) against a budget.
9. **The Rust API Guidelines as the review checklist** for G1: naming, conversions, `#[must_use]` on outcome types,
   `#[non_exhaustive]` on public enums that will grow (key methods, events, actions).
10. **A version matrix in CI:** scheduled VM runs across ZFS versions × dracut and mkinitcpio × x86_64 and aarch64,
    to exercise the JSON adapter's text fallback on real older ZFS.
11. **Small, mergeable steps:** one tracking issue per phase with a checklist, no step larger than a reviewable pull
    request, every merge leaves the main branch releasable, and old and new code paths coexist behind internal
    switches rather than on long-lived branches.
12. **A log level policy:** error (the boot cannot continue), warn (a fallback was used), info (each stage and
    decision), debug (each external command), trace (data); a test that a normal boot at the default level fits on an
    80×25 console.
13. **`xtask doctor`:** checks the toolchain, the build container, QEMU and KVM, swtpm and loop devices, and says what
    is missing and how to fix it.
14. **More static checks:** `cargo machete` for unused dependencies, and address and undefined behaviour sanitizers on
    the `zbm-sys` tests.
15. **Measurable pilot criteria:** R0's pilots pass or fail on numbers: binary size (a minimal build with all the
    agreed crates measured 0.43 MB against 1.09 MB for today's `zbm`, a lower bound), added boot time, memory, and
    redraw time over a 115200-baud serial console. The pilots measured binary size, frame times and serial bytes
    per key ([What the pilots taught](#what-the-pilots-taught)).
16. **An upstream sync policy:** watch upstream ZFSBootMenu releases; for each relevant fix, write a test first, port
    it, and log it as `legacy/RUNTIME-CHANGES.md` does today.
17. **A reproducibility check:** build the images twice in different directories and compare them, with build paths
    removed from the binaries (`--remap-path-prefix`), keeping the identical-image guarantee of PORT-ROADMAP phase 3.
    This is ROADMAP.md #5. Done: the CI job `reproducible` builds the release image twice and compares the bytes.
18. **Metrics for rules 2 and 4:** debt columns for public functions of domain crates that take a raw string, and for
    public items without a doc example, ratcheted like the others.

## Record of finished work

What each finished phase did, in the order it was done. [Status](#status) summarises it.

### Done in R0

- `cargo xtask` (`xtask/`, documented in `docs/dev/tooling.md`): `lint` runs every check a pull request must pass
  (rustfmt, line widths, clippy, rustdoc, `check-deps`, `metrics --check`); `lint --strict` reports the lints of
  [How the rules are enforced](#how-the-rules-are-enforced) with the configuration in `xtask/strict/clippy.toml`;
  `test` (under `cargo nextest` when it is installed, profiles in `.config/nextest.toml`), `bless`, `doc`,
  `check-deps`, `metrics`, `coverage` and `widths`.
- The strict-lint list `xtask/strict/crates.txt`: `xtask`, the pilot crates (`zbm-types`, `zbm-trace`,
  `zbm-platform`, `zbm-crypt`, `zbm-app`, `zbm-tui`) and the empty `zbm-menu` have no strict finding, and
  `cargo xtask lint` fails if one appears.
- The dependency layers of today's crates in `xtask/src/deps.rs`, and the tables of `docs/dev/architecture.md`
  generated from them.
- The starting metrics in `xtask/metrics-baseline.txt`; CI fails when a crate's debt rises.
- The starting line coverage in `xtask/coverage-baseline.txt` (92% over the workspace; every crate but `zbm`, 68%,
  and `xtask`, 74%, at 80% or more); CI fails when a crate's coverage drops.
- CI runs `cargo xtask lint` and `cargo xtask test` (with nextest), the strict report and `cargo xtask coverage
  --check` in jobs of their own.
- `docs/dev/` (architecture, testing, tooling, dependencies). The shell width check is replaced by
  `cargo xtask widths`.
- [Decisions](#decisions) 1, 2, 3 and 5. The agreed crates are pinned, with their features, in
  `[workspace.dependencies]`; they build together on Rust 1.88, the new minimum, which also brings let chains.
- `cargo deny` checks licences, advisories, sources and banned crates (`deny.toml`), in CI.
- Package B of PORT-ROADMAP.md is merged into this branch, with its VM results.
- Both pilot slices, with unit, snapshot, property and device tests, and the VM test `runtime-rust-pilot`, which
  runs them in the Rust image on a serial console. [What the pilots taught](#what-the-pilots-taught) lists what they
  changed.

Decisions 4 and 6 belong to later phases (R2 and R9); 4 was decided in R2.

### Done in R1

- `zbm-testutil` is `zbm-testkit`. No Rust test runs Bash, Perl or `legacy/` any more: the 71 tests that did record
  the legacy results in `tests/golden/legacy/` (`zbm_testkit::parity`), `cargo xtask record-parity` records them, and
  CI's coverage job checks them against the legacy code (`ZBM_PARITY=check`). The legacy helpers panic outside a
  recording. `cargo xtask test` takes 8 s instead of 27 s.
- `zbm_testkit::efi` describes EFI images by their payload sections, so recordings made with one systemd stub hold on
  machines with another.

- The threat model, `docs/dev/threat-model.md`: attackers, assets, twelve untrusted inputs, eight properties with
  their evidence and status, and the risks that stay. Fuzz targets in `fuzz/` for the parsers of untrusted input
  (`cargo xtask fuzz`, nightly in CI); `cargo xtask lint` checks that they build. Their first run found a panic on
  `zpool import` output that is not ASCII, now fixed.

- `zbm-types` gains `SnapshotName`, `KernelVersion` with the `sort -V` order (moved from `zbm-boot`), `ImportPolicy`,
  `Stage` and `HookStage`, each with property tests and in the `domain_names` fuzz target. The start-up stages, the
  hook stages and the import policy are typed where `zbm-boot` and `zbm-cmdline` use them. Two concepts stay text for
  now: the menu timeout, which `/etc/zfsbootmenu.conf` keeps as written (`+5`) and which R5's menu counts down, and
  the LUKS mode, `zbm_luks::config::Mode`, which moves with the LUKS domain crate in R3.

- `zbm-trace` replaces `zbm-log` and the `KernelLog` that code passed around. Code logs with `zbm_trace::error!`,
  `warn!`, `notice!`, `info!` and `debug!`; `KmsgLayer` writes those events to the kernel log in the legacy formats,
  with the legacy level filter and marker files, and other `tracing` events never reach it. `zbm_platform::KernelLog`
  only installs the layer for a process (`System::kernel_log`), so no function takes a log parameter any more.
  Reading the log back (`zbm_platform::dmesg`, first `zbm_trace::read`) moved from `zbm-log`.

- The runtime journal: every process that sets up its kernel log also appends every `tracing` event, whatever its
  level, to `/zfsbootmenu/journal.jsonl` (at most 4 MiB a process), and fields named for secrets are written as
  `[redacted]`.
- `zbm_testkit::machine::FakeMachine` builds a machine with LUKS volumes, pools, key files and a command line in one
  expression, and its fake programs answer from them; the LUKS unlocking tests use it in place of their own model.
  `assert_ran!`, `assert_not_ran!`, `assert_no_secret_leaked!` and the table macro `cases!` (used by the `zbm-types`
  tests) make failures name the command or case. `zbm-testkit` passes the strict lints; as test code, its helpers may
  panic (that is how a test fails) and its host helpers, golden files and fake `zfs` may use real files.

- Crash reports. Every runtime program's panic hook (`zbm_platform::CrashReporter`) writes
  `/zfsbootmenu/crash-<time>-<pid>.txt` with the message and place, the build ID, the open spans
  (`zbm_trace::crash::SpanStack`) and the stack as offsets from the load address (`zbm_sys::return_addresses`,
  through the unwinder's `_Unwind_Backtrace`), logs an error naming it and writes it to the journal. The supervisor
  names the report of the start-up program, or writes one when a signal killed it, and the recovery shell says
  where it is; `zbm debug crash` prints it. Release builds keep line tables (`debug = "line-tables-only"`, no
  `strip`), `zbm` links with a build ID, and image builds move the debug information to
  `debug/<build-id>.debug` with `cargo xtask split-debug`; `cargo xtask symbolize` resolves a report with it. The
  VM test `runtime-rust-crash` panics a test-only command (`zbm debug panic`, feature `crash-test`) and checks that
  the symbolized report names the function and line. `xtask` uses `objcopy` and `addr2line` from binutils rather
  than the `addr2line` crate, which is not among the agreed dependencies; the VM test harness runs the tree's own
  `build-init.sh`, so changes to it need no new build container.

- Error enums: `zbm_luks::LuksError` (with `TokenFailure`) and `zbm_boot::BootError`, and outcomes where an
  operation ends in one of several good ways (`Opened`, `UnlockSummary`, `Wait`, `ResumeChoice`, and
  `zbm_types::Answer` for timed prompts). No function of the two crates returns a success flag any more (the metric
  went from 18 and 10 to 0); `bool` is left for questions. The unit tests check the specific outcome and error where
  they checked `true` or `false`.

- Shadow mode ([docs/dev/shadow-mode.md](docs/dev/shadow-mode.md)): `zbm shadow` compares the host ID, the pools
  imported, the LUKS mapping names, the default boot environment and the kernel with what the legacy runtime did,
  through `zbm_platform::ReadOnlyRunner` (an allowlist of read-only commands, no secret input, a five-second
  budget) and without a console (`zbm_platform::NoConsole`). Legacy images built with `zfsbootmenu_shadow=yes` run it
  from the `boot-sel.d` hook `99-zbm-shadow.sh`; `zbm.shadow=efivar` keeps a summary with differences in an EFI
  variable for `zbm shadow report`. The VM harness builds every legacy image with shadow mode and fails a test whose
  serial console reports differences. The key sources of encryption roots are not compared yet: the Rust code does
  not decide them until R4.

R1 is complete: every legacy VM scenario that boots reports zero shadow differences.

### Done in R2

- `zbm-platform` holds the ports and their real implementations: `zbm-core`'s `exec`, `secret` and `error` modules,
  all of `zbm-system` (files, clock, console, processes, environment, the kernel log writer, crash reports) and
  `zbm-trace`'s reader of the kernel log (now `zbm_platform::dmesg`, which runs `dmesg`) moved there, and
  `zbm-system` is gone. `zbm-core` keeps only code that does no I/O through the ports (file tests, `getopts`, shell
  semantics, release information, colours); `zbm-trace` no longer runs programs. The layers are `foundation`
  (`zbm-core`, `zbm-trace`), `platform` (`zbm-platform`) and `state` (`zbm-state`, `zbm-cmdline`). The moved code meets
  the strict lints of `zbm-platform`: the real implementations allow `clippy::disallowed_methods` once per module,
  `SystemRunner::run` is split into steps, and `Status::success` became `Status::is_success`. The platform tests that
  use the fakes of `zbm-testkit` are integration tests (`crates/zbm-platform/tests/`), because the fakes implement the
  traits as other crates see them. The last Rust test that ran Bash, `getopts`' comparison with Bash's `getopts`, now
  checks answers recorded from Bash.

- The ZFS port: `zbm_zfs::Zfs` (version, pools, pool properties, filesystems, snapshots, properties with their
  source, `set`, `inherit`) with `ZfsError`, answered by `ZfsCli`, which reads `-j -p` JSON through `serde` when `zfs
  version -j` works (OpenZFS 2.3 and later) and the `-H -p` text otherwise, into the same types (`Pool`, `Dataset`,
  `Snapshot`, `Property` with `Source`, `Health`, `Version`). Without `-p`, JSON values such as `creation` are written
  for people, which the first recording showed. `zbm_testkit::zfs::FakeZfs` is the in-memory model that tests build
  with builders (the old fake `zfs` program for parity tests is now `ZfsScript`). `cargo xtask record-zfs` defines a
  pool and the commands to record and runs them in a test VM through the harness scenario `runtime-record-zfs`; the
  fixtures of OpenZFS 2.4 are in `crates/zbm-zfs/tests/fixtures/2.4/`, and the fixture tests check that JSON and text
  give the same answers. The menu's snapshot (`zbm-app`) reads through the port. The legacy-compatible command
  wrappers are now `ZfsCommand` and `ZpoolCommand`.

- The `zbm-crypt` adapters: `Cryptsetup` reads tokens from `luksDump --dump-json-metadata` through `serde` (the text
  of `luksDump` and `token export` for cryptsetup before 2.4), replacing the `sed`/`awk` emulation, and opens and
  closes volumes, with a key file, a secret on standard input or a token plugin; `Devices` probes with `blkid` and
  `lsblk`; `Tpm2` (`tpm2_*`, with a `PolicySession` flushed on drop), `Fido2` (`fido2-token`, `fido2-assert`),
  `Clevis` (`jose`, `clevis decrypt`), `YubiKey` (`ykinfo`, `ykchalresp`) and `GnupgHome` (`gpg`, `pcscd`; the home
  is killed with `gpgconf` and removed on drop) answer the key methods of `zbm-luks`, which no longer builds their
  commands. Each adapter has fake-runner tests in `zbm-crypt`; the command lines are unchanged, and the LUKS unit
  suites pass (252 checks) with `ZBMTEST_IMPL=rust`. The ZFS port gained `pool_names`, `devices` and `export`, so
  closing volumes exports pools through it.

- uutils: version order (`zbm_types::version`, so kernel order) is `uucore`'s `version_cmp`, the `sort -V` of uutils,
  replacing the emulation of GNU's `filevercmp`; a recorded fixture of the build container's `sort -V` on generated
  names checks it on every host. `zbm-coreutils` copies, restricts and removes file trees in process through the
  `Files` port (`cp -a`, `chmod -R`, `rm -rf`), which the GnuPG home of `zbm-crypt` uses. The dracut and mkinitcpio
  modules install the uutils multi-call binary and a hard link per program when the host has it (uutils refuses to run
  through a symbolic link whose name is not its own). Decision 4 is recorded: images ship that binary next to `zbm`
  rather than linking `uu_*` applets into it.

- Adapters only: `zbm-tools` (adapters) runs the system's programs (`mount`, `umount`, `mountpoint`, `modprobe`,
  `modinfo`, `insmod`, `udevadm`, `tput`, `stty`, `kexec`, `blkid`, `lsblk`, `objcopy`, `objdump`, hooks, key
  scripts, the recovery shell, editors, Bash's `=~` and `cp`), each with fake-runner tests; `zbm-boot`'s `zfs` and
  `zpool` runs go through `ZfsCommand::text` and `ZpoolCommand::text` with the same command lines. The workspace's
  `clippy.toml` (and the strict one) disallow `Command::new` and `Command::shell`; the adapter modules allow them,
  and test builds allow them with `cfg_attr(test, ...)`. The process supervision of `zbm` (`std::process`, which
  replaces or supervises whole processes) is the one place outside them that starts programs.

- The shell helpers (`command_substitution`, `read_lines`, `test_integer`, `echo`) are gone from all code but the
  parsing of configuration: `zbm-cmdline` and the LUKS configuration and crypttab parser of `zbm-luks`. Program
  output is read with `str::lines` and `trim_end_matches('\n')`, numbers with `parse`; the golden files of the
  legacy results did not change. `ZbmOptions::menu_timeout_seconds` gives the timeout as a number.

- The LUKS device suites in Rust: `crates/zbm-luks/tests/devices/` (`unlock`, `tokens`, `edge`, `config`) ports the
  Bash suites case for case and check for check, 52 + 55 + 81 + 64 = 252 checks, all passing; `cargo xtask test
  devices` runs them as root and CI runs them next to the Bash suites. They call the LUKS code directly, with real LUKS
  volumes on loop devices, swtpm, `systemd-cryptenroll`, clevis and GnuPG; a device runner stands in for
  device-mapper (`cryptsetup open` checks the key with `--test-passphrase` and creates a node in the case's own
  mapper directory), so they need no mount namespace. The Bash stand-ins become Rust: the timed prompts, the console
  input that arrives late (`< <(sleep N; echo ...)`), the background jobs of the race cases (threads), and the
  `luksDump` of systemd 261's token plugins. The three legacy-only checks are unit tests. With `ZBM_DEVICE_TESTS=1`
  set, a suite that cannot run fails rather than skips. The Bash suites and `test-seams` stay while the legacy
  runtime does: they are the only suites that run against it.

- Exit: every LUKS VM test passes on `aaef3a9`, 44 tests and 764 checks with KVM in about 20 minutes over three
  parallel lanes, with zero shadow differences (every legacy image runs shadow mode, and a difference fails its
  test). The two `oldzfs` tests need an image with an older ZFS and were skipped. The run found the uutils symbolic
  link refusal above, and two gaps in the documented host requirements (`systemd-cryptenroll` is its own package
  on newer Ubuntu and Debian; the crash test needs `addr2line` and the Rust toolchain), now in
  `docs/luks/testing.rst`; the crash test runs `cargo xtask symbolize` as the user who started the tests.

R2 is complete. Carried over: `zbm-boot`'s text-mode ZFS runs onto the typed port (R3, with the domain services),
and fixtures of OpenZFS 2.2 and 2.3 (they need images with those versions).

### Done in R3

- `zbm-pool` (domain): pool import moved out of `zbm-boot`. The settings are types (`ImportSettings` with
  `PoolRef`, `Requirement`, `BootfsPreference`, `RetryDelay` and `Build`, instead of strings and a flag);
  `Import` builds `zpool import` from `Target`, `ImportPolicy`, `Access` and `Rewind`; `HostId` is the host ID with
  its `/etc/hostid` bytes. The retry loop of the start-up stage is `ImportLoop`, a state machine whose transitions
  (`imported`, `matched`) do no I/O and are tested as a table of paths; `stage` drives it against the system and
  reaches the console only through the `Operator` port, which `zbm-boot` implements with its countdown prompts and
  recovery shell. The crate meets the size and type rules and the strict lints, and has no doc comment that names a
  legacy function. The parity tests of the stage stay in `zbm-boot` and pass against the legacy stage with their
  recordings unchanged; they caught a dropped debug message during the move. The stage keeps the `zpool` command
  lines of the legacy code, which those recordings compare, so moving it onto the typed ZFS port waits until the
  parity tests against the legacy runtime end (R9) or behaviour may change (R5).

- `zbm-startup` (front end): the start-up program moved out of `zbm-boot`, with the stages of modules, host ID,
  device wait and early hooks, the recovery shell and the start-up hooks (`preinit`). The program is a
  `Vec<Box<dyn Step>>`, one step per `Stage`, in the order of `Stage::ALL` (a test checks it), run by `run_steps`
  against a `StepContext`; `Rerun` replaces the `force` flag. The early-hooks stage is a step; running a hook
  directory stays in `zbm-boot`, which also runs the boot-time hooks. `zbm-startup` has an error enum of its own
  (`StartupError`), meets the strict lints and the size and type rules, and starts at 97% line coverage; `zbm-boot`
  has two long files and six legacy-named doc lines fewer. Moving the parity tests moved their recordings with them,
  unchanged, and showed that `.gitignore` patterns meant for the upstream test VMs (`hostid`, `generate-zbm`,
  `keys`) had kept 18 recordings out of the repository: CI records live, so their absence went unnoticed. They are
  committed now, and match the legacy code.
- `zbm-luks` (domain): the unlock session is `Unlocker` (it was `Session`; the pilot's key chain is now
  `pilot::KeyChain`). The key methods tried before the prompt are strategies (`KeyMethod`: key script, key file,
  tokens, passphrases of other volumes, YubiKey), listed by `key_methods` in the legacy order, which a test checks;
  each answers a `Tried` instead of a flag. LUKS2 tokens go through a registry of `TokenHandler`s (TPM2, clevis,
  FIDO2), each saying whether it needs the user (`Pass`); unknown types still go to cryptsetup's plugins in the
  interactive pass. The passphrase prompt is `PassphrasePrompt`, a state machine whose transitions do no I/O, and
  the mapping state file and skip list are a repository, `MappingStore`, which records where each mapping came from
  (`Origin`). `zbm-luks` has two long functions and seven legacy-named doc lines fewer, and keeps its 95% line
  coverage.
- `zbm-config` (state layer, formerly `zbm-cmdline`): the table of options, 98 entries covering the kernel command
  line (`zbm.*` and the kernel's `loglevel` and `console`), the dracut and mkinitcpio build options, pool and dataset
  properties, crypttab options and `generate-zbm`'s configuration keys. Every option of `zfsbootmenu(7)`,
  `zfsbootmenu-crypttab(5)` and `generate-zbm(5)` comes from the table: `cargo xtask gen-docs` rewrites the marked
  sections of the pages, `cargo xtask lint` and a unit test check them. Rendered with Sphinx, the pages say the
  same as before; two notes moved into the entry they belong to, and the deprecated options are listed in table
  order. The parsers read options by their specification through the `Args` trait (the command line, or the
  `cmdline.d` files the start-up writes, which replaces `State::zbm_arg`); crypttab parsing moved here from
  `zbm-luks`, matched by the canonical names of the table; `generate-zbm` reads its keys and the boot code its ZFS
  properties by their specification. `ZbmOptions::read` records where each value came from (`Origin`: default,
  command line under which name, or an invalid value replaced by the default), and `Config` puts the options, the
  LUKS mode and the origins together for `zbm debug config` in R4. Behaviour is unchanged: the parity recordings
  moved with their tests and still match the legacy code. Not done: the release each option appeared in (the table
  would have to guess it), new warnings for deprecated names (a behaviour change, for R5), and the build options are
  still read by the shell scripts of the dracut module and the mkinitcpio hook (R6, recorded in the table as their
  reader). The strict lints for `zbm-config` are part of R3.5.
- Size and type rules (R3.5): `zbm-boot`'s `boot.rs` is split into `boot/` (ZFS queries, mounts, kernel discovery,
  the boot sequence) and `commandline.rs` lost its long function; `generate-zbm`'s `generate.rs` is split into
  `generate/` (settings, system, kernels, initramfs, bundle, install), `dump.rs` into its emitter and quoting styles,
  and its parity tests into a directory; the long functions of `zbm-luks` (unlocking every volume, TPM2, GnuPG,
  FIDO2, YubiKey, key devices) and `zbm-config` (crypttab options) are split into small steps, with enums where
  outcomes were `Option<Option<_>>`. These four crates and `zbm-boot` now have no function over 60 lines and no file
  over 600 (`xtask metrics`: 0 long functions and files, down from 18 and 10). `zbm-config` and `zbm-luks` pass the
  strict lints and are on the strict list; indexing in the hand-written SHA-256, HMAC and base64 of `zbm-luks` is
  allowed with its reason. Moving tests out of source files lowered the measured coverage of `zbm-boot` and
  `zbm-luks`, whose test lines had counted as covered; new tests of the error paths and of the cryptsetup plugin
  paths for FIDO2 and TPM2 bring both back to their baselines. Behaviour is unchanged: unit, parity and device tests
  pass.
- The VM harness in Rust (R3.6): `zbm-vmtest` has every scenario of the Python harness (layouts, complex, keys,
  firmware, procedures with the documentation's commands, runtime) and the fixture recorders, and `zbm-vmtest run`
  (`cargo xtask test vm`) replaces `run-all.sh`. QEMU's serial console goes to a Unix socket, read by a thread into
  the log, a `vt100` screen and the text patterns match, without escape sequences; `expect` keeps `pexpect`'s rule
  (the earliest match wins). Check messages are those of the Python harness, and `zbm-vmtest compare` set the two
  runs side by side: the 45 tests gave the same 764 checks with the same verdicts, except one in `procedures` that
  the Rust harness, quicker to send the next command, caught failing: `zpool wait -t resilver` returns before ZFS
  drops the replaced disk, so "Replacing a failed disk" in `docs/luks/administration.rst` now waits with `-t
  replace`. The Python harness and `scripts/dev/record-fixtures.py` are removed; CI runs the runtime tests with the
  Rust harness. A test that cannot go on records why as a failed check and still writes its result. The harness is
  unit tested against scripted consoles; its scenarios run only in VMs, so the crate starts at 60% line coverage,
  below the 80% of new crates, as its baseline.

### Done in D

- The design in `docs/dev/menu-design/`: users, core tasks with keystroke budgets and scenarios; the information
  architecture (frame, views, dialogs, navigation); the visual system (colour roles, monochrome, ASCII, capability
  detection, layout by size); the key map, one meaning per key; the `Snapshot`, `Action` and event lists for R4
  (`interfaces.md`); the plan of the usability sessions (`usability.md`).
- In `zbm-tui`, `menu::theme` and `menu::caps` (roles as Ratatui styles, from `TERM`, `NO_COLOR` and the locale) and
  `menu::keymap` (the bindings, the footer and help generated from them, the classic aliases), with the Elm
  architecture of the prototype: `Model`, `update`, `view`, and `menu::world`, the lists proposed for `zbm-app`.
- `cargo run -p zbm-tui -- --demo` runs every core task on a fake machine: two pools (one degraded, with a
  checkpoint), four boot environments, 400 snapshots, three LUKS volumes with a TPM2 PIN, a passphrase and a security
  key that is never plugged in, and with `--foreign` a pool of another host. `--log-keys` records key presses for
  the sessions.
- The wireframes are the prototype's snapshot tests: every view at 120×40, 80×25 and 52×20, and the main views in
  ASCII, in `docs/dev/menu-design/wireframes/`. A test of the core tasks keeps each within its budget: T1 0 keys,
  T2 2, T3 3, T4 3 to boot a snapshot and 4 plus the name to roll back, T5 the secret and Enter, T6 2 plus the edit,
  T7 2.
- The design follows Apple's Human Interface Guidelines adapted to the console (`docs/dev/menu-design/apple-hig.md`):
  a navigation bar with a back label, a countdown banner, grouped lists with value rows, an inspector, alerts with
  verb buttons (the destructive one disabled until the target's name is typed), action sheets with Cancel set apart,
  a dimmed backdrop, a toolbar of at most five actions and `? More`, and title case. Glyphs are limited to those of
  the image's Terminus fonts, which have no check marks or rounded corners; a test of the wireframes reads the
  fonts' Unicode tables and fails on any other.
- Boot health in the prototype (`docs/dev/menu-design/boot-health.md`): checks of whether each environment will
  boot, shown on its row and in the inspector, with a *May Not Boot* sheet before booting one that fails; boot
  counting, with a banner naming the fallback when the default used up its tries (`--failing`) and a palette
  command to give them back; a one-time boot asked by the running system (`--next-boot`); and a Changes view of a
  snapshot that restores marked files behind a safety snapshot. T8, restoring a file, takes 5 keys of its 6.
- Unlocking and upgrades in the prototype (`docs/dev/menu-design/unlocking.md`, `boot-health.md`): when a PCR a TPM2
  key is sealed to changed, the unlock dialog asks for the passphrase and says which PCR and why, then offers to seal
  the key again (`--tpm-changed`); `Ctrl-K` types with another keyboard layout, translated by key position, and a
  wrong passphrase points to it; `Ctrl-R` shows what is typed; the Changes view of a snapshot gains a Packages
  segment, from the package database, with the files of each package. ROADMAP.md gains #15 (reclaim space from
  snapshots) and #16 (pool health in detail).
- Decision 8 is proposed, not taken: the `fzf` menu's `MOD+key` bindings work as aliases in their old views, listed
  under *classic* in help, with no second profile. The usability sessions decide.
- Left for the exit: the usability sessions (at least five people, one on a serial console), which need real users
  and are run by the maintainer, and the review of the design documents.

## What the pilots taught

The two pilot slices of R0 ran through every layer of the target architecture: key-file unlock
(`zluks --pilot open`: `zbm-types`, `zbm-trace`, `zbm-platform`, `zbm-crypt`, the `pilot` of `zbm-luks`,
`zbm-app`) and the boot environment list of the new menu (`zbm-menu-pilot`: `zbm-app`'s `Snapshot`, `zbm-tui`). Both
work in the Rust image on a serial console (`runtime-rust-pilot`). The architecture holds; these are the changes and
measurements they brought, which R1 to R5 follow.

**Crate boundaries and layers.** Three layers were added to the table in `xtask/src/deps.rs`: `types` above `unsafe`
(`zbm-types`), `ports` between platform and adapters (`zbm-platform`), and `application` between domain and
orchestration (`zbm-app`). `zbm-tui` sits with `zbm` among the front ends. `check-deps` caught six new crates
without a layer and four third-party crates nobody had allowed, at once; test-only crates (`insta`, `proptest`,
`pretty_assertions`) are now allowed as development dependencies of every crate (`TEST_THIRD_PARTY`).

**The shape of `Context`.** A `Context` is the runner (traced), the files and the clock. It must not require a
console: the real `System` bundles one, which needs a terminal, and `zluks --pilot` run from a script has none.
`Context::from_parts` builds one from the ports a program has; R2 makes that the only constructor, and the console a
port of front ends.

**Errors and outcomes.** Typestate with a hand-back works: `Mapping<Closed>` → `Mapping<Open>` through
`Cryptsetup::open`, and a failure (`Failed<S>`) returns the mapping unchanged with a `CryptError`, so the chain of key
methods passes it on without clones. `cryptsetup`'s exit statuses map onto typed errors (2 a wrong key, 4 no device, 5
busy), as a real run in the VM confirmed. "One error enum per crate" bends where a struct reads better:
`zbm_types::ParseError` is one struct with a `Reason` enum.

**Events.** `Event` is `#[non_exhaustive]`, so every front end matching it needs a catch-all arm; instead, `Event`
implements `Display` in `zbm-app` and front ends print it. `App::perform` returns `Vec<Event>` for now; the event
stream waits for a use case that emits events over time (R4).

**Tracing and the journal.** The traced runner, a decorator, gives the command journal for free. Spans carry domain
fields only (`device`, `name`, the action's `kind`), never the debug text of a request. Journal lines need the
process ID besides the sequence number, because several programs append to one journal.

**The menu on a serial console.** Ratatui asks the terminal where the cursor is (`ESC [ 6 n`) when it starts; a
serial console with nobody answering makes it fail. `zbm-tui`'s backend therefore never asks: it keeps the cursor
position and the size itself (`QuietBackend`). Serial consoles report no size, or whatever `stty` set; the menu uses
the reported size, else `--size`, else 80×24, in a fixed viewport. `TERM=vt100` gets ASCII borders. Typing filters and
Enter chooses on both `TERM=vt100` and `TERM=linux`. `nucleo-matcher` inside `update` keeps it pure and fast for the
short lists of a boot menu.

**Measured.** In the VM, under software emulation (TCG): frames of 15 to 24 ms on average and 38 to 64 ms at most;
about 76 serial bytes per key typed into the filter, because Ratatui redraws only the cells that change; 0.06 s from
Enter to the answer. The release `zbm` grows from 1.09 MB to 1.78 MB with Ratatui, crossterm, `nucleo-matcher` and
`tracing`, about 1% of a 62 MB initramfs.

**Tests.** Snapshot tests paid off at once: the 52-column screen showed the help line cut mid-word, so the help line
now shows whole key bindings in order of importance. The VM harness needs a terminal emulator: Ratatui moves the
cursor over blank cells instead of writing spaces, so a serial transcript reads `Boot` + cursor move +
`environments`. R3 adds one (the `vt100` crate) to `zbm-vmtest`, which can then take `insta` snapshots of real
screens. Patterns must also wait for the end of a line, or they match half of it. Device tests need a kernel with
device-mapper, which this development sandbox lacks: they skip and say so, and run in CI and in the VM.

**Lints.** `clippy::must_use_candidate` made up 313 of the 1,105 strict findings, nearly all of them getters; the
strict set allows it (`STRICT_ALLOWED`) and puts `#[must_use]` where ignoring a value is a mistake. The pilot crates
have no strict finding.

**Tooling.** `cargo vendor` deletes every other file in `vendor/`, so the list of dependencies moved to
`docs/dev/dependencies.md`.
