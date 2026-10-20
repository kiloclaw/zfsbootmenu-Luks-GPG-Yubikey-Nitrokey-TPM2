[![ZFSBootMenu Logo](assets/logos/Logo_TextOnly_Color.svg)](https://zfsbootmenu.org)

[![Build Check](https://github.com/zbm-dev/zfsbootmenu/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/zbm-dev/zfsbootmenu/actions/workflows/build.yml) [![Documentation Status](https://readthedocs.org/projects/zfsbootmenu/badge/?version=latest)](https://docs.zfsbootmenu.org/en/latest/?badge=latest) [![Latest Packaged Version(s)](https://repology.org/badge/latest-versions/zfsbootmenu.svg)](https://repology.org/project/zfsbootmenu/versions)

ZFSBootMenu is a Linux bootloader that attempts to provide an experience similar to FreeBSD's bootloader. By taking advantage of ZFS features, it allows a user to have multiple "boot environments" (with different distributions, for example), manipulate snapshots before booting, and, for the adventurous user, even bootstrap a system installation via `zfs recv`.

In essence, ZFSBootMenu is a small, self-contained Linux system that knows how to find other Linux kernels and initramfs images within ZFS filesystems. When a suitable kernel and initramfs are identified (either through an automatic process or direct user selection), ZFSBootMenu launches that kernel using the `kexec` command.

![screenshot](/assets/v2.3.0-multi-be.png)

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

### For more details, see:

- [Documentation](https://docs.zfsbootmenu.org)
- [Boot Environments and You: A Primer](https://docs.zfsbootmenu.org/en/latest/general/bootenvs-and-you.html)

### Join us on IRC

Come chat about ZFSBootMenu in [#zfsbootmenu on libera.chat](https://web.libera.chat/#zfsbootmenu)
