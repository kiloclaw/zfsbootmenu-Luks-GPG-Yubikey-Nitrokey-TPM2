# Tooling: `cargo xtask`

`cargo xtask` runs the developer automation in `xtask/`, a Rust program in the workspace. The alias in
`.cargo/config.toml` builds and runs it, so it needs nothing but the Rust toolchain. Each command prints the programs
it runs (`$ cargo clippy ...`), so a failing step can be repeated alone. CI runs the same commands, so a change that
passes `cargo xtask lint` and `cargo xtask test` locally passes those jobs.

| Command | What it does |
| --- | --- |
| `cargo xtask lint` | Every check a pull request must pass: `rustfmt` (also of the fuzz targets), that the fuzz targets build, line widths, clippy with warnings as errors (also on the test build of `zbm`), the strict lints of the crates in `xtask/strict/crates.txt` (below), rustdoc with warnings as errors, `gen-docs --check`, `check-deps` and `metrics --check`. All checks run, and the failures are listed at the end |
| `cargo xtask lint --strict` | Reports the strict lints for every crate (below); fails only on a finding in a crate of `xtask/strict/crates.txt` |
| `cargo xtask test` | Every test that needs neither root nor a virtual machine: the workspace and the test build of `zbm`, under `cargo nextest` when it is installed ([testing.md](testing.md)) |
| `cargo xtask record-zfs [--manifest]` | Records what `zfs` and `zpool` print, as JSON and as text, in a test VM (root, QEMU), into `crates/zbm-zfs/tests/fixtures/<major.minor>/`; `--manifest` prints the commands the VM harness runs ([testing.md](testing.md)) |
| `cargo xtask record-parity [--check]` | Runs the legacy code of the parity tests and records its results in `tests/golden/legacy/`; `--check` fails where a recording no longer matches ([testing.md](testing.md)) |
| `cargo xtask fuzz [<target>] [--seconds <n>]` | Runs the fuzz targets of `fuzz/` with nightly Rust and `cargo-fuzz` ([testing.md](testing.md)) |
| `cargo xtask bless` | Accepts every changed snapshot (`INSTA_UPDATE=always` over the crates that use `insta`) and lists the `.snap` files that changed, for review with `git diff` |
| `cargo xtask coverage [--check \| --bless]` | Prints line coverage per crate with `cargo llvm-cov` (below) |
| `cargo xtask doc [--open]` | Builds the documentation with warnings as errors |
| `cargo xtask gen-docs [--check]` | Writes the option sections of `zfsbootmenu(7)`, `zfsbootmenu-crypttab(5)` and `generate-zbm(5)` from the table of options in `zbm-config` (`zbm_config::spec`), between the `.. BEGIN generated` and `.. END generated` lines of each page; `--check` fails when one is out of date. Edit an option's entry in the table, never the generated text. It runs the example `gen_docs` of `zbm-config`, since `xtask` depends on no crate of the workspace |
| `cargo xtask check-deps [--bless]` | Checks the dependencies between crates against the layers ([architecture.md](architecture.md)); `--bless` regenerates the tables on that page |
| `cargo xtask metrics [--check \| --bless]` | Prints code metrics per crate (below) |
| `cargo xtask split-debug <binary> [<dir>]` | Moves the debug information of a release binary to `<dir>/<build-id>.debug` (default `target/debug-files`) with `objcopy` and strips the binary; image builds do this to `zbm` (below) |
| `cargo xtask symbolize <report> [--debug-dir <dir>]...` | Prints a crash report of the runtime with the function, file and line of each frame of the crashed binary, from the debug file with the report's build ID, with `addr2line` (below) |
| `cargo xtask widths` | Checks the line widths of The Rust Style Guide that `rustfmt` leaves alone: at most 100 characters, comment lines at most 80 without their indentation |

The suites that need root or QEMU are described in [testing.md](testing.md); they are not part of `cargo xtask` yet
(REFACTOR-ROADMAP.md moves them there in R2 and R3).

## Crash reports

When a runtime program panics, its panic hook (`zbm_platform::CrashReporter`) writes
`/zfsbootmenu/crash-<time>-<pid>.txt`, logs an error naming it and writes it to the journal; when the supervisor's
start-up program is killed by a signal, the supervisor writes one. The recovery shell names the report, and
`zbm debug crash` prints the latest (`--list` lists them). A report holds the panic message and place, the build ID
of the binary, the `tracing` spans that were open, and the stack as offsets from the binary's load address, which
address space randomisation does not change:

```text
ZFSBootMenu crash report
program: zfsbootmenu-init
binary: /libexec/zbm
...
build-id: 875eff95d23b38bc184fe3a40d875ee1f296b335
spans: startup > import{pool=tank}
frames:
  #0 0x000000000010c436 /libexec/zbm
```

The image's `zbm` is stripped, so the offsets are all a machine can tell. Release builds keep line tables and
function names (`debug = "line-tables-only"`), and the image build (`packaging/container/build-init.sh`) moves them
with `cargo xtask split-debug` to `debug/<build-id>.debug` next to the images. With that file,

```sh
cargo xtask symbolize crash-report.txt --debug-dir build/debug
```

prints each frame of the binary as its function, file and line, with the calls inlined into it. The commands use
`objcopy` and `addr2line` from GNU binutils (or the programs named by `OBJCOPY` and `ADDR2LINE`).
`make install` strips the programs it installs unless `INSTALL_STRIP` is empty, for packages that keep debug files
of their own.

## Third-party crates

`cargo deny check` (with [cargo-deny](https://github.com/EmbarkStudios/cargo-deny), installed with
`cargo install cargo-deny --locked`) checks every third-party crate against `deny.toml`: permissive licences only
(MPL-2.0 for the menu's matcher alone), no known security advisory or yanked version, crates.io as the only source,
and no binding to the ZFS C libraries. CI runs it on every change to the dependencies. Which crate may use which
third-party crate is `cargo xtask check-deps`'s business ([architecture.md](architecture.md)).

## Code metrics

`cargo xtask metrics` reads every source file of every crate with `syn`, following the module tree like the compiler,
so it knows which code is test code. It prints one row per crate and writes the same numbers to
`target/metrics.json`.

| Column | Counts | Debt |
| --- | --- | --- |
| files, lines | Source files and their lines | |
| fns | Functions outside tests | |
| tests | `#[test]` functions | |
| long fns | Functions outside tests longer than 60 lines | yes |
| long files | Files longer than 600 lines | yes |
| bool flags | Functions outside tests that return `bool` but whose name asks no question (`is_`, `has`, `present`, `valid` and the other words in `xtask/src/metrics/mod.rs`): success flags that lose the reason for a failure. Trait methods do not count | yes |
| Result fns | Functions outside tests that return a `Result` | |
| legacy docs | Doc comment lines that name a function of the legacy code between backticks, explaining Rust by the Bash it replaced | yes |
| shell helpers | Uses of the shell-semantics helpers of `zbm_core::shell` outside tests | yes |
| legacy tests | Tests that run legacy Bash or Perl, directly or through a helper | yes |
| unsafe | `unsafe` blocks | yes |

The debt columns may only go down. `xtask/metrics-baseline.txt` holds each crate's numbers at its last improvement,
and `cargo xtask metrics --check` (part of `lint`) fails when a crate goes above them; a crate that is not listed, such
as a new one, must stay at zero. After improving the code, run `cargo xtask metrics --bless` to lower the baseline in
the same pull request. Never raise a number by hand: if a change needs more debt, the reviewer should see that
discussion in the pull request instead.

## Strict lints

`cargo xtask lint --strict` runs clippy with `clippy::pedantic`, the no-panic lints (`unwrap_used`, `expect_used`,
`panic`, `indexing_slicing`, `unreachable`, `todo`, `dbg_macro`), except `must_use_candidate` (`#[must_use]` belongs
on outcomes and errors, not on every getter; `STRICT_ALLOWED` in `xtask/src/lint.rs`), and the configuration in
`xtask/strict/clippy.toml`,
which forbids running programs, touching files, reading the environment or the clock, and printing, outside the real
implementations of the system interfaces. It builds in `target/strict` so the normal build stays cached, prints the
findings per crate and per lint, and writes them to `target/strict/strict-lints.txt`.

A crate that has reached the strict lints stays there: `xtask/strict/crates.txt` lists those crates (`xtask`, the
crates of the R0 pilots and the empty `zbm-menu`), `cargo xtask lint` runs the strict lints over them alone and fails
on any finding, and `cargo xtask lint --strict`, which reports every crate, fails on them too. Each phase of
REFACTOR-ROADMAP.md adds the crates it finishes; a crate never leaves the list. The other crates' findings are only
reported. Silence a finding that is right in context with `#[allow(lint, reason = "...")]` on the item, never for a
whole crate.

## Line coverage

`cargo xtask coverage` runs the tests of the workspace and of the test build of `zbm`, with `ZBM_PARITY=check` so that
the recordings of the legacy results are checked too, under
[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) (`cargo install cargo-llvm-cov --locked` and
`rustup component add llvm-tools`), adds up the lines of each source file per crate, test code included, and prints
the line coverage of each crate with what it must reach. The report is in `target/coverage.json`, and the profiles
stay, so `cargo llvm-cov report --html` shows the uncovered lines afterwards.

Coverage may only go up, to the whole percent: `xtask/coverage-baseline.txt` holds each crate's coverage, and
`--check` fails when a crate drops below it. A crate that is not listed, such as a new one, must reach 80%. After
adding tests, run `cargo xtask coverage --bless` to raise the baseline in the same pull request; never lower a number
by hand. CI's coverage job runs `--check`.

The baseline is measured as root, as CI measures it: a parity test of `zbm-boot` compares with the legacy code only
as root, the user that code runs as. Run the command as root (`sudo env "PATH=$PATH" cargo xtask coverage --check`)
to compare with it; as another user `zbm-boot` reads below its baseline.
