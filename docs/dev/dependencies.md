# Dependencies

Every third-party crate the workspace uses is vendored in `vendor/`, so that builds, including container builds,
need no network (`.cargo/config.toml` replaces crates.io with that directory). Refresh it after changing a dependency
in `Cargo.toml`:

```sh
cargo vendor --versioned-dirs --sync fuzz/Cargo.toml vendor
cargo deny check
```

`cargo vendor` owns `vendor/` and deletes any other file in it, which is why this page lives here. The fuzz targets
have a lock file of their own: after changing the dependencies of a crate they use, refresh it with
`cargo check --manifest-path fuzz/Cargo.toml --offline` (`cargo xtask lint` fails until then).

Each crate a workspace crate depends on directly is listed below with the reason it is used, and allowed for that
crate in `xtask/src/deps.rs` (`THIRD_PARTY`), which `cargo xtask check-deps` enforces; `cargo deny check` checks their
licences and advisories (`deny.toml`). Adding one is a decision for the pull request; REFACTOR-ROADMAP.md
("Decisions") lists the crates agreed for the refactoring, with their versions and features pinned in
`[workspace.dependencies]`.

## The boot runtime (`zbm`)

| Crate | Used by | Why |
| --- | --- | --- |
| `thiserror` | `zbm-types`, `zbm-trace`, `zbm-crypt`, `zbm-zfs`, `zbm-luks`, `zbm-app`, `zbm-boot` | Error enums with their causes; runs at build time only |
| `tracing`, `tracing-subscriber` | `zbm-trace`, `zbm-platform`, `zbm-crypt`, `zbm-luks`, `zbm-app`; `tracing` in `zbm` and `zbm-boot` | Spans and events; the stderr output and the JSON journal |
| `serde_json` | `zbm-trace`, `zbm-zfs`, `zbm-crypt` | The JSON journal; the JSON output of `zfs` and `zpool`; LUKS2 metadata from `cryptsetup` |
| `serde` | `zbm-zfs` | The shape of that output, derived (`derive`) |
| `ratatui` (with `crossterm`) | `zbm-tui` | The menu: layout, widgets, terminal output and key input |
| `nucleo-matcher` | `zbm-tui` | Fuzzy matching of the filter text, the matcher of Helix's Nucleo; MPL-2.0 (see `deny.toml`) |
| `uucore` (`version-cmp`) | `zbm-types` | Version order, the `sort -V` of uutils coreutils, the images' coreutils. Its required dependencies (`clap`, `fluent`, `nix` and others) are built, but link-time optimization leaves only `version_cmp` in `zbm`: 6.8 KB of a 2.2 MB stripped binary |

Their own dependencies are vendored with them; `cargo tree -e normal -p zbm` lists everything that is compiled into
`zbm`. Some vendored crates are never compiled for Linux: `windows-sys`, `winapi`, `wasi`, `redox_syscall` and the
like, which `Cargo.lock` records for other platforms, and proc-macro helpers such as `darling` and `derive_more`,
which run at build time only.

## Host tools

| Crate | Used by | Why |
| --- | --- | --- |
| `yaml-rust2` | `generate-zbm` | Reads `/etc/zfsbootmenu/config.yaml` as the Perl `YAML::PP` did |
| `hashlink`, `hashbrown`, `foldhash`, `arraydeque` | (`yaml-rust2`) | Its dependencies |

## Tests

These are development dependencies only, allowed for every crate (`TEST_THIRD_PARTY` in `xtask/src/deps.rs`).

| Crate | Why |
| --- | --- |
| `insta` | Snapshot tests: the menu's screens (`crates/zbm-tui/src/snapshots/`) |
| `proptest` | Property tests: parsing never panics, valid values round-trip (`zbm-types`) |
| `pretty_assertions` | Readable diffs when an assertion of equality fails |
| `serde`, `serde_json` | The golden files of the legacy results (`zbm_testkit::parity`, in `tests/golden/legacy/`); `serde`'s `derive` describes a test's recorded result |

## Fuzzing

`fuzz/` is a workspace of its own, so these are never built with the rest.

| Crate | Why |
| --- | --- |
| `libfuzzer-sys` | Links libFuzzer and provides `fuzz_target!`, as `cargo fuzz` expects |
| `arbitrary`, `cc`, `jobserver`, `shlex`, `find-msvc-tools` | Its dependencies: input types, and building libFuzzer's C++ |

## Developer tooling

Used by `xtask` alone, never by an image or a host tool.

| Crate | Why |
| --- | --- |
| `syn`, `proc-macro2`, `unicode-ident` | Parse Rust source for `cargo xtask metrics` |
| `serde_json` | Read `cargo metadata` and clippy's JSON messages, write `target/metrics.json` |

## Agreed, not used yet

Pinned in `[workspace.dependencies]` and vendored when a crate first uses them: `serde` with `derive` for
`zbm-crypt` (which reads JSON through `serde_json::Value` so far). `crossterm` is pinned for direct use but
reached through `ratatui::crossterm` so far, which keeps one version.

## Vendored but never built

`serde_json` names `serde`, and `serde_core` names `serde_derive`, as dependencies for a target that never matches
(`cfg(any())`), to keep the versions of the serde crates in step. Cargo records them in `Cargo.lock` and therefore
vendors them, with what `serde_derive` needs: `quote` and `syn` 3. None of them is compiled.
