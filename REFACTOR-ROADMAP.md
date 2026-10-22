# Refactoring Roadmap

This roadmap turns the ZFSBootMenu code base into an idiomatic Rust project that developers can read, change, debug
and test, and on which a new menu built with [Nucleo] and [Ratatui] is straightforward to write. It is
based on an analysis of the whole tree in October 2026, during Phase 6 of [PORT-ROADMAP.md](PORT-ROADMAP.md), and it
changes that plan: fundamental changes to structure and behaviour are allowed, and every line of code and every test
ends up in Rust.

[Nucleo]: https://github.com/helix-editor/nucleo
[Ratatui]: https://ratatui.rs

## Contents

- [Status](#status)
- [Goals](#goals)
- [Where the code stands](#where-the-code-stands)
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

## Status

| Phase | State |
| --- | --- |
| R0 | In progress on branch `claude/zbm-refactor` |
| R1 to R9 | Not started |

Done in R0:

- `cargo xtask` (`xtask/`, documented in `docs/dev/tooling.md`): `lint` runs every check a pull request must pass
  (rustfmt, line widths, clippy, rustdoc, `check-deps`, `metrics --check`); `lint --strict` reports the lints of
  [How the rules are enforced](#how-the-rules-are-enforced) with the configuration in `xtask/strict/clippy.toml`;
  `test`, `doc`, `check-deps`, `metrics` and `widths`. `xtask` itself passes the strict lints.
- The dependency layers of today's crates in `xtask/src/deps.rs`, and the tables of `docs/dev/architecture.md`
  generated from them.
- The starting metrics in `xtask/metrics-baseline.txt`; CI fails when a crate's debt rises.
- CI runs `cargo xtask lint` and `cargo xtask test`, and the strict report in a job of its own.
- `docs/dev/` (architecture, testing, tooling) and `vendor/README.md`. The shell width check is replaced by
  `cargo xtask widths`.
- [Decisions](#decisions) 1, 2, 3 and 5. The agreed crates are pinned, with their features, in
  `[workspace.dependencies]`; they build together on Rust 1.88, the new minimum, which also brings let chains.
- `cargo deny` checks licences, advisories, sources and banned crates (`deny.toml`), in CI.
- Package B of PORT-ROADMAP.md is merged into this branch, with its VM results.

Open in R0:

- The two pilot slices, which vendor the first agreed crates as they use them.
- `xtask bless`, with the first `insta` snapshot test; `cargo nextest`.
- The strict-lint list (`xtask/strict/crates.txt`, starting with `xtask`) and coverage in the metrics baseline
  ([How the rules are enforced](#how-the-rules-are-enforced)).
- Decisions 4 and 6 belong to later phases (R2 and R9).

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
confined to `zbm-sys` (13 blocks), every system call behind a trait with a fake. It was, however, written to *be* the
legacy code in another language, and that shows.

## Findings

Each finding names the goals it touches and an example; the phases below fix them.

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

### F2. Errors and outcomes are invisible (G5)

- 50 functions outside tests return a `bool` that only says whether they succeeded (`unlock_all`, `export_pool`,
  `cryptsetup_open`, the `try_*` key methods). A failed unlock, import or mount says "no" and logs a line; the cause
  is gone by the time a caller decides what to do.
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

- 71 Rust tests run the legacy Bash or Perl code to compare results. They need `bash`, the legacy tree and Perl
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
| `zbm-tui` | `zbm-menu`, `lib/zfsbootmenu-ui.sh`, `fzf` | The menu: Elm architecture over `zbm-app`, `nucleo-matcher` for matching, Ratatui widgets per screen |
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
- `nucleo-matcher`, the matcher of Nucleo without its thread pool, scores the filter text against the list of the
  current screen inside `update`, so matching is as pure and testable as the rest of `update`; the list widget
  renders the highlighted positions. The lists of a boot menu hold tens to hundreds of entries, which this matches in
  well under a frame. `nucleo` itself adds `rayon` and `parking_lot` to run matching on a thread pool, built for
  editors that search millions of files; the menu does not take it unless the R0 pilot measures a need.
- Key bindings are a table (`keymap.rs`) that the help screen renders, so documentation and behaviour cannot drift.
- Tests: `update` with table-driven cases; `view` with `TestBackend` and `insta` snapshots at 80×25, 120×40 and
  52×20; one end-to-end test per screen through `zbm-app` with fakes.

### Options defined once

Today each kernel command line option, configuration key and crypttab option is defined three times: a parser in
`zbm-cmdline` or `zbm-luks`, a section in a hand-written man page (`docs/man/zfsbootmenu.7.rst`,
`zfsbootmenu-crypttab.5.rst`, `generate-zbm.5.rst`) and, for the menu, a help file rendered from RST. They drift.

After the refactor, `zbm-config` holds one table of option specifications, a `const` slice of `OptionSpec` values:
name, type (a `zbm-types` type with `FromStr`), default, accepted values, the release it appeared in, deprecation,
a one-line summary, the full description and examples. From that table:

- the parsers are driven by the table, so an option without a specification cannot be parsed;
- `xtask gen-docs` writes the option sections of the man pages as RST fragments that the pages include, and the
  option reference in `docs/`;
- `zbm debug config` prints each value with its specification and where it came from;
- deprecated names produce a warning naming the replacement;
- a table-driven test parses every example of every specification and checks that it round-trips through `Display`.

The menu's key bindings work the same way: `zbm-tui`'s `keymap.rs` table drives the input handling, the help screen
and a generated section of the man page. CI runs `xtask gen-docs --check`, which fails when a generated fragment is
out of date.

### Crash reports

The runtime is built with `strip = true`, so a panic on a user's machine produces a backtrace of bare addresses that
nobody can read. After the refactor:

- **Release builds keep their debug information outside the image.** They are built with debug information and a
  GNU build ID; `xtask build-image` splits it into a separate file per binary (`objcopy --only-keep-debug`), strips
  the binary in the image and links the two by build ID. The debug files are published with each release and kept
  as CI artifacts for test images, so the image size does not change.
- **A panic hook in `zbm-trace`** writes a crash report to the journal and to `/zfsbootmenu/crash-<time>.txt`: the
  panic message (secrets cannot appear, since `Secret` has a redacted `Debug`), the location, the build ID, the
  open `tracing` spans (stage, volume, pool) and the backtrace as offsets from the binary's load address, read from
  `/proc/self/maps`, so that address space randomisation does not matter. The supervisor records a child killed by
  a signal the same way, then opens the recovery shell with the path of the report.
- **`zbm debug crash`** shows the latest report on the machine; **`xtask symbolize <report>`** finds the debug file
  by build ID and prints function names, files and lines, using the `addr2line` crate.

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
9. **Dependencies:** vendored, each with a written reason in `vendor/README.md`, checked with `cargo deny` (licences,
   advisories, duplicates).
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
| Strict lints stay clean once reached | `xtask lint` with `xtask/strict/crates.txt` | The crates listed there (from R0: `xtask`) must have no finding of `cargo xtask lint --strict`, and `lint` fails otherwise. Each phase adds the crates it finishes, so a refactored crate cannot slip back while the others are still being worked on |
| Test coverage only goes up | `xtask metrics` with `cargo llvm-cov` | Line coverage per crate becomes a metric of the baseline, with the opposite direction: `--check` fails when a crate's coverage drops below its baseline (to the whole percent), and `--bless` raises it. A new crate starts at 80% |
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
- **Out of scope,** stated as such: for example, a compromised firmware, or an attacker who can already run code as
  root before ZFSBootMenu starts.

Each fuzz target of the [testing strategy](#testing-strategy) names the threat it covers. Before R9 removes the
legacy runtime, an independent security review of the Rust runtime against this model is an exit condition.

## Phases

Each phase ends with every suite green on its own branch merge, the measures of its goals met for the code it
touched, and its docs written. Phases R1 to R4 change structure without changing behaviour (the golden files and VM
tests are the proof); behaviour changes start with R5.

### R0. Decisions, scaffolding and pilot slices (3 to 5 weeks)

- Decide the [open questions](#decisions): dependency set, MSRV, menu replacing `fzf` outright, when the
  legacy runtime goes.
- Add `xtask` with `lint`, `test`, `doc`, `bless`, `check-deps` and `metrics`; add `cargo deny`, `cargo nextest`, and
  the lints of [How the rules are enforced](#how-the-rules-are-enforced) as warnings. Record the starting metrics.
- Vendor the agreed crates; write `vendor/README.md`.
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

- `zbm-luks`: replace `Session` by `Unlocker` + `KeyMethod` strategies + `TokenHandler` registry; the passphrase
  prompt becomes a state machine that asks a `Prompter` port; mapping state behind a small repository type.
- `zbm-pool`: import policies as an enum with methods; the retry loop as a state machine whose transitions are unit
  tested; host ID handling as a type.
- `zbm-startup`: the start-up program as `Vec<Box<dyn Step>>` with `Stage` from `zbm-types`; hooks as a `Step`.
- `zbm-config`: one `Config` from command line, configuration file and crypttab, with the origin of each value for
  `zbm debug config`, built on the [option specification table](#options-defined-once); `xtask gen-docs` generates
  the option sections of `zfsbootmenu(7)`, `zfsbootmenu-crypttab(5)` and `generate-zbm(5)`, and CI checks them.
- Split `import.rs`, `boot.rs`, `generate.rs` along these lines; no module over 600 lines.
- **Port the VM test harness to Rust** (`zbm-vmtest`, alongside R3 and R4): the QEMU driver, a serial console with
  `expect`-style matching, swtpm and USB hot-plug, disk builders, and the existing LUKS scenarios. This has to be done
  before R5: the menu's VM tests are new, and written once, in Rust, instead of in Python and again in R7. Python and
  Rust harnesses run the same scenarios until their results match, then the Python harness is removed.
- **Exit:** the size and type rules hold in these crates (checked by `xtask metrics`); golden files unchanged; the VM
  tests pass in the Rust harness with zero shadow differences; every option in the man pages comes from the table.

### R4. Application layer (3 to 4 weeks)

- Create `zbm-app`: the `Snapshot` of the world, `Action`, events, and the use cases that the start-up program and
  the applets already need (boot the default environment, unlock, close, import).
- Rewrite the applets (`zluks`, `zfsbootmenu-init`, the supervisor glue) as thin front ends over `zbm-app`.
- Add `zbm debug` (`config`, `state`, `journal`, `commands`, `luks`, `pools`, `crash`), replacing `ztrace` and
  `zlogtail`.
- Shadow mode covers the use cases of `zbm-app`; ask users to run the shadow images and report differences before R5
  starts changing behaviour.
- **Exit:** no front end imports an adapter crate; `zbm debug` documented in `zbm-debug(8)`; zero shadow differences
  in the VM tests and no open shadow difference reported by users.

### R5. The new menu (6 to 8 weeks)

- `zbm-tui` with Ratatui and Nucleo, screen by screen, in the order users meet them: boot environments, kernels,
  snapshots and their actions, diff, pool status, help, command line editor, LUKS volumes (new: unlock from the menu
  instead of the recovery shell), power menu.
- Port package C (boot environments, kernels, native encryption keys, key sources, booting) into `zbm-bootenv` and
  `zbm-kexec` as the boot environment screen needs it, and package D (snapshots, rollback, clone, set default) into
  `zbm-app` use cases.
- Decide and document every behaviour change against the `fzf` menu in `CHANGES-FROM-LEGACY.md` (key bindings,
  layout, previews). The key binding section of the man page is generated from `keymap.rs`.
- **Exit:** the VM tests that drive the menu are written for the new screens in `zbm-vmtest` and pass; snapshot
  tests cover every screen at three sizes; the menu runs without `fzf` in images, including on a serial console.

### R6. Recovery tools and image assembly in Rust (3 to 4 weeks)

- Port package F (recovery shell commands `zfs-chroot`, `zsnapshots`, `zkexec`, `mount_esp`, `reboot`, `firmware-setup`,
  `logs`, help viewer) as `zbm` applets over `zbm-app`.
- Built-in hooks (`20-dracut-console-init`, `30-console-autosize`, `80-luks-close`, `90-xhci-unbind`) become Rust
  steps; user hooks stay shell scripts, run by `zbm-startup`.
- Image assembly logic moves from `install-helpers.sh` into `zbm-generate` (a library computing the file list); the
  dracut `module-setup.sh` and the mkinitcpio install hook shrink to shims that call it.
- Releases publish the debug files of every binary next to the images, indexed by build ID, so `xtask symbolize`
  can find them for any released version.
- **Exit:** the only shell scripts left in the runtime are the two packaging shims and the user hook examples.

### R7. Remaining tests and tooling in Rust (3 to 4 weeks, can run alongside R5 and R6)

- The LUKS device suites moved in R2 and the VM harness in R3 and R4. What is left: the upstream test VMs
  (`tests/setup.sh`, `tests/run.sh`) as non-interactive `zbm-vmtest` scenarios.
- Replace `scripts/` and `packaging/container/*.sh` with `xtask` subcommands (`build-image`, `record-fixtures`,
  `release`, `compare-images`); the container's entry point becomes a Rust program.
- **Exit:** no Bash, Perl or Python under `tests/`, `scripts/` or `packaging/container/`; CI runs device and VM tests
  through `xtask`.

### R8. Developer documentation (2 to 3 weeks, finishing alongside R7)

- `docs/dev/`: the boot flow from `zbm` start to `kexec`, stage by stage, with links into rustdoc; "how to add a key
  method", "how to add a menu screen", "how to add a boot step", "how to debug a failing boot" (journal, `zbm debug`,
  QEMU serial logs), "how the tests are organised".
- Rustdoc landing pages per crate with the architecture position; `cargo doc --workspace` published by CI.
- **Exit:** a developer who has not worked on the code base adds a key method and a menu screen using only the
  documentation.

### R9. Cutover and removal (with PORT-ROADMAP phases 7 and 8)

- The Rust runtime becomes the default after a beta in which the legacy image is the fallback.
- Delete `legacy/`, the golden files recorded from it that no longer guard anything, and the compatibility shims.
- An independent security review of the Rust runtime against the [threat model](#threat-model); its findings are
  fixed or accepted in writing before the legacy runtime is removed.
- **Exit:** the tree contains Rust, the packaging shims, documentation and user hook examples only; the security
  review is done.

### Order and overlap

```text
R0 ─ R1 ─ R2 ─ R3 ─ R4 ─ R5 ─ R6 ─ R9
          │    └────┤    └────┤
          │         │         └ R7: upstream test VMs and tooling (alongside R5 and R6)
          │         └ VM harness in Rust (alongside R3 and R4, done before R5)
          └ LUKS device suites in Rust (done before R3)

R0 includes the pilot slices; R8 (docs) runs alongside R7 and ends before R9.
```

Two test ports are tied to the code they protect: the LUKS device suites move to Rust in R2, before R3 rewrites
`zbm-luks`, and the VM harness moves during R3 and R4, before R5 adds the menu's VM tests. Package B's VM tests, now
running against the Rust image, are the regression check from R1 on.

## Relation to PORT-ROADMAP.md

- Packages A and B are done (B's complex-pool VM tests wait for the boot environment code of R5). They are
  refactored in R1 to R4, not ported again.
- Packages C, D, E and F are no longer ported in the current architecture: C and D land in R5 and E is replaced by
  the Ratatui menu in R5, F in R6. This avoids writing the menu twice (once around `fzf`, once natively).
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
   gets its own small expect module in R3. `uucore` needs its default `signals` feature.
2. **MSRV:** 1.88, what `ratatui` 0.30, `uucore` 0.12 and `proptest` 1.12 need, checked in CI with `cargo +1.88`; it
   rises only when an agreed crate needs it. The workspace now uses let chains, which 1.88 brought.
3. **Menu:** the Nucleo and Ratatui menu replaces `fzf` outright in R5; `fzf` leaves the images then.
5. **Errors:** `thiserror`, which only runs at build time and adds nothing to the binaries.

Still open:

4. **uutils in `zbm`:** link `uu_*` applets into `zbm` (one binary, BusyBox style) or ship the uutils multi-call
   binary next to it; decided by the R2 spike.
6. **When the legacy runtime goes:** keep the PORT-ROADMAP beta (phase 7) with the legacy image as fallback, or cut
   over directly after R6; decided before R9.

## Proposals for later

Improvements that were suggested during review and kept for a later revision of this roadmap. None of them is part of
the plan yet; each would be added to a phase when it is adopted.

1. **Trustworthy fakes:** contract tests that run one suite against both `FakeZfs` and `ZfsCli` (in a VM), and the same
   for the `zbm-crypt` adapters; mutation testing with `cargo-mutants` to measure whether tests catch faults; state
   machine property tests (`proptest-state-machine`) for the import retry loop and the unlocker against a reference
   model.
2. **Debugging failures offline:** `zbm debug bundle` writes a redacted archive of the journal, the commands run, the
   configuration and the pool layout; `xtask replay <bundle>` turns it into a fake machine, so a reported boot
   failure becomes a `cargo test`; boot timelines exported as Chrome trace files for Perfetto.
3. **Developer experience:** a demo mode for the menu (`cargo run -p zbm-tui -- --demo`) on a fake machine;
   Architecture Decision Records in `docs/dev/adr/`; runnable `examples/` per crate; an `AGENTS.md` per crate with its
   invariants, commands and test conventions for AI tools.
4. **Stable error codes:** `ZBM-E0412: no key method could open /dev/sda2`, explained by `zbm explain <code>` and one
   documentation page per code.
5. **Boot environment specifics:** terminal capability detection with ASCII borders, 16 colours and `NO_COLOR`;
   budgets for boot time, image size and memory with CI gates; `zeroize` or `secrecy` instead of the hand-written
   `Secret`, `mlock` for secret memory and no core dumps; `cargo vet` and `cargo auditable` for the supply chain.
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
    redraw time over a 115200-baud serial console.
16. **An upstream sync policy:** watch upstream ZFSBootMenu releases; for each relevant fix, write a test first, port
    it, and log it as `legacy/RUNTIME-CHANGES.md` does today.
17. **A reproducibility check:** build the images twice in different directories and compare them, with build paths
    removed from the binaries (`--remap-path-prefix`), keeping the identical-image guarantee of PORT-ROADMAP phase 3.
18. **Metrics for rules 2 and 4:** debt columns for public functions of domain crates that take a raw string, and for
    public items without a doc example, ratcheted like the others.
