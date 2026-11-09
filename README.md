[![ZFSBootMenu Logo](assets/logos/Logo_TextOnly_Color.svg)](https://zfsbootmenu.org)

[![Build Check](https://github.com/zbm-dev/zfsbootmenu/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/zbm-dev/zfsbootmenu/actions/workflows/build.yml) [![Documentation Status](https://readthedocs.org/projects/zfsbootmenu/badge/?version=latest)](https://docs.zfsbootmenu.org/en/latest/?badge=latest) [![Latest Packaged Version(s)](https://repology.org/badge/latest-versions/zfsbootmenu.svg)](https://repology.org/project/zfsbootmenu/versions)

ZFSBootMenu is a Linux bootloader that attempts to provide an experience similar to FreeBSD's bootloader. By taking advantage of ZFS features, it allows a user to have multiple "boot environments" (with different distributions, for example), manipulate snapshots before booting, and, for the adventurous user, even bootstrap a system installation via `zfs recv`.

In essence, ZFSBootMenu is a small, self-contained Linux system that knows how to find other Linux kernels and initramfs images within ZFS filesystems. When a suitable kernel and initramfs are identified (either through an automatic process or direct user selection), ZFSBootMenu launches that kernel using the `kexec` command.

### The new menu

The Rust runtime, which will replace the shell one, has a new menu: a workspace of four tabs (Boot, Snapshots, Pools
and Log), with columns that drill down from a pool to its environments and their kernels or snapshots, an inspector,
and a list of what needs attention. Anything that cannot be undone opens a review page first. The same menu runs on a
serial console at 80×24, in ASCII, over SSH, and as lines for screen readers; the `:` key opens a palette of
commands. Build an image with `zfsbootmenu_runtime=rust` to use it; the shell runtime, with its `fzf` menu, stays the
default until then.

![The Boot tab: pools, LUKS volumes and what needs attention; the boot environments of zroot; the kernels of the
selected one; and the inspector with its checks, command line and next steps](assets/menu/boot.png)

*The Boot tab, counting down to the default environment.*

![The Snapshots tab: the snapshots of zroot/ROOT/void by week, earlier months folded, with their age and size
](assets/menu/snapshots.png)

*The Snapshots tab: a timeline that folds earlier months; a snapshot can be diffed, restored from, cloned or rolled
back.*

![A review page before a rollback: the snapshots and data that are lost, the zfs command that runs, the safer
alternative, and the name to type to confirm](assets/menu/rollback-review.png)

*A rollback's review page: what is lost, the command that runs, and a safer alternative.*

These are the menu's own screens of its demo machine (`zbm-tui --demo`), drawn at 120×40 in the font and colours of
the console on the display.

### ZFS pools on LUKS volumes

This build of ZFSBootMenu can unlock LUKS (dm-crypt) volumes that hold ZFS vdevs before importing pools:

- With no configuration, every LUKS volume is offered for unlocking. A passphrase that opens one volume is reused for the
  others, so a mirror on two LUKS disks needs a single passphrase.
- Volumes, key files, key devices (e.g. a USB stick) and key scripts can be configured with the `zbm.luks` and
  `zbm.luks_key` kernel options or with a crypttab embedded at build time (`/etc/zfsbootmenu/crypttab`; see
  [`examples/crypttab`](examples/crypttab)).
- Before booting the selected environment, ZFSBootMenu exports the affected pools and closes the LUKS mappings, which
  wipes the volume keys from memory.
- `zluks` in the recovery shell (or over SSH) unlocks, closes and lists volumes.
- Hardware keys:
  - **TPM2** tokens from `systemd-cryptenroll` (with or without a PIN, optionally bound to PCRs) and clevis tokens;
  - **FIDO2** security keys such as YubiKey and Nitrokey 3, enrolled with `systemd-cryptenroll`;
  - **GnuPG**-encrypted key files, protected by an OpenPGP smart card (YubiKey, Nitrokey) or a passphrase;
  - **YubiKey challenge-response**, compatible with `yubikey-luks`.

  systemd's cryptsetup plugins are used when available; otherwise ZFSBootMenu unlocks TPM2 and FIDO2 tokens with
  `tpm2-tools` and `fido2-assert`, so images built without systemd support every method too.

The build container (`packaging/container`) is based on Ubuntu 26.10 and builds images with Ubuntu's Linux 7.3 kernel
and its OpenZFS 2.4.4 module, selected by `Kernel.Version: 7.3.*` in `config/zbm-builder/config.yaml`. It also includes
systemd's cryptsetup token plugins. The image is not published; build it with `podman build -t localhost/zbm-builder
packaging/container` (or `docker build`), which is the default image of `packaging/container/zbm-builder.sh`. See
[ZFSBootMenu Build Containers](docs/general/container-building.rst).

See [ZFS Pools on LUKS Volumes](docs/general/luks.rst) for details, including how to avoid a second passphrase prompt in
the booted system.

### Repository layout

| Path | Contents |
| --- | --- |
| `legacy/zfsbootmenu/` | The bootloader runtime, written in Bash: init hooks, libraries and help pages. Installed as `/usr/share/zfsbootmenu`. |
| `crates/` | Rust crates for the port of ZFSBootMenu to Rust, in the Cargo workspace defined by `Cargo.toml`: `generate-zbm`, `zbm-kcl`, the runtime program `zbm` and their libraries. Their dependencies are vendored in `vendor/`. |
| `packaging/dracut/` | The dracut module (`90zfsbootmenu`). |
| `packaging/initcpio/` | The mkinitcpio hooks and install scripts. |
| `packaging/container/` | The build container: `Dockerfile`, its entrypoint `build-init.sh`, `image-build.sh` and `zbm-builder.sh`. |
| `config/` | Default configuration, installed in `/etc/zfsbootmenu` (`config/zfsbootmenu/`) or used by the build container (`config/zbm-builder/`). |
| `contrib/`, `examples/` | `generate-zbm` hooks, helper scripts and example configuration files, installed as examples. |
| `scripts/` | Development tools (`install-tree.sh`, `version.sh`, `rst2help.sh`, `update-includes.sh`), checks and image comparisons in `scripts/dev/`, release tools and signing keys in `scripts/release/`. |
| `tests/` | Test suites: `tests/luks/` (unit and VM tests), `tests/validation/` and the test VMs created by `tests/setup.sh`. |
| `docs/` | Sphinx documentation and manual pages. Gentoo installation guides are in `docs/guides/gentoo/`. |
| `assets/` | Logos, screenshots and the splash image. |

The boot runtime that ships by default is the Bash code under `legacy/zfsbootmenu/`; the host tools `generate-zbm` and
`zbm-kcl` are the Rust ones in `crates/`, which replaced the Perl and Bash tools and build the same images. The Rust
runtime, `zbm` with the new menu, is chosen with `zfsbootmenu_runtime=rust` and will replace the Bash one as a whole,
after a beta. `make`
builds the Rust programs, and `make install` installs them with files from `legacy/zfsbootmenu/`, `packaging/dracut/`,
`packaging/initcpio/`, `config/zfsbootmenu/`, `contrib/`, `examples/` and `docs/man/dist/`, plus `assets/splash.bmp`.
See [CONTRIBUTING.md](CONTRIBUTING.md) for how to build and test
each part, and [ROADMAP.md](ROADMAP.md) for the target architecture and the work left, features included.
[Building ZFSBootMenu](docs/general/building.rst) walks through building and installing your own image, in the build
container or on the host, with LUKS and hardware keys.

### For more details, see:

- [Documentation](https://docs.zfsbootmenu.org)
- [Boot Environments and You: A Primer](https://docs.zfsbootmenu.org/en/latest/general/bootenvs-and-you.html)

### Join us on IRC

Come chat about ZFSBootMenu in [#zfsbootmenu on libera.chat](https://web.libera.chat/#zfsbootmenu)
