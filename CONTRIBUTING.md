# Contributing

This repository is a fork of [ZFSBootMenu](https://github.com/zbm-dev/zfsbootmenu) that adds LUKS support (passphrases,
key files, GnuPG, YubiKey, Nitrokey, FIDO2 and TPM2) and is being ported to Rust. Changes that are not specific to this
fork are better sent upstream.

## Repository layout

| Path | Contents |
| --- | --- |
| `legacy/zfsbootmenu/` | The bootloader runtime (Bash). Installed as `/usr/share/zfsbootmenu`. |
| `legacy/bin/` | `generate-zbm` (Perl) and `zbm-kcl`. |
| `legacy/cpanfile` | Perl modules that `generate-zbm` needs. |
| `crates/` | Rust crates of the port, one Cargo workspace (`Cargo.toml` at the root). |
| `packaging/dracut/`, `packaging/initcpio/` | The dracut module and the mkinitcpio hooks. |
| `packaging/container/` | The build container and its helpers (`build-init.sh`, `image-build.sh`, `zbm-builder.sh`). |
| `config/zfsbootmenu/` | Default configuration installed in `/etc/zfsbootmenu`, and the release and recovery dracut snippets. |
| `config/zbm-builder/` | Default configuration of the build container. |
| `contrib/`, `examples/` | Hooks, helper scripts and example configuration files. |
| `scripts/` | Development tools. `scripts/release/` holds the release tools and signing keys. |
| `tests/` | Test suites (see [Testing](#testing)). |
| `docs/` | Sphinx documentation, manual pages and the Gentoo guides (`docs/guides/gentoo/`). |
| `assets/` | Logos, screenshots and the splash image. |

Run every command below from the root of the repository unless it says otherwise.

## Building

Install the Bash and Perl implementation:

```sh
make install DESTDIR=/tmp/zbm-root     # or: make core dracut initcpio
```

Build boot images in the build container (Ubuntu 26.10, Linux 7.3, OpenZFS 2.4.4):

```sh
podman build -t localhost/zbm-builder packaging/container     # docker build works too
mkdir -p /tmp/zbm-build
packaging/container/zbm-builder.sh -b /tmp/zbm-build -l "$PWD"  # images from this checkout, in /tmp/zbm-build/build
```

Build and check the Rust workspace. `rust-toolchain.toml` selects the compiler; the oldest supported one is
`rust-version` in `Cargo.toml`.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## Testing

| Suite | Command | Needs |
| --- | --- | --- |
| LUKS unit tests | `tests/luks/unit/run-all.sh` | root, cryptsetup, swtpm, clevis, GnuPG |
| LUKS unit tests in the build container | `tests/luks/unit/run-in-container.sh` | podman or docker |
| LUKS VM tests | `tests/luks/vm/run-all.sh [test ...]` | root, QEMU (KVM if available), the build container image |
| Upstream test VMs | `cd tests && ./setup.sh -a && ./run.sh` | root, QEMU, ZFS; see `tests/README.md` |
| Rust | `cargo test --workspace` | the Rust toolchain |

`docs/luks/testing.rst` describes every LUKS test. Run the suites that cover the code you changed, and say in the pull
request which ones you ran.

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
- Keep crates free of `unsafe` unless they need FFI; the workspace denies `unsafe_code` by default.
- Features beyond parity are listed in [ROADMAP.md](ROADMAP.md). Update an entry's status when you start or finish
  it.

## Style

- Shell: 2-space indentation, `shellcheck -S warning` clean. CI runs ShellCheck on every script.
- Perl: `perltidy` with `.perltidyrc`.
- Rust: `rustfmt` and `clippy` clean.
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
