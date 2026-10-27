# Refactoring Roadmap

This roadmap turns the ZFSBootMenu code base into an idiomatic Rust project that developers can read, change, debug
and test, with a new menu built on [Nucleo] and [Ratatui]. It started in October 2026, during Phase 6 of
[PORT-ROADMAP.md](PORT-ROADMAP.md), and changed that plan: fundamental changes to structure and behaviour are allowed,
and every line of code and every test ends up in Rust.

It is one of three plans. [ROADMAP.md](ROADMAP.md) lists the features for users (numbered #1 to #21), and
[docs/dev/console-roadmap.md](docs/dev/console-roadmap.md) the milestones of the console on the display (M1 to M9).
This file is about the code: its structure, its rules and its tests.

The file starts with what is left to do. The plan for the code comes next, and the record of finished work is at the
end.

[Nucleo]: https://github.com/helix-editor/nucleo
[Ratatui]: https://ratatui.rs

## Contents

- [What is left](#what-is-left): [now](#now), [next](#next), [later](#later), [carried over](#carried-over)
- [Where the code stands](#where-the-code-stands)
- [Goals](#goals)
- [Target architecture](#target-architecture), with [the menu redesign](#the-menu-redesign)
- [Rules for refactored code](#rules-for-refactored-code) and [how the rules are
  enforced](#how-the-rules-are-enforced)
- [Testing strategy](#testing-strategy), with [shadow mode](#shadow-mode-on-real-machines) and
  [benchmarks](#benchmarks)
- [Using uutils coreutils](#using-uutils-coreutils), [the ZFS boundary](#the-zfs-boundary), [the
  shell](#the-shell)
- [Threat model](#threat-model)
- [Phases still to do](#phases-still-to-do)
- [Relation to PORT-ROADMAP.md](#relation-to-port-roadmapmd), [risks](#risks), [decisions](#decisions),
  [proposals for later](#proposals-for-later)
- [Record of finished work](#record-of-finished-work)

## What is left

On branch `claude/zbm-refactor`, at the end of October 2026:

| Phase | State | What is left |
| --- | --- | --- |
| R0 to R4 | Done | Only the last part of R4's exit: shadow mode on testers' machines |
| D. Menu design | Nearly done | The usability sessions, the review of the design, decision 8 |
| R5. The new menu | Steps 1 to 4 of 7 done, step 5 under way | The rest of step 5, then steps 6 and 7 |
| R6. Recovery tools and image assembly | Not started | Two built-in hooks were done early |
| R7. Remaining tests and tooling | Not started | The upstream test VMs, the scripts |
| R8. Developer documentation | Not started | The how-to pages |
| R9. Cutover and removal | Not started | Decision 6, the security review |

### Now

1. **brush in every Rust VM test.** The Rust runtime now runs its shell work on brush ([The shell](#the-shell)):
   the recovery shell, hooks written for Bash or `sh`, SSH logins and its own short scripts. Every VM test of the
   Rust images must pass again with images built from the new code. If a test fails because of brush, fix brush (in
   `patches/`) or the test; `zbm.shell=bash` is not a way to pass a test.
2. **R5 step 5, snapshots** ([plan](#r5-the-new-menu)): VM tests of each change made through the menu, the previews
   of a diff and of package changes, and the command palette's entries for the changes.
3. **Offer the brush parser fix upstream** (`patches/README.md`), so that the patch can go when a release has it.

### Next

1. **R5 step 6**, the rest of the menu: kernels and the command line editor, pools and LUKS volumes, recovery tools,
   help, power.
2. **R5 step 7**, the exit of R5: the generated key binding section, the 10,000-snapshot performance test and the
   menu's [benchmarks](#benchmarks), the hostile-names fuzz target, the usability sessions.
3. **The exit of D**, alongside: the usability sessions on the prototype (at least five people, one on a serial
   console), which the maintainer runs; the review of the design documents; decision 8.
4. **A first [benchmark](#benchmarks) baseline**: `cargo xtask bench` and the `bench-*` VM scenarios, for the legacy
   runtime and the Rust runtime of today, on the maintainer's machine.
5. **Shadow mode on testers' machines**, the last part of R4's exit: no open shadow difference reported by users.
6. **The console on the display**, from M4 of the [console roadmap](docs/dev/console-roadmap.md) (M3, a comfortable
   recovery shell, is done and waits for its VM run), and the group 1 features of [ROADMAP.md](ROADMAP.md) (#17, the
   graphics drivers, is partly done).

### Later

1. **R6**: the recovery tools as menu views and `zbm` applets; the built-in hooks in Rust; image assembly in
   `generate-zbm`, with the dracut and mkinitcpio scripts as thin shims.
2. **R7**, alongside R6: the upstream test VMs in `zbm-vmtest`; `scripts/` and the build container's scripts as
   `xtask` commands.
3. **R8**: the developer guides.
4. **R9**: the beta, the security review, and the removal of the legacy runtime.
5. The features of groups 2 and 3 of [ROADMAP.md](ROADMAP.md), many of them already prototyped in D.

The [phases still to do](#phases-still-to-do) give each one in full.

### Carried over

What the finished phases left open, and where it goes:

| Item | From | To |
| --- | --- | --- |
| The menu timeout is the text of `/etc/zfsbootmenu.conf` (`+5`); it becomes a type when the menu counts it down | R1 | R5 |
| The start-up's pool import and `zbm-boot` run `zfs` and `zpool` with the legacy command lines rather than the typed port, so that their parity recordings compare | R2, R3 | R5, where behaviour may change, or R9, when parity ends |
| Fixtures of OpenZFS 2.2 and 2.3 (the `oldzfs` VM tests already run with an OpenZFS 2.2 image) | R2 | When fixtures are recorded from such images ([proposal](#proposals-for-later) 10) |
| Warnings for deprecated option names, a change of behaviour | R3 | R5 |
| The build options are read by the shell scripts of the dracut module and the mkinitcpio hook | R3 | R6 |
| Line coverage below the 80% of new crates: `xtask` 78%, `zbm-vmtest` 62% (its scenarios run only in VMs) | R0, R3 | Ratcheted; never lowered |
| `zbm-menu`, an empty crate | R0 | R5, which builds the menu in `zbm-tui` |

## Where the code stands

| Part | Language | Size | State |
| --- | --- | --- | --- |
| Host tools: `generate-zbm`, `zbm-kcl` | Rust | 10,700 lines | Done |
| Foundation and types: `zbm-types`, `zbm-core`, `zbm-trace`, `zbm-sys` | Rust | 6,900 lines | Done (R1, R2) |
| Platform: `zbm-platform`, `zbm-coreutils`, `zbm-display` | Rust | 8,800 lines | Done (R2); `zbm-display` for ROADMAP.md #19 |
| State and configuration: `zbm-config`, `zbm-state` | Rust | 7,900 lines | Done (R3) |
| Adapters: `zbm-zfs`, `zbm-crypt`, `zbm-tools` | Rust | 6,800 lines | Done (R2) |
| Domain: `zbm-luks`, `zbm-pool`, `zbm-boot` | Rust | 23,500 lines | Done (R3); `zbm-boot` splits in R5 |
| Application: `zbm-app` | Rust | 4,700 lines | Done (R4); grows with each R5 step |
| Front ends: `zbm`, `zbm-startup`, `zbm-tui`, `zbm-console`, `zbm-brush` | Rust | 31,100 lines | R5 builds the menu |
| Test kit and developer tools: `zbm-testkit`, `xtask`, `zbm-vmtest` | Rust | 28,200 lines | Done (R1 to R3) |
| Legacy runtime (`legacy/zfsbootmenu`) | Bash | 9,200 lines | Frozen; the default runtime until R9 |
| Legacy host tools (`legacy/bin`) | Perl, Bash | 1,800 lines | Replaced; kept to record parity results until R9 |
| LUKS unit suites (`tests/luks/unit`) | Bash | 1,300 lines | Kept while the legacy runtime is; the Rust port is `crates/zbm-luks/tests/devices` |
| Upstream test VMs (`tests/*.sh`, `tests/helpers`) | Bash, Perl | 2,800 lines | Interactive; R7 |
| Packaging and scripts (`packaging/`, `scripts/`) | Bash, Python | 2,300 lines | R6, R7 |

The Rust code is 128,700 lines in 27 crates. `cargo xtask test` runs about 1,630 tests without root; seven fuzz
targets cover the parsers of untrusted input; `xtask metrics` counts no long function, no long file, no success flag
and no legacy-named doc line, 24 uses of the shell helpers (in configuration parsing, where they are allowed) and 29
`unsafe` blocks, all in `zbm-sys`. The VM tests of the Rust images passed 900 checks on 25 to 28 October 2026
(`docs/luks/testing.rst`).

## Goals

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

## Target architecture

Ports and adapters (hexagonal architecture): the domain and the application layer know nothing about processes,
terminals or files; adapters implement narrow traits; front ends drive the application layer.
`docs/dev/architecture.md`, generated from the layer table of `xtask check-deps`, lists every crate and the edges
allowed between them.

```text
                 ┌────────────────────┐  ┌──────────────────┐  ┌──────────────────┐
  front ends     │ zbm-tui            │  │ zbm (applets),   │  │ zbm-startup,     │
                 │ Ratatui + Nucleo   │  │ zluks, zbm-brush │  │ zbm-console      │
                 │ (Elm architecture) │  │                  │  │                  │
                 └─────────┬──────────┘  └────────┬─────────┘  └────────┬─────────┘
                           └──────────────┬───────┴─────────────────────┘
  application              ┌──────────────▼──────────────┐
                           │ zbm-app: use cases, events, │
                           │ snapshot, Action enum       │
                           └──────────────┬──────────────┘
  domain services   ┌──────────────┬──────┴───────┬──────────────┐
                    │ zbm-luks     │ zbm-pool     │ zbm-boot     │
                    │ KeyMethod    │ import       │ environments,│
                    │ strategies   │ policies     │ kernels, kexec│
                    └──────┬───────┴──────┬───────┴──────┬───────┘
  domain types             └──────── zbm-types ──────────┘
                                   (newtypes, enums, no IO)
  ports (traits)    zbm-platform: Runner, Files, Clock, Console, Processes, Environment   zbm-trace
  adapters          zbm-zfs (CLI, JSON)  zbm-crypt (cryptsetup, tpm2, fido2, gpg, ykchalresp)  zbm-tools
                    zbm-coreutils (uutils)  zbm-display (DRM, input)  zbm-sys (the only unsafe)
```

### Crates after the refactor

Most crates exist and are listed in `docs/dev/architecture.md`. The changes still planned:

| Crate | Change | When |
| --- | --- | --- |
| `zbm-bootenv`, `zbm-kexec` | Boot environments, kernels, command lines and key sources, and loading kernels with teardown, split out of `zbm-boot` once the menu's use cases show where the line falls | R5 |
| `zbm-menu` | Removed: the menu is `zbm-tui` | R5 |
| `generate-zbm` | Computes the file list of an image, which the dracut and mkinitcpio shims call | R6 |
| `xtask` | `build-image`, `record-fixtures`, `release`, and the build container's entry point | R7 |
| `zbm-vmtest` | The upstream test VMs as scenarios | R7 |
| `zbm-sys` | `kexec_file_load`, with `zbm-kexec` | R5 |

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
| Observer | Events from `zbm-app` (`UnlockFinished`, `StageStarted`, `Booting`) | The menu, the start-up program and logs react without coupling |
| RAII guards | Mounted key devices, GnuPG homes, TPM sessions, temporary directories | Clean-up happens on every path, including `?` |
| Dependency injection through a context | `System` and `Context` passed by reference | No globals; fakes for every test |

### The menu on Nucleo and Ratatui

- `zbm-app` exposes a `Snapshot` of the world (pools, boot environments, kernels, snapshots, LUKS volumes, health)
  and accepts `Action`s; it is the only thing the menu talks to.
- `zbm-tui` keeps a `Model`, turns terminal events into `Message`s, and has one pure
  `update(model, message) -> (Model, Vec<Action>)`. Each view is a module with a `view` function and its own messages.
- `nucleo-matcher`, the matcher of Nucleo without its thread pool, scores the filter text inside `update`, so
  matching is as pure and testable as the rest of `update`. The lists of a boot menu hold tens to hundreds of entries,
  which this matches in well under a frame; R5's performance test with 10,000 snapshots decides whether the command
  palette still does (decision 7).
- Key bindings are a table (`keymap.rs`) that the help screen renders, so documentation and behaviour cannot drift.
- Tests: `update` with table-driven cases; `view` with `TestBackend` and `insta` snapshots at 80×25, 120×40 and
  52×20; one end-to-end test per screen through `zbm-app` with fakes.

### The menu redesign

The `fzf` menu grew screen by screen around one tool. The new menu is designed from the tasks users come to do, with
these principles, each with the way it is applied and checked. The design itself is in `docs/dev/menu-design/`.

| Principle | Applied as | Checked by |
| --- | --- | --- |
| Task first | The core tasks (boot the default, boot another environment or kernel, roll back or boot a snapshot, unlock volumes, edit the command line once, recover a failed boot) shape the layout; booting the default needs no key | Keystrokes per core task, measured on the prototype and in R5 |
| One place, details on demand | One main view: the boot environment list with a detail pane; other views open from it and return to it | Every core task starts from the main view |
| Visible state and feedback | A status bar shows the countdown, pools, LUKS volumes and health; every action reports its outcome without blocking | Snapshot tests of each state; no silent action |
| Keyboard first and discoverable | One key map for every view (`Enter` primary action, `Esc` back, `/` filter, `?` help, `:` command palette); a footer shows the keys of the focused element | The key map is a table (`keymap.rs`) that drives input, footer, help and the man page |
| Safe by default | Rollback, destroy and promote show what will be lost and need a typed confirmation | A test per destructive action that it cannot run without confirmation |
| Errors that say what to do | Every error view names what failed, why, and the next steps offered (retry, other method, shell) | Each error variant reachable from the menu has a view test |
| Works on every console | 52×20 at least; 16 colours, monochrome and `NO_COLOR`; ASCII borders when the terminal cannot draw lines; meaning never carried by colour alone | Snapshot tests at 80×25, 120×40 and 52×20, in colour, monochrome and ASCII |
| Fast | The first frame shortly after the menu starts; only changed cells are redrawn | Redraw budget over 115200 baud; 10,000 snapshots in the performance test |
| The whole boot in one interface | Start-up progress, the countdown, unlock prompts and pool import problems are views of the menu; a plain console when the terminal cannot run it | VM tests drive unlock and import problems through the menu |
| Secrets stay secret | Secret fields never echo, never enter the model as text, and are wiped after use | `assert_no_secret_leaked` over the model, the journal and the screen |

The views: the start-up view (stages, unlock dialogs, import problems); the main view (environments and the detail
pane, the countdown in the status bar); the command palette; and, opened from the main view, snapshots, kernels, the
command line editor, pools and LUKS volumes, recovery tools and help. Behaviour changes against the `fzf` menu are in
`CHANGES-FROM-LEGACY.md`, written as a migration guide.

### Options defined once

`zbm-config` holds one table of option specifications (`zbm_config::spec`): name, aliases, where it is set, its
values, default, summary, what reads it, its manual page entry, deprecation and examples. The parsers look options up
by their specification; `xtask gen-docs` writes the option sections of the man pages, and `cargo xtask lint` checks
them; `zbm debug config` prints each value with where it came from; a test reads every example. The menu's key
bindings work the same way, from `keymap.rs`.

### Crash reports

Release builds keep line tables and a build ID, and image builds move the debug information out of the image
(`cargo xtask split-debug`). A panic hook writes `/zfsbootmenu/crash-<time>-<pid>.txt` with the message, the open
spans and the stack as offsets; the supervisor does the same for a child killed by a signal. `zbm debug crash` shows a
report, and `cargo xtask symbolize` turns it into function names and lines. The VM test `runtime-rust-crash` checks
the whole chain. Left for R6: releases publish the debug files.

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
| Dependencies (9) | `cargo deny check` | Licences, advisories, duplicate versions and banned crates (`libzfs` bindings among them, for G2) |
| Strict lints stay clean once reached | `xtask lint` with `xtask/strict/crates.txt` | The crates listed there must have no strict finding; each phase adds the crates it finishes |
| Test coverage only goes up | `xtask coverage` with `cargo llvm-cov` | `--check` fails when a crate's line coverage drops below `xtask/coverage-baseline.txt`; a new crate must reach 80% |
| Documentation matches behaviour | `xtask gen-docs --check` | Fails when a man page fragment generated from the option or key binding tables is out of date |

## Testing strategy

### Layers

| Layer | What | Where | Runs | Root |
| --- | --- | --- | --- | --- |
| Unit | Pure functions, parsers, `update`, state machines | next to the code | every `cargo test` | no |
| Component | One service with `zbm-testkit` fakes (`FakeMachine`) | `crates/*/tests/` | every `cargo test` | no |
| Golden | Output compared with recorded files (screens, generated configs, legacy results) | `tests/golden/`, `crates/*/tests/` | every `cargo test` | no |
| Property | Parsers and orderings with `proptest` | next to the code | every `cargo test` | no |
| Fuzz | Every parser of untrusted input in the [threat model](#threat-model) | `fuzz/` | `cargo xtask fuzz`, nightly CI | no |
| Device | Real `cryptsetup`, swtpm, GnuPG on loop devices | `crates/zbm-luks/tests/devices/` | `xtask test devices` and CI | yes, in a container |
| System | Images booted in QEMU (`zbm-vmtest`) | `crates/zbm-vmtest` | `xtask test vm`, by the maintainer with KVM | host with KVM |

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

### Shadow mode on real machines

Shadow mode checks the Rust code on machines that no test describes, without letting it act
([docs/dev/shadow-mode.md](docs/dev/shadow-mode.md)):

- **What it does.** A legacy image built with `zfsbootmenu_shadow=yes` runs `zbm shadow` from a `boot-sel.d` hook,
  after the legacy runtime has chosen what to boot and before it loads the kernel. It works out what the Rust code
  would have decided, and compares: the host ID, the pools imported and how, the LUKS mapping names, the default boot
  environment, the kernel and initramfs, and the key sources of encryption roots.
- **It cannot act.** It runs on `ReadOnlyRunner`, an allowlist of read-only commands; it never prompts or reads a
  secret, gives up after five seconds, and its exit status is ignored.
- **Where the result goes.** `ZBM-SHADOW: <n> differences` on the console and in the kernel log; with
  `zbm.shadow=efivar`, a summary in an EFI variable that `zbm shadow report` reads on the booted system.
- **In tests.** The VM harness builds every legacy image with shadow mode, and a difference fails the test.

### Replacing the Bash-based parity tests

Parity with the legacy code stays the safety net until the cutover, without running Bash at test time:
`xtask record-parity` runs each legacy function and writes its results as golden files
(`tests/golden/legacy/<crate>/<module>/<test>.json`); the Rust tests compare against them; a deliberate change updates
the golden file and is listed in `CHANGES-FROM-LEGACY.md`.

### Benchmarks

The Rust runtime should be faster where it matters and no slower anywhere. These benchmarks turn that into numbers,
measured against the legacy runtime on the same machine. Until they run, no speed claim is made.

| # | What | How it is measured | Compared with | Budget |
| --- | --- | --- | --- | --- |
| B1 | Image size: the EFI image and the initramfs, and their parts (`zbm`, `brush`, fonts, keyboard layouts, uutils) | `generate-zbm` builds both images from one configuration; `cargo xtask bench size` lists the parts | The legacy image | Set from the first baseline; a rise of more than 10% needs a reason in the record |
| B2 | Time to the menu: from the kernel's start to the countdown's first frame | Kernel log timestamps and the journal's spans, in a VM with KVM | The legacy countdown; the kernel console and `zbm console` | No slower than legacy |
| B3 | Time to boot: from QEMU's start to `BE-INIT-START`, with the stage times from the journal | The countdown boot of a `runtime` test, five runs, median and spread | The legacy image | No slower than legacy |
| B4 | Key to screen: moving the selection, typing a filter, opening the snapshots and a dialog | In process on the fake machine, and end to end over the serial console in a VM | `fzf` and its previews | A frame (16 ms) in process; legacy's median end to end |
| B5 | Many snapshots: 100, 1,000 and 10,000 of one environment; opening, filtering, scrolling, memory | A pool built by the VM test, and in process with `FakeZfs`; R5's performance test | The legacy menu | Matching and redraw within a frame at 10,000 |
| B6 | Bytes per redraw over a serial console, for the actions of B4 | The bytes the serial console receives in a VM, at 80×24 | The legacy menu | Each action's redraw within 100 ms at 115200 baud (1,150 bytes) |
| B7 | Processes started: during start-up, per menu action and per boot | The command journal and `/proc` counts in a VM | The legacy runtime, from `bash -x` traces | Fewer than legacy for every action; none for moving the selection |
| B8 | Memory: the peak resident memory of start-up, the menu, `zbm console` and the recovery shell | `/proc/<pid>/status` sampled in a VM | `bash` and `fzf` in the legacy image | Set from the first baseline |
| B9 | The console on the display: time to its first frame, `cat` of a 10 MB file, and a scrolling flood | VM tests with the console's mirror and timestamps (console roadmap C112) | The kernel console | No slower than the kernel console once M5 is done |
| B10 | Unlocking: from Enter to the volume open, without the key derivation | The `secrets` and `keys` VM tests' journal spans | The legacy runtime | The overhead no slower than legacy |
| B11 | Scale: start-up with 4 pools, 24 devices and 12 LUKS volumes | A `complex` VM layout grown for the benchmark | The legacy runtime | No slower than legacy |
| B12 | Building: an image with `generate-zbm`, and `cargo xtask test` | On the build machine and in the build container | The Perl `generate-zbm`; G8 | No slower than the Perl tool; `cargo xtask test` under a minute |

`cargo xtask bench` runs what needs no VM (B1, B4 and B5 in process, B12) and writes `target/bench.json`;
`zbm-vmtest` gets `bench-*` scenarios for the rest. Counts (bytes, processes, sizes) are checked in CI against
budgets; times are recorded with their median and spread over five runs, compared only on the same machine, in a
Benchmarks table of `docs/luks/testing.rst`. They run for a baseline, then at the exits of R5, R6 and R9's beta, and
for B9 at M5 and M9 of the console roadmap.

## Using uutils coreutils

The build container's images already contain uutils (Ubuntu 26.10 made it the default). The refactor makes it the
coreutils of ZFSBootMenu everywhere:

1. **In images:** the dracut and mkinitcpio modules install the uutils multi-call `coreutils` binary, with a hard link
   per program, when the host has it (`coreutils --list` works), and the host's programs otherwise.
2. **In Rust code:** `zbm-coreutils` copies, restricts and removes file trees in process through the `Files` port, and
   version order is `uucore::version_cmp` (`zbm_types::version`). A separate program, when one is needed, is the
   uutils binary, so that tests and images agree.
3. **Not in `zbm` itself** (decision 4): linking the `uu_*` applets into `zbm` works, but adds 9.5 MB and 293 crates,
   and a start-up hook into the binary that runs as process 1. Images ship the uutils multi-call binary next to `zbm`.
4. **In tests:** expectations for ordering, paths and formatting come from uutils behaviour.

uutils crates follow a recent MSRV, are vendored and pinned, and are upgraded deliberately with the golden files as
the check.

## The ZFS boundary

- **No libzfs.** No FFI to `libzfs`, `libzfs_core` or `libnvpair`, now or later: their ABI changes between OpenZFS
  releases, and the images ship the CLI tools anyway.
- **The CLI with JSON:** `ZfsCli` runs `zfs` and `zpool` with `-j -p` (OpenZFS 2.3 and later) and parses with `serde`
  into `Pool`, `Dataset`, `Snapshot` and `Property`. For older ZFS, a text fallback with `-H -p` gives the same types;
  the choice is made once, from `zfs version -j`.
- **Kernel interfaces read directly** where they are files: `/proc/spl/kstat/zfs`, `/sys/module/zfs/version`,
  `/proc/self/mounts`.
- **One trait, two implementations:** `trait Zfs` with `ZfsCli` and `FakeZfs`, an in-memory model that tests build
  with builders; every command `ZfsCli` runs is in the command journal.
- **Recorded output:** fixtures for each command and ZFS version, refreshed by `xtask record-zfs`.

## The shell

ZFSBootMenu runs shell code in many places: the recovery shell, user hooks, SSH sessions, and short scripts of its
own. Rust images run all of it on [brush](https://github.com/reubeno/brush), a shell compatible with Bash and written
in Rust (decision 9). GNU Bash stays in every image as dracut's `/bin/sh`, which dracut's own scripts need, and legacy
images use Bash as before.

- **One choice, made once.** `zfsbootmenu-preinit` chooses the shell from `zbm.shell` and whether the image has
  `/usr/bin/brush` (`zbm_platform::shell::choose`), passes it on to every program in `ZBM_SHELL`, and writes it to
  the state directory for SSH logins. `zbm.shell=bash` is the way back to Bash.
- **Where it is used.** The supervisor's recovery shell, with `/bin/bash -i` as the fallback when brush does not
  start or ends within five seconds; hooks whose first line names Bash or `sh`, or that have no `#!` line; interactive
  SSH logins; the regex match and the send and receive pipeline of the runtime.
- **The terminal.** brush's best line editor, reedline, asks the terminal where its cursor is, and ends the shell if
  no answer comes. `zbm-brush`, the program images install as `brush`, starts brush's basic line editor on serial
  lines, which may not answer; virtual terminals, the console on the display and SSH get reedline.
- **Patches.** brush is vendored unchanged, and fixes go in `patches/` through `[patch.crates-io]`, each to be offered
  upstream. Today: a parser fix for `case` patterns inside `$( )` (`patches/README.md`).
- **Known gaps.** A script given on standard input fails where a `$(` substitution spans lines; script files work.

## Threat model

ZFSBootMenu runs before the operating system, holds the keys of encrypted volumes, and reads data that someone other
than its user may control. [docs/dev/threat-model.md](docs/dev/threat-model.md) says against whom, which inputs are
hostile, which properties must hold with their evidence, and which risks stay. Every phase keeps it current, and each
fuzz target names the input it covers. It covers:

- **Attackers:** physical access to a powered-off machine ("evil maid"), a malicious USB device, whoever controls a
  pool the user imports, and processes in the booted system.
- **Untrusted input:** the command line and configuration of the image, pool and dataset names and properties, files
  in `/boot`, hooks, crypttab and key files on removable devices, LUKS headers, program output, key presses, display
  data, and what programs write to the console on the display.
- **Display:** names that a hostile pool controls never reach the terminal with control characters or escape
  sequences; a fuzz target renders hostile names through `view` (R5).
- **Before R9** removes the legacy runtime, an independent security review against the model is an exit condition.

## Phases still to do

Each phase ends with every suite green, the measures of its goals met for the code it touched, and its docs written.
Behaviour may change from R5 on; each change is listed in `CHANGES-FROM-LEGACY.md`.

### R5. The new menu

Steps 1 to 4 are done ([record](#done-in-r5)). Each step is pushed and tested on its own:

5. **Snapshots and package D** (under way): diff, clone, rollback and set default, and the command palette. Done:
   the ZFS port makes the changes; `zbm-app` performs them, in the menu when nothing has to be asked and on the
   console otherwise; the menu names new snapshots and environments in a dialog. Every VM test of the Rust images
   passes on `57e74b1` (779 checks), and the two `oldzfs` tests with OpenZFS 2.2. **Left:** VM tests of each change
   through the menu, the previews of a diff and of package changes, and the command palette's entries for the
   changes.
6. **The rest of the menu:** kernels and the command line editor, pools and LUKS volumes from the menu (unlock and
   import instead of the recovery shell), recovery tools, help, power. Port package C (boot environments, kernels,
   keys, booting) splits out of `zbm-boot` into `zbm-bootenv` and `zbm-kexec` as these views need it.
7. **The exit:** `CHANGES-FROM-LEGACY.md` as a migration guide, the key binding section of the man page generated
   from `keymap.rs`, the 10,000-snapshot performance test and the menu's benchmarks (B2 to B8), the hostile-names
   fuzz target, the 52×20 pseudo-language fit, and usability sessions on the R5 build.

**Exit:** the VM tests drive every core task through the menu and pass; snapshot tests cover every view at three
sizes, in colour, monochrome and ASCII; the task measures and the redraw budget are met; the performance test with
10,000 snapshots passes; the hostile-names fuzz target passes; the menu runs without `fzf`, also on a serial console;
no string the user reads is a literal outside the message catalogue.

### D. Menu design: the exit

The design, the prototype and the wireframes are done ([record](#done-in-d)). Left: the usability sessions on the
prototype (at least five people who use ZFSBootMenu, one of them on a serial console), the review of the design
documents, and decision 8.

### R6. Recovery tools and image assembly in Rust

- Port package F (`zfs-chroot`, `zsnapshots`, `zkexec`, `mount_esp`, `reboot`, `firmware-setup`, `logs`, the help
  viewer) as `zbm-app` use cases: views of the menu's recovery tools, and thin `zbm` applets for scripts and the
  recovery shell. `fzf` then leaves Rust images.
- The built-in hooks `20-dracut-console-init` and `90-xhci-unbind` become Rust steps, as `30-console-autosize` and
  `80-luks-close` already are; user hooks stay scripts, which brush runs.
- Image assembly moves from `install-helpers.sh` into `generate-zbm`, a library that computes the file list; the
  dracut `module-setup.sh` and the mkinitcpio install hook shrink to shims that call it, and the build options are
  read by it.
- Releases publish the debug files of every binary next to the images, indexed by build ID.
- **Exit:** the only shell scripts left in the runtime are the two packaging shims and the user hook examples.

### R7. Remaining tests and tooling in Rust (alongside R5 and R6)

- The upstream test VMs (`tests/setup.sh`, `tests/run.sh`) as non-interactive `zbm-vmtest` scenarios.
- `scripts/` and `packaging/container/*.sh` become `xtask` commands (`build-image`, `record-fixtures`, `release`,
  `compare-images`); the container's entry point becomes a Rust program.
- **Exit:** no Bash, Perl or Python under `tests/`, `scripts/` or `packaging/container/`; CI runs device and VM tests
  through `xtask`.

### R8. Developer documentation (finishing alongside R7)

- `docs/dev/`: the boot flow from `zbm` start to `kexec`, stage by stage, with links into rustdoc; how to add a key
  method, a menu screen and a boot step; how to debug a failing boot; how the tests are organised.
- Rustdoc landing pages per crate; `cargo doc --workspace` published by CI.
- **Exit:** a developer new to the code base adds a key method and a menu screen using only the documentation.

### R9. Cutover and removal (with PORT-ROADMAP phases 7 and 8)

- The Rust runtime becomes the default after a beta in which the legacy image is the fallback (decision 6).
- Delete `legacy/`, the golden files recorded from it that no longer guard anything, the LUKS unit suites in Bash and
  the compatibility shims.
- An independent security review of the Rust runtime against the [threat model](#threat-model); its findings are
  fixed or accepted in writing before the legacy runtime is removed.
- The [benchmarks](#benchmarks) run on the beta's images: none is worse than the legacy runtime's without a reason
  recorded.
- **Exit:** the tree contains Rust, the packaging shims, documentation and user hook examples only; the security
  review is done.

### Order and overlap

```text
done:  R0 ─ R1 ─ R2 ─ R3 ─ R4 ─┬─ D (exit: usability sessions)
                               │
now:                           R5 (step 5) ─ R5 (steps 6, 7) ─ R6 ─ R9
                                                    └──── R7 ────┤
                                                           R8 ───┘
```

## Relation to PORT-ROADMAP.md

- Packages A and B of the port are done and were refactored in R1 to R4.
- Packages C, D, E and F are not ported in the old architecture: C and D land in R5, E is replaced by the redesigned
  menu, and F lands in R6. The menu is written once, not around `fzf` first.
- From R5 on, behaviour changes are allowed and recorded in `CHANGES-FROM-LEGACY.md`.
- The legacy freeze, `legacy/RUNTIME-CHANGES.md` and the rule that every suite runs against both images stay until
  R9.

## Risks

| Risk | Mitigation |
| --- | --- |
| Behaviour drifts while structure changes | Golden files recorded from the legacy code, the full VM suite after each step, shadow mode on testers' machines |
| Documentation drifts from behaviour | Options and key bindings are defined once and their documentation generated; `xtask gen-docs --check` |
| Crashes on users' machines cannot be diagnosed | Crash reports with build IDs and published debug files; `xtask symbolize` |
| Shadow mode harms a boot | Read-only adapter with an allowlist, no prompts or secrets, a time budget, an ignored exit status; opt-in images only |
| The new menu is slower or worse on small consoles than `fzf` | Snapshot tests at 52 columns; a performance test with 10,000 snapshots; the benchmarks against the legacy menu |
| New dependencies grow the image and the attack surface | Image size per part (B1), `cargo deny` advisories, minimal features, vendoring |
| uutils behaviour differs from what users expect on GNU systems | Golden tests for every coreutils behaviour the runtime relies on; documented differences |
| brush, younger than Bash, runs a script differently or has a parser bug | Every Rust VM test runs on it; fixes in `patches/`, offered upstream; `zbm.shell=bash`; the recovery shell falls back to Bash; threat model R7 |
| JSON output is missing in older ZFS | Text fallback with the same types; the `oldzfs` tests with OpenZFS 2.2 |
| The refactor stalls half way, with two architectures | Each phase is complete on its own; `xtask metrics` shows how far each crate has come |
| A refactor breaks the tests that should catch it | Test suites move to Rust before the code they protect changes, and old and new suites run side by side until they agree |
| A hostile disk, pool or USB device attacks the boot | The threat model, a fuzz target per untrusted input, an independent security review before R9 |
| Rules erode as more people contribute | Every rule is checked by a tool in CI |
| The redesign breaks users' habits | A migration guide in `CHANGES-FROM-LEGACY.md`; the keys of the focused element on every screen; the old `MOD+key` bindings as aliases; usability sessions with existing users |
| Design work grows without end | D has a fixed exit: the core tasks only; new ideas go to ROADMAP.md or the proposals below |

## Decisions

Decided:

1. **Dependencies** (22 October 2026, with additions since): for the runtime, `ratatui`, `crossterm`,
   `nucleo-matcher`, `serde`, `serde_json`, `thiserror`, `tracing`, `tracing-subscriber` and `uucore`; for tests,
   `insta`, `proptest` and `pretty_assertions`. Versions and features are pinned in `[workspace.dependencies]`, with
   default features off where the crate allows it. The menu takes `nucleo-matcher` rather than `nucleo`, whose thread
   pool a boot menu's short lists do not need; `nucleo-matcher` is under MPL-2.0, which `deny.toml` allows for this
   crate only. Added since, each with its reason in `docs/dev/dependencies.md`:
   - for the VM harness only: `vt100` and `regex`;
   - for ROADMAP.md #19: `drm` 0.15 (step 1), `alacritty_terminal` 0.26 with `rustix` and `signal-hook` (step 2),
     `swash` 0.2 rather than the unmaintained `fontdue` (step 3), and `kbvm` 0.1.7 rather than `xkbcommon-rs` or the
     kernel's keymap (step 4);
   - for R5 step 2: `fluent-bundle` 0.16 and `unic-langid` 0.9, the menu's message catalogue;
   - for the shell: `brush-shell` 0.4.0 (decision 9).
2. **MSRV:** 1.88, what `ratatui` 0.30, `uucore` 0.12 and `proptest` 1.12 need, checked in CI; it rises only when an
   agreed crate needs it.
3. **Menu:** the Nucleo and Ratatui menu, redesigned in D, replaces `fzf` outright in R5; `fzf` leaves Rust images
   with R6's recovery tools.
4. **uutils in `zbm`** (R2): images ship the uutils multi-call binary next to `zbm`, which does not link `uu_*`
   applets ([Using uutils coreutils](#using-uutils-coreutils)).
5. **Errors:** `thiserror`, which only runs at build time and adds nothing to the binaries.

Decided later, after 6 to 8 were opened:

9. **The shell** (October 2026): Rust images run their shell work on brush 0.4 (`brush-shell`, MIT), not GNU Bash,
   and not a shell of another language such as fish, which would not run the Bash of hooks and of ZFSBootMenu's own
   libraries ([The shell](#the-shell)). It brings 190 crates to vendor, with two licences that `deny.toml` allows by
   name: `terminfo` (WTFPL) and `git-version` (BSD-2-Clause). The release `brush` is 9.0 MB stripped, 2.8 MB with
   `zstd -19` (Bash: 1.4 MB and 0.6 MB; `zbm`: 5.8 MB and 2.1 MB); B1 tracks it. GNU Bash stays as dracut's
   `/bin/sh`. Legacy images keep Bash. `zbm.shell=bash` and the fallback of the recovery shell are the way back if brush fails someone.

Still open:

6. **When the legacy runtime goes:** keep the PORT-ROADMAP beta (phase 7) with the legacy image as fallback, or cut
   over directly after R6; decided before R9.
7. **Nucleo or its matcher:** decision 1 stands unless the command palette misses its frame budget in the performance
   test of R5 (10,000 snapshots); then `nucleo` is taken with its worker thread, and `rayon` and `parking_lot` are
   measured against the image size budget. Decided in R5.
8. **A classic key map:** whether `keymap.rs` offers a second profile close to the `fzf` menu's bindings. Proposed in
   D: no second profile; the `MOD+key` bindings work as aliases in their old views
   ([keymap.md](docs/dev/menu-design/keymap.md)). Decided by the usability sessions.

## Proposals for later

Improvements suggested during review and kept for later. None is part of the plan yet; each is added to a phase when
it is adopted.

1. **Trustworthy fakes:** contract tests that run one suite against both `FakeZfs` and `ZfsCli` (in a VM), and the
   same for the `zbm-crypt` adapters; mutation testing with `cargo-mutants`; state machine property tests for the
   import retry loop and the unlocker.
2. **Debugging failures offline:** `xtask replay <bundle>` turns a `zbm report` bundle (ROADMAP.md #9, done) into a
   fake machine, so a reported boot failure becomes a `cargo test`; boot timelines as Chrome trace files.
3. **Developer experience:** Architecture Decision Records in `docs/dev/adr/`; runnable `examples/` per crate; an
   `AGENTS.md` per crate with its invariants, commands and test conventions for AI tools.
4. **Stable error codes:** `ZBM-E0412: no key method could open /dev/sda2`, explained by `zbm explain <code>`.
5. **The supply chain:** `cargo vet` and `cargo auditable`.
6. **A deprecation policy** for options and configuration files: old names keep working with a warning for one
   release; possibly a typed TOML configuration with `zbm config check`.
7. **One concurrency model:** threads and channels with a single event loop, no async runtime, documented once.
8. **Fewer crates, with a rule for splitting:** split a crate only when it has its own dependencies or a second user;
   track build time in CI against a budget.
9. **The Rust API Guidelines as the review checklist** for G1: naming, conversions, `#[must_use]` on outcome types,
   `#[non_exhaustive]` on public enums that will grow.
10. **A version matrix in CI:** scheduled VM runs across ZFS versions, dracut and mkinitcpio, x86_64 and aarch64.
11. **Small, mergeable steps:** one tracking issue per phase with a checklist, no step larger than a reviewable pull
    request, every merge leaves the main branch releasable.
12. **A log level policy:** error, warn, info, debug and trace each with one meaning; a test that a normal boot at the
    default level fits on an 80×25 console.
13. **`xtask doctor`:** checks the toolchain, the build container, QEMU and KVM, swtpm and loop devices, and says what
    is missing.
14. **More static checks:** `cargo machete` for unused dependencies, and sanitizers on the `zbm-sys` tests.
15. **An upstream sync policy:** for each relevant upstream ZFSBootMenu fix, a test first, then the port, logged as
    `legacy/RUNTIME-CHANGES.md` does today.
16. **Metrics for rules 2 and 4:** debt columns for public functions of domain crates that take a raw string, and for
    public items without a doc example.

## Record of finished work

What each phase did, in the order it was done.

### Findings at the start

The analysis at the start of R0 found the Rust code formatted, clippy-clean, documented, with `unsafe` confined to
`zbm-sys` and every system call behind a trait with a fake. But it was written to *be* the legacy code in another
language. The findings, and where each ended:

| # | Finding | Goals | Now |
| --- | --- | --- | --- |
| F1 | The code was shaped like the Bash it replaced: 229 doc lines named legacy functions, shell semantics were emulated 71 times, parsers copied `sed` and `awk` quirks, values were strings | G1, G3 | Fixed in R1 to R3 and the debt clean-up: no legacy-named doc line, shell helpers only in configuration parsing, LUKS2 metadata through `serde`, typed stages, policies, host IDs and outcomes |
| F2 | Errors and outcomes were invisible: 50 functions returned a success flag | G5 | Fixed: typed errors and outcome enums; no success flag left |
| F3 | Coupling made features expensive: one `Session` type over eleven files, `zbm-boot` mixing orchestration, policy, IO and drawing, no application layer, files of 1,300 lines | G4 | Fixed: `Unlocker` with strategies, `zbm-pool` and `zbm-startup`, `zbm-app`; every file within the size rules |
| F4 | The ZFS boundary was text tables | G2, G4 | Fixed in R2: `trait Zfs` with typed values from JSON and a text fallback; start-up's import still runs the legacy command lines ([carried over](#carried-over)) |
| F5 | Debugging meant reading the kernel log | G5 | Fixed: `tracing` spans, the journal, crash reports, `zbm debug` and `zbm report` |
| F6 | Tests ran Bash (71 of them), fakes were closures, device and VM suites were Bash and Python | G7, G8, G10 | Fixed in R1 to R3: no Rust test runs Bash; `FakeMachine` builders; device suites and the VM harness in Rust. Left: the upstream test VMs (R7) and the test map |
| F7 | Coreutils were used implicitly, with an emulated GNU `sort -V` | G9 | Fixed in R2: `uucore`'s version order, file trees in process, uutils in images |
| F8 | What was good and stays: the system interfaces and their fakes, `Command`, `Secret`, the supervisor's no-panic discipline, the multi-call `zbm`, the fixtures and VM scenarios | | Kept and sharpened |

### Done in R0

`cargo xtask` with `lint`, `test`, `bless`, `doc`, `check-deps`, `metrics`, `coverage` and `widths`; the strict-lint
list; the dependency layers and the generated `docs/dev/architecture.md`; the starting metrics and coverage baselines,
which CI ratchets; `cargo deny`; decisions 1, 2, 3 and 5. Two pilot slices ran through every layer: key-file unlock
(`zluks --pilot open`) and the boot environment list of the new menu, both in the Rust image on a serial console
(`runtime-rust-pilot`). What they taught:

- **Layers:** `types`, `ports` and `application` were added; test-only crates are allowed as development
  dependencies everywhere.
- **Context:** it must not require a console, since `zluks` run from a script has none.
- **Errors:** typestate with a hand-back works (`Mapping<Closed>` → `Mapping<Open>`, a failure returns the mapping);
  `cryptsetup`'s exit statuses map onto typed errors.
- **The menu on a serial console:** Ratatui asks where the cursor is, which a serial console may not answer, so the
  menu's backend never asks (`QuietBackend`); `TERM=vt100` gets ASCII borders. The same lesson chose brush's basic
  line editor for serial lines.
- **Measured:** frames of 15 to 24 ms under software emulation, about 76 serial bytes per key, `zbm` from 1.09 MB to
  1.78 MB with the menu's crates.
- **Tests:** snapshot tests paid off at once; the VM harness needed a terminal emulator (`vt100`, R3).
- **Lints:** `clippy::must_use_candidate` made 313 of 1,105 strict findings; the strict set allows it.

### Done in R1

- `zbm-testkit`: no Rust test runs Bash, Perl or `legacy/`; the legacy results are golden files that
  `cargo xtask record-parity` records and CI checks. `FakeMachine` builders, `assert_ran!`, `assert_not_ran!`,
  `assert_no_secret_leaked!` and `cases!`.
- The threat model and the fuzz targets; their first run found a panic on non-ASCII `zpool import` output, now fixed.
- `zbm-types`: `SnapshotName`, `KernelVersion`, `ImportPolicy`, `Stage`, `HookStage`, with property tests.
- `zbm-trace` replaces `zbm-log`: events go to the kernel log in the legacy formats and to the journal
  (`/zfsbootmenu/journal.jsonl`), with secret fields redacted.
- Crash reports, `zbm debug crash`, `cargo xtask split-debug` and `symbolize`, checked by `runtime-rust-crash`.
- Error enums and outcomes in `zbm-luks` and `zbm-boot`.
- Shadow mode; every legacy VM scenario reports zero differences.

### Done in R2

- `zbm-platform` holds the ports and their real implementations; `zbm-system` is gone.
- The ZFS port, `zbm_zfs::Zfs`, from JSON or text into the same types; `FakeZfs`; `cargo xtask record-zfs` and the
  OpenZFS 2.4 fixtures.
- The `zbm-crypt` adapters (`cryptsetup`, `blkid`, `tpm2_*`, `fido2-*`, clevis, YubiKey, GnuPG), with RAII guards.
- uutils: `uucore`'s version order, file trees in process, the multi-call binary in images; decision 4.
- Adapters only: `zbm-tools` runs the system's programs; `Command::new` is disallowed elsewhere.
- The shell helpers are gone outside configuration parsing.
- The LUKS device suites in Rust, 252 checks, check for check with the Bash suites.
- Exit: every LUKS VM test passed on `aaef3a9` (44 tests, 764 checks) with zero shadow differences.

### Done in R3

- `zbm-pool`: typed import settings, `Import`, `HostId`, and the retry loop as a state machine behind the `Operator`
  port.
- `zbm-startup`: the start-up program as `Vec<Box<dyn Step>>`, one step per `Stage`.
- `zbm-luks`: `Unlocker` with `KeyMethod` strategies, a `TokenHandler` registry, the passphrase prompt as a state
  machine, and `MappingStore`.
- `zbm-config`: the option table (98 entries), the generated option sections of `zfsbootmenu(7)`,
  `zfsbootmenu-crypttab(5)` and `generate-zbm(5)`, and the origin of every value.
- The size and type rules: `boot.rs`, `generate.rs` and the long functions of `zbm-luks` and `zbm-config` split;
  no long function or file in those crates.
- The VM harness in Rust, `zbm-vmtest`, with every scenario of the Python harness; the two gave the same 764 checks,
  and the Rust one found a documentation error (`zpool wait -t replace`). The Python harness is removed.

### Done in R4

- `zbm-app`: the vocabulary of `docs/dev/menu-design/interfaces.md` (`Snapshot`, `Action`, `Event`); `App::snapshot`
  reads the machine; `App::act` unlocks, opens and closes, with events; start-up sends stage events.
- `zluks` goes through `App::act`; `check-deps` refuses a front end that depends on an adapter.
- `zbm debug` gained `config`, `state`, `journal`, `commands`, `luks` and `pools` (`zbm-debug(8)`), replacing `ztrace`
  and `zlogtail` for the Rust runtime.
- The Rust code decides key sources, and shadow mode compares them; `runtime-legacy-keysource` boots with one.
- Verified on `7a25250`: zero shadow differences in every legacy VM test.

### Done in D

- The design in `docs/dev/menu-design/`: users, core tasks with keystroke budgets, the information architecture, the
  visual system, the key map, the lists for `zbm-app`, the plan of the usability sessions, and the adaptation of
  Apple's Human Interface Guidelines to the console.
- In `zbm-tui`: the theme, capability detection and key map, and a prototype (`cargo run -p zbm-tui -- --demo`) that
  runs every core task on a fake machine within its keystroke budget.
- The wireframes are the prototype's snapshot tests, at 120×40, 80×25 and 52×20, and in ASCII; only glyphs of the
  image's fonts are used.
- Prototypes beyond the core tasks, for after R5: boot health and boot counting, boot once, file restore and package
  changes (ROADMAP.md #6, #7, #21), the TPM2 re-seal (#11), the keyboard layout at unlock and clearer unlock errors
  (#4).

### Done in R5

1. **Keys on the countdown's boot:** `zbm_boot::keys::load_key` loads the key of a locked encryption root in the
   legacy order, with `load-key.d` hooks; the `complex` VM tests pass on the Rust runtime.
2. **The message catalogue:** every string of the menu comes from `crates/zbm-tui/locales/en/menu.ftl`; a
   pseudo-language is drawn at 52×20, and `cargo xtask lint` refuses a text literal in the menu's code.
3. **The menu in the image:** the main view on the real machine, booting from it with key sources, the countdown and
   the start-up view, replacing `fzf` as the menu. Every VM test of the Rust images passed on `910ce63`.
4. **Start-up inside the menu:** the stages run on the start-up view, and its dialogs answer the prompts of
   `zbm-luks` and `zbm-pool`; keys are read into buffers that are wiped. Every VM test of the Rust images passed, 25 to
   27 October 2026.

A review of the Rust port after step 4 found bugs in seven groups (test tooling, wrong boots and lock-outs, the
start-up view, process 1 and the console, the menu, `generate-zbm`, logs and parity gaps); all are fixed
(`CHANGELOG.md`, "Fixes from the review of the Rust port").

### Done outside the phases

Work that no phase planned, or that a later phase planned and was done early:

- **Debt:** after R4, `zbm-kcl` and `zbm-state` within the size rules, no success flag and no legacy-named doc line
  left. Every debt metric but the configuration parsers' shell helpers and `zbm-sys`'s `unsafe` blocks is zero.
- **R6's built-in hooks** `30-console-autosize` and `80-luks-close` are `zbm console-autosize` (sizing the font from
  the display's EDID) and `zluks close`.
- **ROADMAP.md features:** #1 secrets in locked memory, wiped before `kexec`; #5 reproducible images; #9 saved boot
  logs and the `zbm report` bundle; #17 step 1 and 2, graphics drivers chosen by GPU and loaded before the first
  prompt; #20 console fonts (256-glyph subsets, Spleen).
- **ROADMAP.md #19, the console on the display** (`zbm-display`, `zbm-console`, `zbm.console=drm`): the menu on DRM
  without a terminal; Alacritty's terminal drawn through DRM with the supervisor's programs on its pseudo-terminal;
  TrueType fonts; keyboard layouts with `kbvm`; and M1 of the [console roadmap](docs/dev/console-roadmap.md) (start
  without a keyboard, cursor shapes, underlines, one frame per burst, scrolling by moving pixels, `/dev/console`, the
  screen kept across a failed `kexec`, the keypad). Its VM tests pass.
- **brush** (decision 9, [The shell](#the-shell)): vendored with its parser fix, `zbm-brush`, and the Rust runtime on
  it; its VM run is the first item of [Now](#now).
