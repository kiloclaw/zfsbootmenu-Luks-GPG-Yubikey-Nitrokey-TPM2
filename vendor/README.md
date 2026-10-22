# Vendored crates

Every third-party crate the workspace uses is vendored here, so that builds, including container builds, need no
network (`.cargo/config.toml` replaces crates.io with this directory). Refresh the directory after changing a
dependency in `Cargo.toml`:

```sh
cargo vendor --versioned-dirs vendor
```

Each crate a workspace crate depends on directly is listed below with the reason it is used, and allowed for that
crate in `xtask/src/deps.rs` (`THIRD_PARTY`), which `cargo xtask check-deps` enforces. Adding one is a decision for the
pull request; REFACTOR-ROADMAP.md ("Decisions for review") lists the crates proposed for the refactoring.

## In images or host tools

| Crate | Used by | Why |
| --- | --- | --- |
| `yaml-rust2` | `generate-zbm` | Reads `/etc/zfsbootmenu/config.yaml` as the Perl `YAML::PP` did |
| `hashlink`, `hashbrown`, `foldhash`, `arraydeque` | (`yaml-rust2`) | Its dependencies |

The boot runtime (`zbm` and the crates it uses) has no third-party dependency.

## Developer tooling only

These are used by `xtask` alone and never end up in an image or a host tool.

| Crate | Used by | Why |
| --- | --- | --- |
| `syn`, `proc-macro2`, `unicode-ident` | `xtask` | Parse Rust source for `cargo xtask metrics` |
| `serde_json` | `xtask` | Read `cargo metadata` and clippy's JSON messages, write `target/metrics.json` |
| `serde_core`, `itoa`, `memchr`, `zmij` | (`serde_json`) | Its dependencies |

## Vendored but never built

`serde_json` names `serde`, and `serde_core` names `serde_derive`, as dependencies for a target that never matches
(`cfg(any())`), to keep the versions of the serde crates in step. Cargo records them in `Cargo.lock` and therefore
vendors them, with what `serde_derive` needs: `quote` and `syn` 3. None of them is compiled.
