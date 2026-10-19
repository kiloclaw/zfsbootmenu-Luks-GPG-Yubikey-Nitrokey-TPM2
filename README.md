[![ZFSBootMenu Logo](media/logos/Logo_TextOnly_Color.svg)](https://zfsbootmenu.org)

[![Build Check](https://github.com/zbm-dev/zfsbootmenu/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/zbm-dev/zfsbootmenu/actions/workflows/build.yml) [![Documentation Status](https://readthedocs.org/projects/zfsbootmenu/badge/?version=latest)](https://docs.zfsbootmenu.org/en/latest/?badge=latest) [![Latest Packaged Version(s)](https://repology.org/badge/latest-versions/zfsbootmenu.svg)](https://repology.org/project/zfsbootmenu/versions)

ZFSBootMenu is a Linux bootloader that attempts to provide an experience similar to FreeBSD's bootloader. By taking advantage of ZFS features, it allows a user to have multiple "boot environments" (with different distributions, for example), manipulate snapshots before booting, and, for the adventurous user, even bootstrap a system installation via `zfs recv`.

In essence, ZFSBootMenu is a small, self-contained Linux system that knows how to find other Linux kernels and initramfs images within ZFS filesystems. When a suitable kernel and initramfs are identified (either through an automatic process or direct user selection), ZFSBootMenu launches that kernel using the `kexec` command.

![screenshot](/media/v2.3.0-multi-be.png)

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

The build container (`releng/docker`) is pinned to the Void Linux kernel package `linux7.2-7.2.11_1` (Linux 7.2.11,
which needs OpenZFS 2.4.4 or later). Run `releng/docker/cache-kernel.sh` while that version is current to keep a
verified local copy, and build images with the locally built container (`zbm-builder.sh -i ...`); see
[releng/docker/README.md](releng/docker/README.md).

See [ZFS Pools on LUKS Volumes](docs/general/luks.rst) for details, including how to avoid a second passphrase prompt in
the booted system.

### For more details, see:

- [Documentation](https://docs.zfsbootmenu.org)
- [Boot Environments and You: A Primer](https://docs.zfsbootmenu.org/en/latest/general/bootenvs-and-you.html)

### Join us on IRC

Come chat about ZFSBootMenu in [#zfsbootmenu on libera.chat](https://web.libera.chat/#zfsbootmenu)
