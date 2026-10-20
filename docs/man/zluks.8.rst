=====
zluks
=====

SYNOPSIS
========

**zluks** [**unlock**]

**zluks** **open** *device* [*name*]

**zluks** **close** [*name*]

**zluks** **status**

**zluks** **list**

**zluks** **help**

DESCRIPTION
===========

**zluks** manages LUKS (dm-crypt) volumes that hold ZFS vdevs from within a running ZFSBootMenu image: from the
recovery shell, or from an SSH session when remote access is configured. It uses the same unlock procedure that
ZFSBootMenu runs at start-up, described in :doc:`zfsbootmenu-luks(7) </man/zfsbootmenu-luks.7>`, so key scripts,
key files, GnuPG-encrypted key files, LUKS2 tokens (TPM2, FIDO2 and clevis), YubiKey challenge-response and
passphrases all work the same way.

Mappings opened by **zluks** or by ZFSBootMenu itself are recorded, so that they are closed again before ZFSBootMenu
boots a boot environment. Mappings opened directly with **cryptsetup**\ (8) are not recorded and are left alone.

After unlocking volumes, type **exit** in the recovery shell to return to ZFSBootMenu, which retries the pool import,
or import pools manually with **zpool import**. This also applies to the emergency shell that ZFSBootMenu opens when it
finds no boot environment, for example because only pools with unencrypted devices could be imported: when the shell
exits, ZFSBootMenu imports the pools that became importable and shows its menu.

**zluks** is only present in images built with LUKS support (the default when **cryptsetup** is installed on the build
host).

SUBCOMMANDS
===========

**unlock**

  Unlock every LUKS volume selected by the configuration, exactly as ZFSBootMenu does at start-up: the volumes listed
  in **zbm.luks**, otherwise those in the image's crypttab, otherwise every LUKS volume on the system. Volumes that are
  already open are skipped. This is the default when no subcommand is given.

**open** *device* [*name*]

  Unlock a single LUKS volume. *device* is a path below */dev* or a device specification of the form **UUID=**,
  **PARTUUID=**, **LABEL=**, **PARTLABEL=**, **ID=** (*/dev/disk/by-id*) or **PATH=** (*/dev/disk/by-path*). A bare
  LUKS UUID, or **luks-**\ *UUID*, is also accepted.

  The mapping is created as */dev/mapper/*\ *name*. When *name* is omitted, the mapping is called **luks-**\ *UUID*,
  which is also the default name used by systemd-cryptsetup and dracut. The key file given by **zbm.luks_key**, if
  any, is tried before the volume's LUKS2 tokens and a passphrase prompt.

**close** [*name*]

  Export every imported pool that has a vdev on a mapping opened by ZFSBootMenu, then close the mapping. Data, special,
  log, cache and spare vdevs are recognized, including vdevs on partitions inside a mapping. With *name*, only that
  mapping is closed. A mapping whose pools cannot be exported stays open and is reported.

**status**

  List the mappings opened by ZFSBootMenu: mapping name, LUKS device, whether the mapping is open, and the imported
  pools that use it.

**list**

  List every LUKS volume on the system with its size and label, whether it is in use (opened by anything), and the
  types of LUKS2 tokens in its header (**systemd-tpm2**, **systemd-fido2**, **clevis** or others).

**help**, **-h**, **--help**

  Print a usage summary.

ENVIRONMENT VARIABLES
=====================

**LUKS_TPM2_TCTI**

  The TPM2 software stack TCTI used for TPM2 and clevis tokens. The default is *device:/dev/tpmrm0*.

**LUKS_NO_TOKEN_PLUGINS**

  When set to any non-empty value, ignore systemd's cryptsetup token plugins and use ZFSBootMenu's own TPM2 and FIDO2
  implementations. Useful for diagnosing plugin problems.

**LUKS_DEFAULT_TRIES**

  Passphrase attempts per round when the crypttab sets no **tries=** option. The default is 3.

**ZBM_CRYPTTAB**

  The crypttab to read. The default is */etc/zfsbootmenu/crypttab*.

FILES
=====

*/etc/zfsbootmenu/crypttab*

  Volumes to unlock, embedded in the image at build time; see
  :doc:`zfsbootmenu-crypttab(5) </man/zfsbootmenu-crypttab.5>`.

*/etc/zfsbootmenu/gnupg/*

  Public keyring used to decrypt GnuPG-encrypted key files.

*/zfsbootmenu/luks-mappings*

  Mappings opened by ZFSBootMenu: name, device and whether the volume was discovered automatically.

*/zfsbootmenu/luks-skipped*

  Volumes the user chose to skip; they are not offered again when ZFSBootMenu retries the pool import.

EXIT STATUS
===========

**0**

  Successful completion. For **unlock**, every selected volume was opened, or at least one was opened.

**1**

  An error occurred: a volume could not be unlocked, a device was not found, a mapping could not be closed,
  **cryptsetup** is not available, or the command line was invalid.

EXAMPLES
========

Example 1: Listing LUKS volumes and their tokens
-------------------------------------------------

The following command shows every LUKS volume that ZFSBootMenu can see. The first two volumes hold a mirror and are
locked; the third has a TPM2 token and is already open::

  # zluks list
  /dev/sda2 "zfs0" (931.5G)   locked   tokens: systemd-fido2
  /dev/sdb2 "zfs1" (931.5G)   locked   tokens: systemd-fido2
  /dev/nvme0n1p2 (476.9G)     in use   tokens: systemd-tpm2

Example 2: Unlocking all volumes from the recovery shell
--------------------------------------------------------

After skipping the passphrase prompt at start-up, unlock the configured volumes and return to ZFSBootMenu, which
imports the pools::

  # zluks
  Unlock LUKS volume /dev/sda2 "zfs0" (931.5G)
  Passphrase (leave empty to skip):
  # exit

Example 3: Unlocking one volume with a chosen mapping name
----------------------------------------------------------

Hot spare and cache vdevs are located by the device path recorded in the pool. To make such a vdev available, open its
LUKS volume under the same name the running system uses::

  # zluks open PARTLABEL=spare0 luks-3f1c6e0a-5b7d-4e8b-9d3e-2a6c1f0e9b42
  # zpool status zroot

Example 4: Unlocking over SSH while the console waits
-----------------------------------------------------

When ZFSBootMenu waits at a passphrase prompt on the console, an SSH session can unlock the same volume; the console
prompt notices and continues::

  $ ssh -p 222 root@zbm-host
  # zluks open UUID=0a1b2c3d-4e5f-6789-abcd-ef0123456789

Example 5: Showing and closing mappings
---------------------------------------

The following commands show which pools use the mappings opened by ZFSBootMenu, then export the pool *tank* and close
its mapping::

  # zluks status
  luks-0a1b2c3d-...   /dev/sdc2   open   pools: tank
  luks-9f8e7d6c-...   /dev/sdd2   open   pools: zroot
  # zluks close luks-0a1b2c3d-4e5f-6789-abcd-ef0123456789

SEE ALSO
========

:doc:`zfsbootmenu-luks(7) </man/zfsbootmenu-luks.7>`, :doc:`zfsbootmenu-crypttab(5) </man/zfsbootmenu-crypttab.5>`,
:doc:`zfsbootmenu(7) </man/zfsbootmenu.7>`, **cryptsetup**\ (8), **systemd-cryptenroll**\ (1), **zpool**\ (8)
