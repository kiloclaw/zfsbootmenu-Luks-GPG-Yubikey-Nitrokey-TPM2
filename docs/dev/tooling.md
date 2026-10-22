# Tooling: `cargo xtask`

`cargo xtask` runs the developer automation in `xtask/`, a Rust program in the workspace. The alias in
`.cargo/config.toml` builds and runs it, so it needs nothing but the Rust toolchain. Each command prints the programs
it runs (`$ cargo clippy ...`), so a failing step can be repeated alone. CI runs the same commands, so a change that
passes `cargo xtask lint` and `cargo xtask test` locally passes those jobs.

| Command | What it does |
| --- | --- |
| `cargo xtask lint` | Every check a pull request must pass: `rustfmt`, line widths, clippy with warnings as errors (also on the test build of `zbm`), rustdoc with warnings as errors, `check-deps` and `metrics --check`. All checks run, and the failures are listed at the end |
| `cargo xtask lint --strict` | Reports the stricter lints that refactored crates must pass (below). Never fails |
| `cargo xtask test` | Every test that needs neither root nor a virtual machine: the workspace and the test build of `zbm` |
| `cargo xtask doc [--open]` | Builds the documentation with warnings as errors |
| `cargo xtask check-deps [--bless]` | Checks the dependencies between crates against the layers ([architecture.md](architecture.md)); `--bless` regenerates the tables on that page |
| `cargo xtask metrics [--check \| --bless]` | Prints code metrics per crate (below) |
| `cargo xtask widths` | Checks the line widths of The Rust Style Guide that `rustfmt` leaves alone: at most 100 characters, comment lines at most 80 without their indentation |

The suites that need root or QEMU are described in [testing.md](testing.md); they are not part of `cargo xtask` yet
(REFACTOR-ROADMAP.md moves them there in R2 and R3).

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
`panic`, `indexing_slicing`, `unreachable`, `todo`, `dbg_macro`) and the configuration in `xtask/strict/clippy.toml`,
which forbids running programs, touching files, reading the environment or the clock, and printing, outside the real
implementations of the system interfaces. It builds in `target/strict` so the normal build stays cached, prints the
findings per crate and per lint, and writes them to `target/strict/strict-lints.txt`.

Today these findings are only reported. As each phase of REFACTOR-ROADMAP.md finishes a crate, that crate denies the
lints itself, and the normal `cargo xtask lint` enforces them. `xtask` already passes them.
