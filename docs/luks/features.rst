New Features
============

.. contents:: Contents
  :depth: 2
  :local:
  :backlinks: none

This build of ZFSBootMenu adds support for ZFS pools whose vdevs are stored on LUKS (dm-crypt) volumes, together with
hardware-backed ways of unlocking them. This page summarizes what was added. The other pages in this section explain
how to use it:

* :doc:`/general/luks` explains how unlocking works and how to configure it;
* :doc:`vdev-configurations` walks through creating pools of every common layout on LUKS, step by step;
* :doc:`administration` covers key management and day-to-day administration;
* :doc:`tools` lists every command, option and file.

The reference manual pages are :doc:`zfsbootmenu-luks(7) </man/zfsbootmenu-luks.7>`,
:doc:`zfsbootmenu-crypttab(5) </man/zfsbootmenu-crypttab.5>` and :doc:`zluks(8) </man/zluks.8>`.

Feature Summary
---------------

Pools on LUKS volumes
  ZFSBootMenu unlocks LUKS volumes after early-setup hooks and before it imports pools, so pools of any layout on LUKS
  are imported and booted like any other. Before it hands over to the selected boot environment, it exports those pools
  and closes the volumes, which wipes the volume keys from memory.

Three ways to choose volumes
  The ``zbm.luks`` kernel option, a crypttab embedded at build time (``/etc/zfsbootmenu/crypttab``), or automatic
  discovery of every LUKS volume. Discovered volumes that turn out not to hold ZFS are closed again.

One prompt per pool
  Within one unlock run, secrets are reused across volumes, so the members of a mirror or RAIDZ vdev ask only once.
  This covers passphrases, YubiKey responses, TPM2 PINs, and GnuPG key files, which are decrypted only once.

Key files and key scripts
  Key files in the image or on a separate device such as a USB stick (``path:device``), and Debian-style key scripts.
  Plain key files are never embedded in the image.

TPM 2.0
  LUKS2 tokens from ``systemd-cryptenroll``, with or without a PIN, optionally bound to PCRs, and clevis TPM2 policies.
  systemd's cryptsetup plugin is used when available; otherwise ZFSBootMenu unseals the token itself with ``tpm2-tools``.
  It detects the TPM's dictionary-attack lockout instead of asking for PINs it cannot use.

FIDO2 security keys
  YubiKey 5, Nitrokey 3 and other keys with the *hmac-secret* extension, enrolled with ``systemd-cryptenroll``. Uses
  systemd's plugin when available, otherwise ``fido2-assert``.

OpenPGP smart cards and GnuPG
  Key files encrypted to an OpenPGP key on a YubiKey or Nitrokey, or with a passphrase. The card PIN is asked for only
  once the card is known to hold the right key, and the number of attempts left is shown.

YubiKey challenge-response
  HMAC-SHA1 challenge-response compatible with ``yubikey-luks``, including its hash and concatenate modes.

The ``zluks`` command
  Lists, unlocks, and closes LUKS volumes from the recovery shell or over SSH. See :doc:`zluks(8) </man/zluks.8>`.

Late devices
  Configured devices are waited for. Disks that appear even later are unlocked during the pool-import retry loop, and
  volumes the user skipped are not offered again. Key devices such as USB sticks are waited for, once per boot, for
  ``zbm.luks_token_wait`` seconds.

No crypttab needed
  ``zbm.luks=`` and ``zbm.luks_key=`` on ZFSBootMenu's command line replace its crypttab, and systemd's ``luks.*``
  parameters, passed through ``org.zfsbootmenu:commandline``, replace ``/etc/crypttab`` in the boot environment.
  Activation flags such as TRIM stored in the LUKS2 header with ``cryptsetup open --persistent`` are honoured.

Tolerant configuration
  crypttab files may use CRLF line endings and lack a final newline; ``UUID=`` and ``PARTUUID=`` match in either case;
  invalid numeric options are ignored with a warning instead of making every unlock attempt fail.

Build integration
  The dracut module and the mkinitcpio hook install ``cryptsetup``, device-mapper udev rules, dm-crypt and crypto
  modules, TPM/USB/HID drivers, and, when installed on the build host, systemd's token plugins, ``tpm2-tools``,
  ``fido2-tools``, clevis, GnuPG and ``ykchalresp``. Each part can be switched off. The dracut ``crypt`` module is left
  out and ``rd.luks=0`` is embedded so that dracut never prompts on its own.

Supported vdev Configurations
-----------------------------

Every vdev class can live on LUKS, on whole disks or partitions, alone or mixed with unencrypted vdevs:

==========================================  ======================================================================
Configuration                               Notes
==========================================  ======================================================================
Single disk (stripe)                        no redundancy
Mirror (2-way, 3-way, ...)                  most common for boot pools
Striped mirrors ("RAID 10")                 several mirror vdevs in one pool
RAIDZ1, RAIDZ2, RAIDZ3                      RAIDZ expansion needs OpenZFS 2.3 or later
dRAID                                       needs OpenZFS 2.1 or later
Special and dedup allocation classes        should be mirrored; holds metadata and small blocks
Log (SLOG)                                  must be encrypted: holds plaintext of synchronous writes
Cache (L2ARC)                               must be encrypted; found by path (see below)
Hot spares                                  found by path (see below)
ZFS native encryption on top of LUKS        both layers work together
==========================================  ======================================================================

Data, special, dedup and log vdevs are found by their ZFS labels whatever their LUKS mappings are called. Cache and
spare vdevs are found by the device path recorded in the pool. They are available in ZFSBootMenu only when their
mappings carry the same names as on the running system; ``luks-<UUID>`` is the default for both. See
:doc:`vdev-configurations`.

Unlock Methods
--------------

=============================  ==========================  ========================  ===================================
Method                         Set up with                 Interaction at boot       Implementation
=============================  ==========================  ========================  ===================================
Passphrase                     ``cryptsetup``              passphrase                cryptsetup
Key file / key on USB stick    ``cryptsetup luksAddKey``   none                      cryptsetup
Key script                     crypttab ``keyscript=``     script-defined            script
TPM2, no PIN                   ``systemd-cryptenroll``     none                      systemd plugin, or ``tpm2-tools``
TPM2 with PIN                  ``systemd-cryptenroll``     PIN                       systemd plugin, or ``tpm2-tools``
TPM2 via clevis                ``clevis luks bind``        none                      clevis
FIDO2 security key             ``systemd-cryptenroll``     PIN (optional), touch     systemd plugin, or ``fido2-assert``
OpenPGP smart card             ``gpg --encrypt``           card PIN, touch           GnuPG ``scdaemon``
GnuPG passphrase               ``gpg --symmetric``         passphrase                GnuPG
YubiKey challenge-response     ``ykpersonalize``           passphrase, touch         ``ykchalresp``
=============================  ==========================  ========================  ===================================

Requirements
------------

* ``cryptsetup`` 2.4 or later for LUKS2 tokens; LUKS1 volumes support passphrases, key files and key scripts.
* For TPM2 tokens without systemd's plugin: tokens created by systemd 254 or later, ``tpm2-tools`` and ``openssl``.
* For FIDO2 tokens without systemd's plugin: ``fido2-tools`` (libfido2).
* For OpenPGP cards: GnuPG 2.2 or later with ``scdaemon``.
* The ZFS version in the ZFSBootMenu image must support every feature enabled on the pools.

Other Changes in This Build
---------------------------

* The build container (``releng/docker``) uses the Void Linux ``linux7.2`` kernel series, which needs OpenZFS 2.4.4 or
  later, and ``etc/zbm-builder/config.yaml`` selects it with ``Kernel.Version: 7.2.*``. The container also installs
  GnuPG with ``scdaemon``, libfido2, clevis, tpm2-tools, ykpers and openssl. Void has no systemd, so container builds
  use ZFSBootMenu's own TPM2 and FIDO2 code.
* An Ubuntu-based build container (``releng/docker/Dockerfile.ubuntu``) uses Ubuntu 26.10's kernel (Linux 7.3) and
  ZFS module (OpenZFS 2.4.4) and includes systemd's cryptsetup token plugins. The same entrypoint serves both
  containers; see :doc:`/general/container-building`.
* The build container entrypoint always builds generic images (dracut ``hostonly="no"``). Ubuntu's dracut is
  host-only by default, which left the ZFS and dm-crypt modules out of container builds.
* The build containers start again: their entrypoint was declared as ``/${ZBM_BUILDER}``, which is not expanded in
  that form of ``ENTRYPOINT``.
* The mkinitcpio hook no longer fails the build when an optional kernel module does not exist.
* ``make install`` no longer fails when the version is taken from a branch name containing ``/``, which happens in
  clones without release tags.
* Documentation is built with Sphinx 9.1 and ``myst-parser``. ``docutils`` is held below 0.22 because 0.22 and 0.23
  render cross-references in man pages incorrectly.
* CI uses ``actions/checkout@v7``, ``actions/upload-artifact@v7`` and ``ludeeus/action-shellcheck@2.0.0``.

Known Limitations
-----------------

* The boot environment must unlock its LUKS volumes again after ``kexec``; see :doc:`/general/luks`.
* Each FIDO2 credential belongs to one volume, so a pool of *n* volumes enrolled with one security key needs *n*
  touches.
* The native TPM2 code does not support signed PCR policies, ``--tpm2-pcrlock`` or tokens created before systemd 254.
  Images built in the Void container therefore cannot unlock such tokens; use the Ubuntu container or build on
  another systemd distribution instead.
* Cache and spare vdevs on LUKS need matching mapping names, as described above.
* Pools are exported and volumes closed before ``kexec``. If ``kexec`` itself fails afterwards, which is rare, use
  ``zluks`` and ``zpool import`` in the recovery shell to continue.

Validation Status
-----------------

The following was tested, in virtual machines running this ZFSBootMenu build and in scripted tests on real LUKS
volumes:

* Pools: a RAIDZ2 data vdev with special and log mirrors, a cache device and a hot spare, all on LUKS, plus a mirror
  mixing a whole-disk LUKS volume with a plain partition, and ZFS native encryption on top. Tested healthy, with two
  disks missing (degraded), with disks hot-plugged while ZFSBootMenu waited, and during the import retry loop.
* Root mount failures: a wrong ZFS key leads to the recovery shell, with volumes and pools still available.
* TPM2 with and without a PIN (systemd plugin and native code), PCR policies, lockout handling, clevis TPM2, and
  passphrase-protected GnuPG key files, against a software TPM.
* GnuPG key files encrypted to an OpenPGP key, FIDO2 and YubiKey challenge-response, against software stand-ins.
* The procedures in :doc:`vdev-configurations` and :doc:`administration` (creating every pool layout, adding special,
  log, cache and spare vdevs, replacing and attaching disks), run as written, followed by a boot through ZFSBootMenu.
* A single-disk NVMe installation in a UEFI virtual machine with an emulated NVMe SSD: switching to 4 KiB sectors,
  LUKS2 with 4 KiB sectors and TRIM and work-queue flags stored in the header, unlocking without any crypttab
  (``zbm.luks=`` in ZFSBootMenu, systemd's ``luks.*`` parameters in the boot environment), boot entries created with
  ``efibootmgr``, and a reboot through the firmware: one passphrase prompt, none in the boot environment. A key file
  on a FAT USB stick unlocked without a prompt, and without the stick ZFSBootMenu waited, then asked for the
  passphrase.
* The Gentoo installation guide ``Gentoo Linux ZFS LUKS-V4.md`` on a real Gentoo stage3 (systemd profile,
  ``gentoo-kernel-bin`` 6.18, systemd 261) in a UEFI virtual machine with an emulated NVMe SSD: from partitioning to
  the first boot through the firmware, ZFSBootMenu and Gentoo's own systemd initramfs, with one passphrase prompt, no
  crypttab, and no failed systemd units.
* Images built by the Ubuntu 26.10 build container (Linux 7.3, OpenZFS 2.4.4): the pool tests above (healthy, with
  matching mapping names, degraded, hot-plugged disks, a wrong ZFS key), TPM2 with a PIN through both systemd's plugin
  and the native code together with a GnuPG key file, and its EFI image booted through the firmware on the Gentoo
  NVMe installation described above.
* Edge cases in scripted tests: crypttab files with CRLF line endings, without a final newline, with tabs and
  comments; upper-case UUIDs; the same device listed twice or under two names; mapping-name clashes; unknown device
  specifications and non-LUKS devices; invalid numeric options; passphrases with spaces, quotes, backslashes, a
  leading dash, non-ASCII characters and 400 characters; empty, missing and directory key files; key files inside a
  larger file (``keyfile-offset``, ``keyfile-size``); a USB key stick arriving late, absent, or missing its key file.

Not yet tested on real hardware: YubiKey, Nitrokey and other FIDO2 or OpenPGP devices. Please report problems.
