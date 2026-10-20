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
* :doc:`tools` lists every command, option and file;
* :doc:`zfs-versions` explains which ZFS versions can import which pools;
* :doc:`testing` describes the test suites and how to run them.

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

Every token of a volume
  LUKS2 tokens are read from the volume's JSON metadata (``cryptsetup luksDump --dump-json-metadata``), so a volume
  with several tokens, such as a TPM2 token and a FIDO2 key or a clevis binding, tries each of them in turn. Reading
  the human-readable dump instead missed every token after the first with cryptsetup 2.8 and systemd 261's token
  plugins, which print those sections to standard error. For the same reason, clevis tokens are decrypted from the
  token's JWE (``jose`` and ``clevis decrypt``) rather than with ``clevis luks pass``, which also makes them work with
  detached headers.

OpenZFS 2.4.4
  Images built in the build container use OpenZFS 2.4.4 and import pools that use any OpenZFS 2.4 feature, including
  RAIDZ expansion, fast deduplication, block cloning, long names, physical rewrites and dynamic gang headers. See
  :doc:`zfs-versions`.

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
RAIDZ1, RAIDZ2, RAIDZ3                      any width
dRAID                                       needs OpenZFS 2.1 or later
Special allocation class                    should be mirrored; holds metadata and small blocks
Dedup allocation class                      should be mirrored; holds the deduplication tables
RAIDZ expansion (``zpool attach``)          needs OpenZFS 2.3 or later
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
* The ZFS version in the ZFSBootMenu image must support every feature that is active on the pools (OpenZFS 2.4.4 in
  images built in the build container); see :doc:`zfs-versions`.
* ``cryptsetup`` 2.4 or later reads token metadata as JSON; older versions fall back to parsing ``luksDump`` output.

Other Changes in This Build
---------------------------

* The build container (``releng/docker``) is based on Ubuntu 26.10 instead of Void Linux. It uses Ubuntu's kernel
  (Linux 7.3) and ZFS module (OpenZFS 2.4.4), selected by ``Kernel.Version: 7.3.*`` in
  ``etc/zbm-builder/config.yaml``, and installs GnuPG with ``scdaemon``, libfido2, clevis, tpm2-tools, ykpers, openssl
  and systemd's cryptsetup token plugins. It builds with dracut only, because Ubuntu does not package ``mkinitcpio``.
  The image is built locally (``podman build -t localhost/zbm-builder releng/docker``), and ``zbm-builder.sh`` uses it
  by default; see :doc:`/general/container-building`. CI builds the same image.
* The build container entrypoint always builds generic images (dracut ``hostonly="no"``). Ubuntu's dracut is
  host-only by default, which left the ZFS and dm-crypt modules out of container builds.
* The build container starts again: its entrypoint was declared as ``/${ZBM_BUILDER}``, which is not expanded in that
  form of ``ENTRYPOINT``.
* When ZFSBootMenu finds no boot environment, it opens an emergency shell. When that shell exits, ZFSBootMenu now
  imports the pools that became importable in it, read-only as at start-up, before it shows the menu again, so that
  ``zluks unlock`` followed by ``exit`` works there as in the recovery shell of the import loop.
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
  Images built without systemd's token plugins therefore cannot unlock such tokens; the build container includes the
  plugins.
* Cache and spare vdevs on LUKS need matching mapping names, as described above.
* Pools are exported and volumes closed before ``kexec``. If ``kexec`` itself fails afterwards, which is rare, use
  ``zluks`` and ``zpool import`` in the recovery shell to continue.

Validation Status
-----------------

The test suites in ``testing/luks`` cover every feature on this page; :doc:`testing` describes each test and lists
the latest results. In summary:

* Unit suites run the LUKS code against real LUKS1 and LUKS2 headers, a software TPM, ``systemd-cryptenroll``,
  clevis and GnuPG, with the host's tools and with those of the build container: unlocking and teardown, every key
  method, tokens, configuration parsing and edge cases, and races between sessions.
* Virtual-machine tests boot images built by the build container (Ubuntu 26.10, Linux 7.3, OpenZFS 2.4.4) into a
  boot environment that also runs OpenZFS 2.4.4: single-disk, mirror, striped-mirror, RAIDZ1-3 and dRAID pools on
  LUKS; a RAIDZ2 pool with special, dedup and log mirrors, a cache and a spare, expanded by RAIDZ expansion, with
  ZFS native encryption on top; pools mixing LUKS and plain disks or partitions; healthy, degraded and hot-plugged
  disks; wrong keys and skipped prompts (root mount failures); TPM2, clevis and GnuPG key management; a UEFI boot
  from NVMe without a crypttab, with and without a USB key stick; and the procedures of :doc:`vdev-configurations`,
  :doc:`administration` and :doc:`zfs-versions`, run as written.

Tested outside the suites, by hand:

* The Gentoo installation guide ``Gentoo Linux ZFS LUKS-V4.md`` on a real Gentoo stage3 (systemd profile,
  ``gentoo-kernel-bin`` 6.18, systemd 261) in a UEFI virtual machine with an emulated NVMe SSD: from partitioning to
  the first boot through the firmware, ZFSBootMenu and Gentoo's own systemd initramfs, with one passphrase prompt, no
  crypttab, and no failed systemd units.
* The build container image built with ``podman build``, ``docker build`` and ``image-build.sh``; the CI build steps
  and ``releng/make-binary.sh`` produced release and recovery assets; images with ``dracut-crypt-ssh``, set up as in
  :doc:`/general/container-building/example` and by ``contrib/remote-ssh-build.sh``, accepted SSH logins with the
  authorized key only.

Not yet tested on real hardware: YubiKey, Nitrokey and other FIDO2 or OpenPGP devices. Please report problems.
