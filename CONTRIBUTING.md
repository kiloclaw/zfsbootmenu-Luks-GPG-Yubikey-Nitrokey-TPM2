# Contributing

This repository is a fork of [ZFSBootMenu](https://github.com/zbm-dev/zfsbootmenu) that adds LUKS support (passphrases,
key files, GnuPG, YubiKey, Nitrokey, FIDO2 and TPM2) and is being ported to Rust. Changes that are not specific to this
fork are better sent upstream.

## Repository layout

| Path | Contents |
| --- | --- |
| `legacy/zfsbootmenu/` | The bootloader runtime (Bash). Installed as `/usr/share/zfsbootmenu`. |
| `legacy/bin/` | The legacy `generate-zbm` (Perl) and `zbm-kcl` (Bash), which the Rust ones replace; `make install TOOLS=legacy` still installs them. |
| `legacy/cpanfile` | Perl modules that the legacy `generate-zbm` needs; builds no longer need them. |
| `crates/` | Rust crates of the port, one Cargo workspace (`Cargo.toml` at the root): `generate-zbm` and `zbm-kcl` (host tools), `zbm` (the runtime program, still without commands) and the libraries they share. |
| `xtask/` | Developer automation in Rust, run as `cargo xtask` (lints, tests, dependency checks, code metrics); see `docs/dev/tooling.md`. |
| `vendor/` | The crates the workspace depends on, vendored so that builds need no network (`.cargo/config.toml`). |
| `packaging/dracut/`, `packaging/initcpio/` | The dracut module and the mkinitcpio hooks. |
| `packaging/container/` | The build container and its helpers (`build-init.sh`, `image-build.sh`, `zbm-builder.sh`). |
| `config/zfsbootmenu/` | Default configuration installed in `/etc/zfsbootmenu`, and the release and recovery dracut snippets. |
| `config/zbm-builder/` | Default configuration of the build container. |
| `contrib/`, `examples/` | Hooks, helper scripts and example configuration files. |
| `scripts/` | Development tools; `scripts/dev/` holds the checks and comparisons described below, and `scripts/release/` the release tools and signing keys. |
| `tests/` | Test suites (see [Testing](#testing)). |
| `docs/` | Sphinx documentation, manual pages, the Gentoo guides (`docs/guides/gentoo/`) and the developer documentation (`docs/dev/`). |
| `assets/` | Logos, screenshots and the splash image. |

Run every command below from the root of the repository unless it says otherwise.

## Building

`make` builds the Rust programs (`zbm`, `generate-zbm` and `zbm-kcl`) offline from the vendored crates, and
`make install` installs them with the runtime, the dracut module and the mkinitcpio hooks. `zbm` goes to
`/usr/libexec/zfsbootmenu`, where the dracut module and the mkinitcpio hook find it; images include it, but the legacy
runtime they boot does not use it yet (`zfsbootmenu_runtime`, `zfsbootmenu_zbm` in `zfsbootmenu(7)`).

```sh
make
make install DESTDIR=/tmp/zbm-root     # TOOLS=legacy: the Perl and Bash tools, without Rust
```

Build boot images in the build container (Ubuntu 26.10, Linux 7.3, OpenZFS 2.4.4):

```sh
podman build -t localhost/zbm-builder packaging/container     # docker build works too
mkdir -p /tmp/zbm-build
packaging/container/zbm-builder.sh -b /tmp/zbm-build -l "$PWD"  # images from this checkout, in /tmp/zbm-build/build
```

The container builds `zbm` and the Rust `generate-zbm` from the checkout; `-g perl` builds with the Perl one.

Build and check the Rust workspace. `rust-toolchain.toml` selects the compiler; the oldest supported one is
`rust-version` in `Cargo.toml`. The parity tests run the legacy code they compare with, so they need Bash, Perl with
`YAML::PP`, `Sort::Versions` and `boolean`, binutils and the systemd EFI stub (`systemd-boot-efi`); tests skip
what they cannot run.

```sh
cargo xtask lint      # rustfmt, line widths, clippy, strict lints, rustdoc, dependency layers, code metrics
cargo xtask test      # every test that needs neither root nor a VM (cargo nextest when installed)
cargo xtask bless     # after a deliberate change to a screen: accept the new snapshots, then review them
cargo xtask record-parity   # after changing a parity test's cases: record the legacy results again
cargo +1.88 check --workspace --all-targets --locked
cargo deny check      # after changing a dependency: licences, advisories, banned crates (deny.toml)
```

`cargo xtask help` lists the other commands, among them `coverage`, and [docs/dev/tooling.md](docs/dev/tooling.md)
explains each check.

Generated inputs of the parity tests come from a seeded generator; set `ZBM_TEST_SEED` to try other inputs, and
quote the seed a failing test prints to reproduce it.

## Testing

| Suite | Command | Needs |
| --- | --- | --- |
| LUKS unit tests | `tests/luks/unit/run-all.sh` | root, cryptsetup, swtpm, clevis, GnuPG |
| LUKS unit tests in the build container | `tests/luks/unit/run-in-container.sh` | podman or docker |
| LUKS VM tests | `cargo xtask test vm [test ...]` | root, QEMU (KVM if available), the build container image |
| Upstream test VMs | `cd tests && ./setup.sh -a && ./run.sh` | root, QEMU, ZFS; see `tests/README.md` |
| Rust | `cargo xtask test` | the Rust toolchain; Perl and binutils for the parity tests |
| Perl and Rust `generate-zbm` give the same images | `scripts/dev/compare-generators.sh [release recovery builder]` | podman or docker, the build container image |

`docs/luks/testing.rst` describes every LUKS test. Run the suites that cover the code you changed, and say in the pull
request which ones you ran.

The LUKS unit suites load the code under test through `tests/luks/unit/impl.sh`: `ZBMTEST_IMPL=legacy` (the default)
tests the Bash libraries, and `ZBMTEST_IMPL=rust` will test the Rust runtime; until it exists, the suites skip.

`zbm-vmtest run runtime-record-fixtures` records the output of `zfs`, `zpool` and `/proc/self/mounts` of OpenZFS
2.4.4 in a VM of the LUKS VM tests into `crates/zbm-zfs/tests/fixtures/`; the Rust tests parse those files.
`scripts/dev/compare-images.sh` compares two directories of images by their unpacked contents.

## Porting to Rust

The Bash and Perl code in `legacy/` is what ships, and it stays the reference until a Rust replacement matches it.

- Follow [PORT-ROADMAP.md](PORT-ROADMAP.md): the host tools (`zbm-kcl`, `generate-zbm`) replace the legacy ones one
  at a time, and the boot runtime is written as a whole, next to the legacy runtime, which stays the default until the
  Rust runtime has passed a beta. Keep the legacy code working until then.
- The existing suites decide whether a Rust part behaves like the code it replaces. Add tests to them for behaviour
  that is not tested yet, and run them against the legacy code first.
- Every change to Rust code comes with Rust unit tests in the same pull request: parsers tested against recorded tool
  output, parity tests against the Bash functions they replace, and fakes instead of real programs and devices. The
  "Rust unit tests" section of [PORT-ROADMAP.md](PORT-ROADMAP.md) describes what they cover.
- Runtime code follows [crates/DESIGN.md](crates/DESIGN.md): it reaches the system only through the interfaces of
  `zbm-platform`, shares state between processes only through `zbm-state`, and is tested with the fakes in
  `zbm-testkit::fakes`. Secrets are `Secret` values and reach programs only on standard input. When the code needs
  to depart from the design, change the design document in the same pull request.
- The legacy runtime (`legacy/zfsbootmenu/` and the dracut and mkinitcpio files in `packaging/`) is frozen: it gets
  bug fixes only, each with a test. Record every change in `legacy/RUNTIME-CHANGES.md`, so the Rust runtime ports it;
  `scripts/dev/legacy-runtime-changes.sh` lists the commits since the freeze.
- Keep crates free of `unsafe` unless they need FFI; the workspace denies `unsafe_code` by default, and `zbm-sys` is
  the only crate that allows it.
- Features beyond parity are listed in [ROADMAP.md](ROADMAP.md). Update an entry's status when you start or finish
  it.

## Style

- Shell: 2-space indentation, `shellcheck -S warning` clean. CI runs ShellCheck on every script.
- Perl: `perltidy` with `.perltidyrc`.
- Rust: follow [The Rust Style Guide](https://doc.rust-lang.org/stable/style-guide/) strictly. `rustfmt` with its
  default settings applies it (`rustfmt.toml` only selects the 2024 style edition); the line widths it leaves to
  you are checked by `cargo xtask widths`: at most 100 characters, and comment lines at most 80 without their
  indentation. Code must also be `clippy` clean; `cargo xtask lint` runs every check.
- Documentation: reStructuredText, lines of at most 120 characters. `docs/README.md` explains how to build it; it must
  build without warnings (`make -C docs html SPHINXOPTS=-W`).
- `.editorconfig` sets indentation and line endings for editors that support it.
- Generated files are committed: regenerate `legacy/zfsbootmenu/help-files/` with `scripts/rst2help.sh` and
  `docs/man/dist/` with `make -C docs gen-man` when you change the pages they come from.

## Pull requests

- Keep each pull request to one change, and explain why it is needed.
- Update the documentation and add an entry under "Unreleased" in `CHANGELOG.md` when users would notice the change.
- Report security problems privately; see [SECURITY.md](SECURITY.md).

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
