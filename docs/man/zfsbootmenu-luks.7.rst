================
zfsbootmenu-luks
================

SYNOPSIS
========

ZFSBootMenu kernel command line::

  zbm.luks=off|auto|device[,device...]
  zbm.luks_key=path[:device]
  zbm.luks_tokens=0|1
  zbm.luks_token_wait=seconds

Build options (dracut configuration or ZFSBootMenu *mkinitcpio.conf*)::

  zfsbootmenu_luks=yes|no
  zfsbootmenu_luks_crypttab=path
  zfsbootmenu_luks_tokens=yes|no
  zfsbootmenu_luks_token_plugins=yes|no
  zfsbootmenu_luks_clevis=yes|no
  zfsbootmenu_luks_gpg=yes|no
  zfsbootmenu_luks_gpg_keys="keyid ..."
  zfsbootmenu_luks_gpg_pubkeys="file ..."
  zfsbootmenu_luks_pcscd=yes|no
  zfsbootmenu_luks_yubikey=yes|no

DESCRIPTION
===========

ZFSBootMenu can import ZFS pools whose vdevs are stored on LUKS (dm-crypt) volumes. Every vdev type is supported: data
vdevs of any layout (single disks, mirrors, RAIDZ, dRAID), special, dedup, log, cache and spare vdevs, on whole disks or
on partitions, alone or mixed with unencrypted vdevs. LUKS can be combined with ZFS native encryption.

During start-up, ZFSBootMenu loads the ZFS modules, configures the hostid, waits for any devices named by
**zbm.wait_for**, runs early-setup hooks, then unlocks LUKS volumes before it imports pools. Right before it starts the
selected boot environment with **kexec**\ (8), it exports the pools on those volumes and closes them, which wipes the
volume keys from kernel memory. The boot environment's own initramfs then unlocks the volumes again.

A volume can be unlocked with a passphrase, a key file (plain, on another device, or GnuPG-encrypted), a key script, a
TPM 2.0 chip, a FIDO2 security key, a clevis policy, an OpenPGP smart card, or YubiKey challenge-response. The LUKS
passphrase always remains available as a fallback.

The **zluks**\ (8) command manages LUKS volumes from the recovery shell or over SSH.

VOLUME SELECTION
================

The volumes to unlock are chosen by the first of these that applies:

1. **zbm.luks** on the kernel command line;
2. the crypttab embedded in the image, see :doc:`zfsbootmenu-crypttab(5) </man/zfsbootmenu-crypttab.5>`;
3. automatic discovery: every LUKS volume on the system.

Automatically discovered volumes are mapped as */dev/mapper/luks-*\ *UUID*. After unlocking, a discovered volume whose
contents are recognized as something other than a ZFS vdev, such as an ext4 file system or swap, is closed again.

UNLOCK SEQUENCE
===============

For each volume, ZFSBootMenu tries the following methods in order and stops at the first one that opens the volume:

1. a key script (**keyscript=**);
2. a key file, plain or GnuPG-encrypted;
3. LUKS2 tokens. Tokens that need no interaction come first: TPM2 without a PIN, then clevis. Tokens that need
   interaction follow: TPM2 with a PIN, then FIDO2 security keys;
4. passphrases, and YubiKey responses, that unlocked an earlier volume in this run;
5. YubiKey challenge-response (**ykchalresp**);
6. a passphrase prompt.

Every prompt can be answered with an empty line to move on to the next method. An empty answer at the final
passphrase prompt skips the volume.

Secrets are reused within one unlock run, so the members of a mirror or RAIDZ vdev ask only once:

* a passphrase or YubiKey response that opened one volume is tried on the others;
* a TPM2 PIN that worked is tried once on other PIN-protected TPM2 tokens;
* a GnuPG key file is decrypted only once.

All of these live in RAM only, in a subshell, and are wiped when the run ends.

When a volume is unlocked from elsewhere while a prompt is waiting, for example with **zluks** over SSH, the prompt
notices and moves on.

COMMAND-LINE PARAMETERS
=======================

These options are given on the ZFSBootMenu kernel command line, for example with **zbm-kcl**\ (8) or with
*Kernel.CommandLine* in **generate-zbm**\ (5).

**zbm.luks=off**

  Do not unlock LUKS volumes.

**zbm.luks=auto**

  Use automatic discovery, even when the image has a crypttab.

**zbm.luks=**\ *device*\ [,\ *device*...]

  Unlock only the listed devices. Each *device* takes the forms described for the *device* field in
  :doc:`zfsbootmenu-crypttab(5) </man/zfsbootmenu-crypttab.5>`.

**zbm.luks_key=**\ *path*\ [:*device*]

  A key file for every volume that has none of its own. With *device*, the key file is read from that device, which
  is mounted read-only for the purpose and unmounted again. When *device* is not present yet, for example a USB stick
  that the kernel has not finished detecting, ZFSBootMenu waits for it for **zbm.luks_token_wait** seconds. It waits
  once per device in each unlock run, and not at all during pool-import retries. Files ending in *.gpg*, *.asc* or
  *.pgp* are decrypted with GnuPG.

  A key file named only here, and not in the crypttab, is not copied into the image automatically. A GnuPG-encrypted
  key file can be added with dracut's **install_items**; see `UNLOCKING WITHOUT A CRYPTTAB`_.

**zbm.luks_tokens=0**

  Do not use LUKS2 tokens.

**zbm.luks_token_wait=**\ *seconds*

  How long to wait for a FIDO2 security key, OpenPGP smart card, YubiKey or key device to be plugged in before moving
  on. The default is 10 seconds; **0** disables waiting. Pressing any key ends the wait.

UNLOCK METHODS
==============

TPM2
----

LUKS2 tokens created with **systemd-cryptenroll --tpm2-device**, with or without **--tpm2-with-pin**, and optionally
bound to PCRs. When the image contains systemd's *systemd-tpm2* cryptsetup plugin, the plugin unlocks the token.
Otherwise ZFSBootMenu unseals it with **tpm2-tools**. The native implementation handles tokens created by systemd 254
or later, with or without a PIN and with or without a PCR policy. Signed PCR policies (**--tpm2-public-key**),
**--tpm2-pcrlock** and tokens created by older systemd versions need the plugin.

When the TPM refuses PINs after too many wrong attempts (dictionary-attack lockout), ZFSBootMenu says so and moves on
instead of asking for a PIN it cannot use.

FIDO2
-----

LUKS2 tokens created with **systemd-cryptenroll --fido2-device** for security keys with the *hmac-secret* extension,
such as YubiKey 5 and Nitrokey 3. ZFSBootMenu waits for the key, asks for its PIN if the token requires one, and asks
for a touch. Without systemd's *systemd-fido2* plugin, ZFSBootMenu computes the secret with **fido2-assert**\ (1).

clevis
------

LUKS2 tokens created with **clevis luks bind**. TPM2 policies work in every image that contains clevis. Tang policies
need networking in ZFSBootMenu.

GnuPG-encrypted key files
-------------------------

A key file encrypted with **gpg --encrypt** to an OpenPGP key, typically one stored on a YubiKey or Nitrokey, or with
**gpg --symmetric**. For keys on a card, ZFSBootMenu first reads the card and checks that it holds the key the file is
encrypted to. Only then does it ask for the card PIN, showing the number of attempts left. The public keys come from
**zfsbootmenu_luks_gpg_keys** or **zfsbootmenu_luks_gpg_pubkeys**. GnuPG's built-in CCID driver handles common
readers; **zfsbootmenu_luks_pcscd=yes** adds **pcscd**\ (8) for others.

YubiKey challenge-response
--------------------------

The scheme used by **yubikey-luks**: the typed passphrase (or its SHA-256) is sent to an HMAC-SHA1 slot, and the
response (optionally appended to the challenge) is a LUKS passphrase. It is enabled per volume with the **ykchalresp**
crypttab options.

Key files and key scripts
-------------------------

Plain key files present in the running image or on another device, and Debian-style key scripts; see
:doc:`zfsbootmenu-crypttab(5) </man/zfsbootmenu-crypttab.5>`.

LATE DEVICES
============

Devices named in the crypttab or in **zbm.luks** are waited for at start-up. Disks that appear later, such as slow USB
enclosures or some SAS controllers, are handled by the pool-import retry loop. Before each import retry, ZFSBootMenu
looks for LUKS volumes that are neither open nor skipped by the user, unlocks them, and retries the import at once.

BOOT ENVIRONMENT HAND-OFF
=========================

The system teardown hook *80-luks-close.sh* runs right before **kexec**. It exports every pool that has a vdev on a
mapping opened by ZFSBootMenu, including vdevs on partitions inside a mapping, and closes the mappings. It can be
disabled with **zbm.skip_hooks=80-luks-close.sh**.

The boot environment must unlock its LUKS volumes again from its own initramfs, for example with a key file embedded
in that initramfs. Because the initramfs is stored inside the encrypted pool, this does not expose the key.

UNLOCKING WITHOUT A CRYPTTAB
============================

Both stages of the boot can be configured entirely on kernel command lines, so that neither ZFSBootMenu nor the boot
environment reads a crypttab.

ZFSBootMenu
  Put **zbm.luks** and, if needed, **zbm.luks_key** into *Kernel.CommandLine* in **generate-zbm**\ (5). Do not create
  */etc/zfsbootmenu/crypttab*; **zbm.luks** takes precedence over it in any case. A GnuPG-encrypted key file must be
  added to the image with **install_items** in a file in *Global.DracutConfDir*. Plain key files must not be added:
  the image is stored unencrypted.

Boot environment
  An initramfs that runs systemd, as built by dracut on systemd distributions, unlocks volumes named on its kernel
  command line by **luks.uuid=**\ *UUID*, with the key file given by **luks.key=**\ *UUID*\ **=**\ *path* and
  options given by **luks.options=**\ *UUID*\ **=**\ *options*; see **systemd-cryptsetup-generator**\ (8).
  **luks.crypttab=no** makes systemd ignore */etc/crypttab*. Set these parameters in the
  *org.zfsbootmenu:commandline* property of the boot environment, or of its parent so that all boot environments
  inherit them. The initramfs must contain the key file (dracut **install_items**) and the dracut *crypt* module.
  Set **hostonly_cmdline="no"**, so that dracut does not also embed **rd.luks** parameters it detects itself.

Activation flags such as TRIM and the dm-crypt work-queue options can be stored in the LUKS2 header with
**cryptsetup open --persistent**. ZFSBootMenu, systemd and **cryptsetup open** apply stored flags at every activation,
so no crypttab options are needed for them. **cryptsetup luksDump** shows them on the *Flags* line.

BUILD OPTIONS
=============

These options are set in a dracut configuration file in the directory named by *Global.DracutConfDir* in
**generate-zbm**\ (5), or in the ZFSBootMenu *mkinitcpio.conf*.

**zfsbootmenu_luks=no**

  Leave out LUKS support. By default it is included whenever **cryptsetup** is installed on the build host.

**zfsbootmenu_luks_crypttab=**\ *path*

  The crypttab to embed. The default is */etc/zfsbootmenu/crypttab*.

**zfsbootmenu_luks_tokens=no**

  Leave out TPM2, FIDO2 and clevis support.

**zfsbootmenu_luks_token_plugins=no**

  Leave out systemd's cryptsetup token plugins; ZFSBootMenu's own TPM2 and FIDO2 code is used instead.

**zfsbootmenu_luks_clevis=no**

  Leave out clevis.

**zfsbootmenu_luks_gpg=no**

  Leave out GnuPG.

**zfsbootmenu_luks_gpg_keys=**\ "*keyid* ..."

  Export these OpenPGP public keys from the build host's keyring into the image.

**zfsbootmenu_luks_gpg_pubkeys=**\ "*file* ..."

  Import the OpenPGP public keys in these files into the image.

**zfsbootmenu_luks_pcscd=yes**

  Include **pcscd** and its card-reader drivers.

**zfsbootmenu_luks_yubikey=no**

  Leave out **ykchalresp**.

Each optional component is included only when it is installed on the build host. The image also receives
**cryptsetup**, **dmsetup**, the device-mapper udev rules, *dm-crypt* and common crypto modules, and TPM, USB and HID
drivers. The dracut *crypt* module is left out of ZFSBootMenu images, and *rd.luks=0* is embedded as a safeguard, so
that dracut does not prompt for LUKS volumes on its own.

FILES
=====

*/etc/zfsbootmenu/crypttab*

  Volumes to unlock (on the build host, and in the image).

*/etc/zfsbootmenu/gnupg/*

  Public keyring in the image.

*/lib/zfsbootmenu-luks.sh*, */lib/zfsbootmenu-luks-keys.sh*

  Unlock and teardown functions, and hardware key support.

*/libexec/init.d/45-luks-unlock*

  Start-up stage that unlocks volumes.

*/libexec/hooks/teardown.d/80-luks-close.sh*

  Teardown hook that exports pools and closes volumes before **kexec**.

*/zfsbootmenu/luks-mappings*, */zfsbootmenu/luks-skipped*

  Run-time state: mappings opened by ZFSBootMenu, and volumes skipped by the user.

*/usr/share/examples/zfsbootmenu/crypttab*

  Annotated example crypttab, installed with ZFSBootMenu.

SECURITY
========

* Plain key files are never embedded in an image, because images normally live on an unencrypted EFI system
  partition. GnuPG-encrypted key files are embedded. Those protected only by a passphrase can be attacked offline by
  anyone with the image, and the build warns about them.
* A TPM2 token without a PIN unlocks the pools for anyone who can start the computer, and ZFSBootMenu offers a
  recovery shell. Bind such tokens to PCR 7 with Secure Boot enabled, or require a PIN.
* Every vdev that stores pool data must be encrypted: data, special, dedup, log and cache vdevs, and spares. An
  unencrypted log or cache device holds plaintext data.
* Decrypted key material exists only in RAM, below */run*, and is removed when unlocking ends. GnuPG agents are stopped
  after each use.

EXAMPLES
========

Example 1: Unlocking only two devices
-------------------------------------

The following kernel command line makes ZFSBootMenu unlock only the two partitions of the boot pool, ignoring any other
LUKS volumes::

  # zbm-kcl -a 'zbm.luks=PARTLABEL=zfs0,PARTLABEL=zfs1' /boot/efi/EFI/zbm/vmlinuz.EFI

Example 2: Using a key file on a USB stick for every volume
-----------------------------------------------------------

::

  # zbm-kcl -a 'zbm.luks_key=/keys/pool.key:LABEL=KEYSTICK' /boot/efi/EFI/zbm/vmlinuz.EFI

Example 3: Disabling LUKS2 tokens for one boot
----------------------------------------------

At the ZFSBootMenu kernel command-line editor, or in the boot loader, add::

  zbm.luks_tokens=0

Example 4: Building an image with OpenPGP smart card support
------------------------------------------------------------

::

  # cat /etc/zfsbootmenu/dracut.conf.d/luks.conf
  zfsbootmenu_luks_gpg_keys="0x0123456789ABCDEF"
  # generate-zbm

Example 5: Unlocking with a GnuPG key file, without a crypttab
--------------------------------------------------------------

The key file is decrypted with the OpenPGP card that holds key *0x0123456789ABCDEF*. The LUKS passphrase remains the
fallback::

  # cat /etc/zfsbootmenu/dracut.conf.d/luks-gpg.conf
  install_items+=" /etc/zfsbootmenu/keys/zroot.key.gpg "
  zfsbootmenu_luks_gpg_keys="0x0123456789ABCDEF"
  # grep CommandLine /etc/zfsbootmenu/config.yaml
    CommandLine: "quiet zbm.luks=UUID=3f1c0b2e-8d4a-4c55-9a0e-1f2d3c4b5a69 zbm.luks_key=/etc/zfsbootmenu/keys/zroot.key.gpg"
  # generate-zbm

Example 6: Unlocking a mirror in the boot environment without a crypttab
------------------------------------------------------------------------

Each LUKS volume of the mirror gets its own **luks.uuid** and **luks.key** pair. Both volumes have a key slot for the
same key file, which is stored inside the pool and copied into the boot environment's initramfs::

  # U0=$(cryptsetup luksUUID /dev/disk/by-partlabel/zfs0)
  # U1=$(cryptsetup luksUUID /dev/disk/by-partlabel/zfs1)
  # K=/etc/cryptsetup-keys.d/zroot.key
  # zfs set org.zfsbootmenu:commandline="quiet luks.crypttab=no luks.uuid=$U0 luks.key=$U0=$K luks.uuid=$U1 luks.key=$U1=$K" zroot/ROOT
  # cat /etc/dracut.conf.d/10-zfs-luks.conf
  hostonly_cmdline="no"
  add_dracutmodules+=" zfs crypt "
  install_items+=" /etc/cryptsetup-keys.d/zroot.key "

Example 7: Storing TRIM and NVMe performance flags in the LUKS2 header
----------------------------------------------------------------------

::

  # cryptsetup open --allow-discards --perf-no_read_workqueue --perf-no_write_workqueue \
      --persistent /dev/nvme0n1p2 luks-$(cryptsetup luksUUID /dev/nvme0n1p2)
  # cryptsetup luksDump /dev/nvme0n1p2 | grep Flags
  Flags:       	allow-discards no-read-workqueue no-write-workqueue

SEE ALSO
========

:doc:`zluks(8) </man/zluks.8>`, :doc:`zfsbootmenu-crypttab(5) </man/zfsbootmenu-crypttab.5>`,
:doc:`zfsbootmenu(7) </man/zfsbootmenu.7>`, :doc:`generate-zbm(5) </man/generate-zbm.5>`,
:doc:`zbm-kcl(8) </man/zbm-kcl.8>`, **cryptsetup**\ (8), **systemd-cryptenroll**\ (1), **clevis-luks-bind**\ (1),
**gpg**\ (1), **ykpersonalize**\ (1), **systemd-cryptsetup-generator**\ (8), **zpool**\ (8), **zpoolconcepts**\ (7)
