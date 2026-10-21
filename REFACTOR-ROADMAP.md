# Refactoring Roadmap

This roadmap turns the ZFSBootMenu code base into an idiomatic Rust project that a computer science student can read,
change, debug and test, and on which a new menu built with [Nucleo] and [Ratatui] is straightforward to write. It is
based on an analysis of the whole tree in October 2026, during Phase 6 of [PORT-ROADMAP.md](PORT-ROADMAP.md), and it
changes that plan: fundamental changes to structure and behaviour are allowed, and every line of code and every test
ends up in Rust.

[Nucleo]: https://github.com/helix-editor/nucleo
[Ratatui]: https://ratatui.rs

## Contents

- [Goals](#goals)
- [Where the code stands](#where-the-code-stands)
- [Findings](#findings)
- [Target architecture](#target-architecture)
- [Rules for refactored code](#rules-for-refactored-code)
- [Testing strategy](#testing-strategy)
- [Using uutils coreutils](#using-uutils-coreutils)
- [The ZFS boundary](#the-zfs-boundary)
- [Phases](#phases)
- [Relation to PORT-ROADMAP.md](#relation-to-port-roadmapmd)
- [Risks](#risks)
- [Decisions for review](#decisions-for-review)

## Goals

The goals as given, numbered once, with the measure each phase is checked against:

| # | Goal | Measured by |
| --- | --- | --- |
| G1 | Rust-native idioms, Rust design patterns, [The Rust Style Guide], rustdoc as the documentation standard | `rustfmt` (style edition 2024), `clippy::pedantic` with a short, reasoned allow list, `missing_docs` and `rustdoc::all` denied, every public item documented with an example |
| G2 | No direct binding to libzfs | No FFI to `libzfs`, `libzfs_core` or `libnvpair`; ZFS is reached only through the `zfs` and `zpool` programs (see [The ZFS boundary](#the-zfs-boundary)) |
| G3 | Maintainability and readability, especially for computer science students | No function over 60 lines, no module over 600 lines, no crate without an architecture page, domain types instead of strings, a guided tour (`docs/dev/`) |
| G4 | Modularity, so that a Nucleo + Ratatui menu and other features are easy to add | Front ends depend on one application crate and never on adapters; a new screen touches only `zbm-tui`; a new key method or boot step is one new type registered in one place |
| G5 | Debuggability | Structured `tracing` spans for every stage, volume, pool and external command; a command journal; `zbm debug` reports; every failure carries its cause chain |
| G6 | Fundamental changes allowed | Behaviour changes are listed in `CHANGES-FROM-LEGACY.md` with the reason; the legacy runtime is not the reference after the cutover |
| G7 | Unit tests that AI tools work well with | Deterministic, self-describing tests; a machine-readable test map; one behaviour per test; fixtures and golden files instead of shelling out to Bash |
| G8 | Unit tests that people find easy to use | `cargo test` (or `cargo nextest run`) runs everything without root in under a minute; builders for test machines; readable failure diffs; `docs/dev/testing.md` |
| G9 | Heavy use of uutils coreutils | Images ship uutils as their only coreutils; Rust code uses `uucore` where it reimplements coreutils behaviour; tests compare with uutils, not GNU (see [Using uutils coreutils](#using-uutils-coreutils)) |
| G10 | All code and tests in Rust from the start | No Bash, Perl or Python in the tree except the dracut and mkinitcpio entry points that those tools require to be shell, and user hook examples; refactored code is written in Rust first, never ported later |

[The Rust Style Guide]: https://doc.rust-lang.org/stable/style-guide/

## Where the code stands

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
| LUKS unit suites (`tests/luks/unit`) | Bash | 1,100 lines | 255 checks | Run against both runtimes |
| LUKS VM tests (`tests/luks/vm`) | Python | 2,700 lines | ~700 checks | Run against both runtimes |
| Upstream test VMs (`tests/*.sh`, `tests/helpers`) | Bash, Perl | 2,800 lines | manual | Interactive |
| Packaging and scripts (`packaging/`, `scripts/`) | Bash, Python | 2,400 lines | partly | Container build, release, dev tools |

The Rust code is in good health by most measures: formatted, clippy-clean with `-D warnings`, documented, `unsafe`
confined to `zbm-sys` (14 blocks), every system call behind a trait with a fake. It was, however, written to *be* the
legacy code in another language, and that shows.

## Findings

Each finding names the goals it touches and an example; the phases below fix them.

### F1. The code is shaped like the Bash it replaced (G1, G3)

- Functions are named and documented after the shell functions they port: 104 doc comments explain a function as "as
  `luks_xxx` does". A student must read the Bash to understand the Rust.
- Shell semantics are emulated rather than replaced: `command_substitution`, `read_lines`, `test_integer`,
  `read_first_line` and `echo` are called 131 times. They exist so that output matches the legacy code byte for byte.
- Parsers reproduce the quirks of `sed` and `awk` instead of parsing the format: `zbm_luks::tokens::last_match`
  imitates a greedy `sed` expression over JSON, and `parse_json_metadata` tracks brace depth like the legacy `awk`
  program.
- Values are stringly typed: `Mapping::auto` is `"1"` or `"0"`, `Settings::retry_delay` and log levels are `String`,
  start-up stages are matched by name (`"45-luks-unlock" =>` in `zbm_boot::startup`), booleans travel as `"true"` and
  `""` between functions.

### F2. Errors and outcomes are invisible (G5)

- 135 functions return `bool` and only 26 return `Result`; `zbm_core::error::Error` is constructed 14 times. A failed
  unlock, import or mount says "no" and logs a line; the cause is gone by the time a caller decides what to do.
- Outcomes that are not errors (the user skipped a volume, a device is absent, a token is unsupported) are encoded as
  `false`, `None` or ad-hoc enums, differently in each module.

### F3. Coupling makes new features expensive (G4)

- `zbm_luks::Session` is one type whose behaviour is spread over eleven `impl Session` blocks in eleven files. Every
  key method can reach every other method and every field; there is no seam to add or reorder a method.
- `zbm-boot` mixes orchestration (`Startup`), policy (import retries), IO and presentation (`TimedPrompt` drawing with
  `tput`). The start-up program decides, prints and runs commands in the same functions.
- There is no application layer. The menu (`zbm-menu`, empty) would have to call into `zbm-boot` internals, and
  `crates/DESIGN.md` plans it around `fzf` callbacks as separate processes, which a Ratatui menu does not need.
- `import.rs` and `generate.rs` are 1,300 lines each; `boot.rs` 1,100. Several functions exceed 100 lines
  (`Session::open_device`, `Session::unlock_all`, `Session::tpm2_native_unseal`, `zbm_kcl::run`).

### F4. The ZFS boundary is text tables (G2, G4)

`zbm-zfs` offers `Zfs::get_value(&str, &str) -> Option<String>` and `parse_table` over `-H` output. There are no
`Pool`, `Dataset`, `Snapshot` or `BootEnvironment` types, so every caller re-interprets strings. OpenZFS 2.3 and later
print JSON (`zfs list -j`, `zfs get -j`, `zpool status -j`, `zpool list -j`), which removes most parsing code.

### F5. Debugging means reading the kernel log (G5)

Messages go to `/dev/kmsg` in the legacy formats, filtered by a numeric `loglevel` string. There are no spans, no
correlation between a message and the stage, volume or command that produced it, no record of external commands with
their durations and statuses, and no way to dump the runtime's state. `ztrace` and `zlogtail` are Bash.

### F6. The tests depend on Bash and are hard to scan (G7, G8, G10)

- 55 Rust tests run the legacy Bash or Perl code to compare results. They need `bash`, the legacy tree and Perl
  modules, take seconds each, and stop working when the legacy code is deleted.
- Fake programs are written as closures that match command lines; a reader must simulate the closure to know what
  the machine looks like. Several tests check five or more behaviours at once.
- The suites that exercise real devices are Bash (unit) and Python (VM); the upstream test VMs are interactive Bash.
- There are no snapshot tests, property tests or fuzz targets, and no index from a feature to its tests.

### F7. Coreutils are used implicitly (G9)

Rust code shells out to `cp -a`, `rm -rf`, `chmod -R` and `sort -V` and emulates GNU `sort -V` for kernel ordering,
while images built in the build container (Ubuntu 26.10) already ship uutils, whose `sort -V` differs in corner cases
that `zbm_boot::kernels` documents.

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

| Crate | Replaces | Responsibility |
| --- | --- | --- |
| `zbm-types` | strings across crates | `PoolName`, `DatasetName`, `SnapshotName`, `MappingName`, `DevicePath`, `KernelVersion`, `Stage`, `ImportPolicy`, `LuksMode`, `Timeout`; parsing via `FromStr`, display via `Display`; no IO |
| `zbm-platform` | `zbm-core::exec`, `zbm-system` | The ports (`Exec`, `Fs`, `Clock`, `Terminal`, `Signals`, `Env`), their real implementations, `Command`, `Secret` |
| `zbm-sys` | `zbm-sys` | All `unsafe` (signals, `waitpid`, `poll`, `explicit_bzero`, `kexec_file_load`), nothing else |
| `zbm-trace` | `zbm-log`, `KernelLog` | `tracing` subscriber layers: kmsg (legacy line format), JSON journal file, console; command journal; secret redaction |
| `zbm-config` | `zbm-cmdline`, crypttab parsing, `/etc/zfsbootmenu.conf` | One `Config` value built from the kernel command line, the configuration file and the crypttab, with typed errors and the source of every value |
| `zbm-zfs` | `zbm-zfs` | `trait Zfs` with `ZfsCli` (JSON output, text fallback) and `FakeZfs`; `Pool`, `Dataset`, `Snapshot`, properties |
| `zbm-crypt` | command building in `zbm-luks` | Typed wrappers around `cryptsetup`, `tpm2_*`, `fido2-*`, `gpg`, `clevis`, `ykchalresp`; JSON metadata via `serde` |
| `zbm-coreutils` | `cp`, `rm`, `chmod`, `sort -V` calls | In-process operations through `uucore` and `std`, and the uutils multi-call binary where a program is needed |
| `zbm-luks` | `zbm-luks` | `Unlocker` with a `KeyMethod` strategy chain, `VolumeSet`, mapping state |
| `zbm-pool` | `zbm-boot::import`, `hostid` | Import policies as types, the retry loop as a state machine |
| `zbm-bootenv` | `zbm-boot::{kernels, commandline}`, port package C | Boot environments, kernels and their order, command lines and caches, native encryption keys and key sources |
| `zbm-kexec` | `zbm-boot::boot` | Loading kernels, teardown, exporting pools |
| `zbm-app` | — (new) | Use cases (`boot`, `unlock`, `snapshot`, `rollback`, `clone`, `set_default`, `edit_cmdline`, `chroot`, `diff`), an `Action` enum, an event stream, the shared state store |
| `zbm-tui` | `zbm-menu`, `lib/zfsbootmenu-ui.sh`, `fzf` | The menu: Elm architecture over `zbm-app`, Nucleo for matching, Ratatui widgets per screen |
| `zbm-startup` | `zbm-boot::{startup, modules, devices, hooks, preinit}` | The start-up pipeline as a list of `Step` values; hook running |
| `zbm` | `zbm` | The multi-call binary: thin applets that build a context and call `zbm-app` |
| `zbm-generate`, `zbm-kcl` | `generate-zbm`, `zbm-kcl` | Host tools, split into library and binary like the runtime |
| `zbm-testkit` | `zbm-testutil` | Fakes, machine builders, golden files, assertion helpers; no Bash |
| `xtask` | `scripts/`, `packaging/container/*.sh` | Developer automation in Rust: build images, record fixtures, run suites, release |
| `zbm-vmtest` | `tests/luks/vm`, `tests/*.sh` | The VM test harness and scenarios in Rust |

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
  `Message`s, and has one pure `update(model, message) -> (Model, Vec<Action>)`. Each screen (boot environments,
  kernels, snapshots, diff, pool status, help, command line editor, LUKS volumes) is a module with a `view` function
  and its own messages.
- Nucleo matches the filter text against the list of each screen in a background thread and reports matches as
  messages; the list widget renders the highlighted positions.
- Key bindings are a table (`keymap.rs`) that the help screen renders, so documentation and behaviour cannot drift.
- Tests: `update` with table-driven cases; `view` with `TestBackend` and `insta` snapshots at 80×25, 120×40 and
  52×20; one end-to-end test per screen through `zbm-app` with fakes.

## Rules for refactored code

1. **Formatting and lints:** `rustfmt` style edition 2024 with no options; the width rules of
   `scripts/dev/check-rust-width.sh` move into `xtask lint`; `clippy::all` and `clippy::pedantic` denied, each allow
   carrying a `reason`; `unsafe_code` denied outside `zbm-sys`; `missing_docs`, `rustdoc::broken_intra_doc_links` and
   `rustdoc::missing_crate_level_docs` denied.
2. **Documentation:** every crate starts with what it is for, how it fits the architecture diagram and a short
   example; every public item says what it does, its errors (`# Errors`), panics (`# Panics`, ideally none) and has a
   doctest when it can run without a system. Docs explain intent and behaviour, not which legacy function it was.
3. **Size:** functions of at most 60 lines, modules of at most 600 lines including tests; a test module that grows
   past that moves to `tests.rs` next to the module.
4. **Types over strings:** no `String` or `bool` parameter that stands for a domain concept; `&str` only at the
   boundary (parsing input, printing output).
5. **Errors:** one error enum per crate, written with `thiserror` (or by hand, see decisions), carrying context
   (`#[source]`); `Result` for everything that can fail; non-error outcomes are enums (`Unlock::Opened`,
   `Unlock::Skipped(Reason)`, `Unlock::Unavailable(Reason)`). Binaries render the chain.
6. **Observability:** every public operation in a service runs in a `tracing` span named after it, with the domain
   identifiers as fields; every external command is logged once with argument list (secrets redacted), duration and
   status.
7. **No hidden IO:** domain types and `update` functions take no context and perform no IO; services take a
   `&Context`.
8. **Secrets:** unchanged rule: secrets reach programs only on standard input, are captured as `Secret`, and never
   reach arguments, the environment, files, logs or the screen; `zbm-trace` redacts `Secret` fields by type.
9. **Dependencies:** vendored, each with a written reason in `vendor/README.md`, checked with `cargo deny` (licences,
   advisories, duplicates).
10. **Everything new is Rust:** no new Bash, Perl or Python, including tests and tooling.

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

### Replacing the Bash-based parity tests

Parity with the legacy code stays the safety net until the cutover, but without running Bash at test time:

1. `xtask record-parity` runs each legacy function on the inputs of today's parity tests and on generated inputs, and
   writes the results as golden files (`tests/golden/legacy/<function>/<case>.json`).
2. The Rust tests compare against those files; they run without `bash`, Perl or the legacy tree.
3. Where a refactor changes behaviour on purpose, the golden file is updated and the change listed in
   `CHANGES-FROM-LEGACY.md`.

## Using uutils coreutils

The build container's images already contain uutils (Ubuntu 26.10 made it the default). The refactor makes it the
coreutils of ZFSBootMenu everywhere and uses it from Rust:

1. **In images:** `generate-zbm` and the dracut and mkinitcpio modules install the uutils multi-call `coreutils`
   binary and its links as the only coreutils, instead of whatever the host has (GNU or BusyBox). One behaviour on
   every distribution; the recovery shell gets the same tools.
2. **In Rust code:** `zbm-coreutils` provides the operations the runtime needs (`copy_tree`, `remove_tree`,
   `chmod_recursive`, `version_cmp`, `canonicalize`, `df`-style queries) in process, using `uucore` (for example
   `uucore::version_cmp` for kernel ordering, replacing the GNU emulation in `zbm_boot::kernels`) and `std::fs`.
   Where a separate program is still needed, it calls the uutils multi-call binary explicitly (`coreutils sort ...`)
   through `Exec`, so tests and images agree on the implementation.
3. **In `zbm` itself (option, decided in R2):** `zbm` can link selected `uu_*` crates and dispatch to their `uumain`
   as applets, the way BusyBox does, which removes the separate coreutils from images. This is measured first:
   binary size, MSRV and the stability of `uumain` across uutils releases.
4. **In tests:** expectations for ordering, paths and formatting come from uutils behaviour, and the device and VM
   suites run with uutils in the image.

Constraints: uutils crates follow a recent MSRV (the workspace MSRV moves with it, from 1.85 to the uutils MSRV),
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

## Phases

Each phase ends with every suite green on its own branch merge, the measures of its goals met for the code it
touched, and its docs written. Phases R1 to R4 change structure without changing behaviour (the golden files and VM
tests are the proof); behaviour changes start with R5.

### R0. Decisions and scaffolding (1 to 2 weeks)

- Decide the [open questions](#decisions-for-review): dependency set, MSRV, menu replacing `fzf` outright, when the
  legacy runtime goes.
- Add `xtask` with `lint`, `test`, `doc`, `bless`; add `cargo deny`, `cargo nextest`, `clippy::pedantic` as warnings.
- Vendor the agreed crates; write `vendor/README.md`.
- Write `docs/dev/architecture.md` (the diagram above, crate by crate) and `docs/dev/testing.md`.
- **Exit:** CI runs `xtask lint` and `xtask test`; the decisions are recorded in this file.

### R1. Foundations: types, errors, tracing, test kit (3 to 4 weeks)

- Create `zbm-types` and move every domain concept into it, with `FromStr`, `Display` and property tests.
- Create `zbm-trace`: `tracing` with the kmsg layer (keeping the legacy formats for users who read `dmesg`), the
  JSON journal (`/zfsbootmenu/journal.jsonl`), the command journal and redaction; replace `KernelLog` and `zbm-log`.
- Introduce per-crate error enums and convert `bool`-returning functions to `Result` or outcome enums, crate by
  crate, starting with `zbm-luks` and `zbm-boot`.
- Turn `zbm-testutil` into `zbm-testkit`: `FakeMachine` builders, assertion macros, golden helpers; record the legacy
  parity results (`xtask record-parity`) and switch the 55 Bash-running tests to golden files.
- **Exit:** no test runs Bash, Perl or the legacy tree; every public function of the touched crates returns typed
  results; `clippy::pedantic` denied in `zbm-types`, `zbm-trace`, `zbm-testkit`.

### R2. Platform and adapters (3 to 4 weeks)

- Merge `zbm-core::exec`, `zbm-system` into `zbm-platform`; keep `zbm-sys` as the only `unsafe`.
- Build `zbm-zfs` on JSON with the text fallback, `FakeZfs` and recorded fixtures for ZFS 2.2 to 2.4.
- Build `zbm-crypt`: `cryptsetup` (JSON metadata through `serde`, replacing the `sed` and `awk` emulation),
  `tpm2_*`, `fido2-*`, `gpg`, `clevis`, `ykchalresp`, with typestate mappings and RAII guards.
- Build `zbm-coreutils` on `uucore` and `std`; switch images to the uutils multi-call binary; run the uutils-in-`zbm`
  spike and decide.
- Remove the shell-semantics helpers (`command_substitution`, `read_lines`, `test_integer`, `echo`) from all code
  except `zbm-config`'s parsing of legacy configuration files.
- **Exit:** adapters are the only code that builds `Command`s; every adapter has a fake and fixture tests; the
  LUKS device suite and all VM tests pass unchanged.

### R3. Domain services (4 to 6 weeks)

- `zbm-luks`: replace `Session` by `Unlocker` + `KeyMethod` strategies + `TokenHandler` registry; the passphrase
  prompt becomes a state machine that asks a `Prompter` port; mapping state behind a small repository type.
- `zbm-pool`: import policies as an enum with methods; the retry loop as a state machine whose transitions are unit
  tested; host ID handling as a type.
- `zbm-startup`: the start-up program as `Vec<Box<dyn Step>>` with `Stage` from `zbm-types`; hooks as a `Step`.
- `zbm-config`: one `Config` from command line, configuration file and crypttab, with the origin of each value for
  `zbm debug config`.
- Split `import.rs`, `boot.rs`, `generate.rs` along these lines; no module over 600 lines.
- **Exit:** the size and type rules hold in these crates; golden files and VM tests unchanged.

### R4. Application layer (3 to 4 weeks)

- Create `zbm-app`: the `Snapshot` of the world, `Action`, events, and the use cases that the start-up program and
  the applets already need (boot the default environment, unlock, close, import).
- Rewrite the applets (`zluks`, `zfsbootmenu-init`, the supervisor glue) as thin front ends over `zbm-app`.
- Add `zbm debug` (`config`, `state`, `journal`, `commands`, `luks`, `pools`), replacing `ztrace` and `zlogtail`.
- **Exit:** no front end imports an adapter crate; `zbm debug` documented in `zbm-debug(8)`.

### R5. The new menu (6 to 8 weeks)

- `zbm-tui` with Ratatui and Nucleo, screen by screen, in the order users meet them: boot environments, kernels,
  snapshots and their actions, diff, pool status, help, command line editor, LUKS volumes (new: unlock from the menu
  instead of the recovery shell), power menu.
- Port package C (boot environments, kernels, native encryption keys, key sources, booting) into `zbm-bootenv` and
  `zbm-kexec` as the boot environment screen needs it, and package D (snapshots, rollback, clone, set default) into
  `zbm-app` use cases.
- Decide and document every behaviour change against the `fzf` menu in `CHANGES-FROM-LEGACY.md` (key bindings,
  layout, previews).
- **Exit:** the VM tests that drive the menu are rewritten for the new screens and pass; snapshot tests cover every
  screen at three sizes; the menu runs without `fzf` in images.

### R6. Recovery tools and image assembly in Rust (3 to 4 weeks)

- Port package F (recovery shell commands `zfs-chroot`, `zsnapshots`, `zkexec`, `mount_esp`, `reboot`, `firmware-setup`,
  `logs`, help viewer) as `zbm` applets over `zbm-app`.
- Built-in hooks (`20-dracut-console-init`, `30-console-autosize`, `80-luks-close`, `90-xhci-unbind`) become Rust
  steps; user hooks stay shell scripts, run by `zbm-startup`.
- Image assembly logic moves from `install-helpers.sh` into `zbm-generate` (a library computing the file list); the
  dracut `module-setup.sh` and the mkinitcpio install hook shrink to shims that call it.
- **Exit:** the only shell scripts left in the runtime are the two packaging shims and the user hook examples.

### R7. Tests and tooling in Rust (4 to 6 weeks, can run alongside R3 to R6)

- Port the LUKS unit suites (`tests/luks/unit`, 255 checks) to `crates/zbm-luks/tests/devices/`, run by
  `xtask test devices` in the build container as root; drop the `test-seams` build, which the new ports make
  unnecessary.
- Port the VM harness and scenarios (`tests/luks/vm`, Python) to `zbm-vmtest`: a QEMU driver, a serial console with
  `expect`-style matching, swtpm and USB hot-plug, disk builders; then the upstream test VMs (`tests/setup.sh`,
  `tests/run.sh`) as non-interactive scenarios.
- Replace `scripts/` and `packaging/container/*.sh` with `xtask` subcommands (`build-image`, `record-fixtures`,
  `release`, `compare-images`); the container's entry point becomes a Rust program.
- **Exit:** no Bash, Perl or Python under `tests/`, `scripts/` or `packaging/container/`; CI runs device and VM tests
  through `xtask`.

### R8. Documentation for students (2 to 3 weeks, finishing alongside R7)

- `docs/dev/`: a guided tour (follow one boot from `zbm` start to `kexec`, with links into rustdoc), "how to add a
  key method", "how to add a menu screen", "how to add a boot step", "how to debug a failing boot" (journal, `zbm
  debug`, QEMU serial logs), "how the tests are organised".
- Rustdoc landing pages per crate with the architecture position; `cargo doc --workspace` published by CI.
- Glossary of ZFS, LUKS, TPM and boot terms for readers new to them.
- **Exit:** a reviewer new to the project adds a trivial key method and a trivial menu screen by following the
  guides alone.

### R9. Cutover and removal (with PORT-ROADMAP phases 7 and 8)

- The Rust runtime becomes the default after a beta in which the legacy image is the fallback.
- Delete `legacy/`, the golden files recorded from it that no longer guard anything, and the compatibility shims.
- **Exit:** the tree contains Rust, the packaging shims, documentation and user hook examples only.

### Order and overlap

```text
R0 ─ R1 ─ R2 ─ R3 ─ R4 ─ R5 ─ R6 ─ R9
                └──── R7 (tests, tooling) ────┘
                          R8 (docs) ──────────┘
```

R7 starts once R2 has stable ports, because the device and VM tests in Rust use them. Package B's VM tests, now
running against the Rust image, are the regression check from R1 on.

## Relation to PORT-ROADMAP.md

- Packages A and B are done (B's VM tests are being completed). They are refactored in R1 to R4, not ported again.
- Packages C, D, E and F are no longer ported in the current architecture: C and D land in R5 and E is replaced by
  the Ratatui menu in R5, F in R6. This avoids writing the menu twice (once around `fzf`, once natively).
- PORT-ROADMAP's rule 6 ("no new features") applies until R4 ends; from R5 on, behaviour changes are allowed and
  recorded in `CHANGES-FROM-LEGACY.md`.
- The legacy freeze, `legacy/RUNTIME-CHANGES.md` and the rule that every suite runs against both images stay until
  R9.

## Risks

| Risk | Mitigation |
| --- | --- |
| Behaviour drifts while structure changes (R1 to R4) | Golden files recorded from today's code and the full VM suite after each phase; no behaviour change before R5 |
| The new menu is slower or worse on small consoles than `fzf` | Snapshot tests at 52 columns; a performance test with 10,000 snapshots; keep `zbm.menu=fzf` available until R9 if users need it |
| New dependencies grow the image and the attack surface | Measure image size per crate (`xtask size`), `cargo deny` advisories, minimal features, vendoring |
| uutils behaviour differs from what users expect on GNU systems | Golden tests for every coreutils behaviour the runtime relies on; document differences |
| MSRV rises with uutils and Ratatui | Decide the MSRV in R0; the build container pins the toolchain |
| JSON output is missing in older ZFS | Text fallback with the same types and fixtures from ZFS 2.2 |
| The refactor stalls half way, with two architectures | Each phase is complete on its own; adapters can wrap old code during transition; crates move one at a time |
| Students meet too many abstractions | Patterns only where the table above says; the guided tour explains each one where it is used |

## Decisions for review

1. **Dependencies:** accept `ratatui`, `crossterm`, `nucleo`, `serde`, `serde_json`, `thiserror`, `tracing`,
   `tracing-subscriber`, `uucore` (and possibly `uu_*`) for the runtime, and `insta`, `proptest`,
   `pretty_assertions`, `rexpect` (or an own expect module) for tests. Today the runtime has none.
2. **MSRV:** raise it from 1.85 to what uutils and Ratatui need, and keep one pinned toolchain in the container.
3. **Menu:** replace `fzf` outright in R5 (recommended), or keep it selectable until the cutover.
4. **uutils in `zbm`:** link `uu_*` applets into `zbm` (one binary, BusyBox style) or ship the uutils multi-call
   binary next to it; decided by the R2 spike.
5. **`thiserror` or hand-written errors:** `thiserror` is idiomatic and short; hand-written enums avoid a proc-macro
   dependency in the boot image.
6. **When the legacy runtime goes:** keep the PORT-ROADMAP beta (phase 7) with the legacy image as fallback, or cut
   over directly after R6.
