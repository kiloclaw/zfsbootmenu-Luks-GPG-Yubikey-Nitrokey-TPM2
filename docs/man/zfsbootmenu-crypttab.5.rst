====================
zfsbootmenu-crypttab
====================

SYNOPSIS
========

*/etc/zfsbootmenu/crypttab*

DESCRIPTION
===========

The ZFSBootMenu crypttab lists the LUKS (dm-crypt) volumes that ZFSBootMenu unlocks before it imports ZFS pools, and
how to unlock each of them. It is read on the build host when a ZFSBootMenu image is created, and embedded in the image
as */etc/zfsbootmenu/crypttab* with mode 0600. A different source file may be chosen with the
**zfsbootmenu_luks_crypttab** build option. The file is ignored for release builds.

The crypttab is optional. When it is absent or empty, ZFSBootMenu offers every LUKS volume on the system for unlocking.
The kernel command-line option **zbm.luks** overrides the crypttab; see
:doc:`zfsbootmenu-luks(7) </man/zfsbootmenu-luks.7>`.

The format follows **crypttab**\ (5) as used by systemd and Debian, so an existing */etc/crypttab* can often serve as a
starting point. Options that matter only to other crypttab consumers are accepted and ignored.

FILE FORMAT
===========

Each line describes one volume and has up to four fields separated by spaces or tabs::

  name  device  key  options

Empty lines and lines starting with **#** are ignored. Lines may end in CR LF, as in files edited on Windows, and the
last line does not need a trailing newline.

*name*

  The name of the mapping created below */dev/mapper*. Data, special and log vdevs are found by their ZFS labels
  whatever the mapping is called. Hot spare and cache vdevs are found by the path recorded in the pool, so for those
  the name must match the mapping name used by the running system. When volumes are discovered automatically, they are
  named **luks-**\ *UUID*.

*device*

  The LUKS volume, as a path below */dev* or in one of these forms:

  ===========================  ========================================
  **UUID=**\ *uuid*            LUKS UUID or file system UUID
  **PARTUUID=**\ *uuid*        GPT partition UUID
  **LABEL=**\ *label*          LUKS2 label
  **PARTLABEL=**\ *label*      GPT partition name
  **ID=**\ *name*              entry below */dev/disk/by-id*
  **PATH=**\ *name*            entry below */dev/disk/by-path*
  ===========================  ========================================

  A bare LUKS UUID, or **luks-**\ *UUID*, is also accepted. **UUID=** and **PARTUUID=** values match in upper or
  lower case, so GPT UUIDs copied from **sgdisk**\ (8), which prints them in upper case, work unchanged. When the
  device is missing, ZFSBootMenu waits for it, checking again every **zbm.retry_delay** seconds, until it appears or
  the user presses *Escape*.

*key*

  How the volume is unlocked before LUKS2 tokens and passphrases are tried:

  **none**, **-** or empty

    No key file.

  *path*

    A key file that exists in the running ZFSBootMenu system, for example one provided by an early-setup hook. Plain
    key files are never copied into the image, because the image is normally stored unencrypted on the EFI system
    partition; the build prints a warning instead.

  *path*\ **:**\ *device*

    A key file stored on another device, such as a USB stick. *device* takes the same forms as the *device* field. The
    device is mounted read-only, *path* is read relative to its root, and the device is unmounted again. A device
    that is not present yet is waited for, for up to **zbm.luks_token_wait** seconds (default 10), once per unlock
    run; pressing any key ends the wait. The image includes the *vfat* driver for FAT-formatted sticks.

  *path*\ **.gpg**, *path*\ **.asc** or *path*\ **.pgp**

    A GnuPG-encrypted key file, decrypted with a passphrase or with an OpenPGP smart card such as a YubiKey or
    Nitrokey. Encrypted key files named in the crypttab are embedded in the image. The **gpg** option marks files with
    other names as GnuPG-encrypted.

  With **keyscript=**, this field is passed to the key script as its argument instead.

  If the key does not unlock the volume, ZFSBootMenu moves on to the remaining methods.

*options*

  A comma-separated list of the options below.

OPTIONS
=======

**discard**, **allow-discards**

  Pass TRIM requests through to the device (**cryptsetup --allow-discards**).

**readonly**, **read-only**

  Open the mapping read-only.

**tries=**\ *n*

  Passphrase attempts per round before ZFSBootMenu offers to try again or skip the volume. The default is 3; **0**
  means unlimited. An invalid value is ignored with a warning.

**noauto**

  Do not unlock the volume at start-up. It can still be opened with **zluks open**.

**nofail**

  Do not wait or prompt when the device is missing.

**keyscript=**\ *path*

  Run *path* and use its standard output as the key. A name without a slash refers to */lib/cryptsetup/scripts/*\
  *name*. The script receives the *key* field as its first argument and the environment variables **CRYPTTAB_NAME**,
  **CRYPTTAB_SOURCE**, **CRYPTTAB_KEY**, **CRYPTTAB_OPTIONS** and **CRYPTTAB_TRIED**. Key scripts named in the crypttab
  are installed in the image with their library dependencies.

**header=**\ *path*

  Use a detached LUKS header.

**keyfile-offset=**\ *n*

  Skip *n* bytes at the start of the key file.

**keyfile-size=**\ *n*

  Read at most *n* bytes of the key file.

**key-slot=**\ *n*, **keyslot=**\ *n*

  Try only key slot *n*.

  The values of **keyfile-offset**, **keyfile-size** and **key-slot** must be non-negative integers. Other values are
  ignored with a warning, because they would make every attempt fail, the passphrase included.

**gpg**

  The key file is GnuPG-encrypted, whatever its name.

**no-tokens**, **notoken**, **no-token**

  Do not try the volume's LUKS2 tokens (TPM2, FIDO2, clevis).

**ykchalresp**, **ykchalresp=**\ *slot*

  Unlock with YubiKey HMAC-SHA1 challenge-response, using *slot* 1 or 2 (default 2). The typed passphrase is the
  challenge and the response is the LUKS passphrase, as with **yubikey-luks**.

**ykchalresp-hash**

  Send the SHA-256 of the passphrase, in hexadecimal, as the challenge (**yubikey-luks** *HASH=1*). Implies
  **ykchalresp**.

**ykchalresp-concat**

  Use the challenge followed by the response as the LUKS passphrase (**yubikey-luks** *CONCATENATE=1*). Implies
  **ykchalresp**.

**no-read-workqueue**, **no-write-workqueue**, **same-cpu-crypt**, **submit-from-crypt-cpus**

  dm-crypt performance flags, passed to **cryptsetup** as **--perf-**\ *flag*.

**luks**, **luks1**, **luks2**, **initramfs**, **x-initrd.attach**, **_netdev**, **noearly**, **force**

  Accepted for compatibility with other crypttab files and ignored.

Any other option is ignored and logged.

EXAMPLES
========

Example 1: A mirror of two LUKS partitions
------------------------------------------

Both halves of the mirror share a passphrase, so ZFSBootMenu asks for it once::

  zroot-a  PARTLABEL=zfs0  none  discard
  zroot-b  PARTLABEL=zfs1  none  discard

Example 2: A RAIDZ vdev with mapping names that match the running system
------------------------------------------------------------------------

The names match those in the host's */etc/crypttab*, so the pool's hot spare is found as well::

  luks-5d0c...a1  UUID=5d0c...a1  none  discard
  luks-6e1d...b2  UUID=6e1d...b2  none  discard
  luks-7f2e...c3  UUID=7f2e...c3  none  discard
  luks-8a3f...d4  UUID=8a3f...d4  none  discard

Example 3: A key file on a USB stick
------------------------------------

The pool *tank* unlocks with a key file on a USB stick labelled KEYSTICK, and is skipped when the stick is absent::

  tank0  PARTLABEL=tank0  /keys/tank.key:LABEL=KEYSTICK  nofail
  tank1  PARTLABEL=tank1  /keys/tank.key:LABEL=KEYSTICK  nofail

Example 4: A key file protected by an OpenPGP smart card
--------------------------------------------------------

The key file is encrypted to a key on a YubiKey or Nitrokey and embedded in the image; its public key is named by the
**zfsbootmenu_luks_gpg_keys** build option. The card PIN is asked for once for both volumes::

  zroot-a  PARTLABEL=zfs0  /etc/zfsbootmenu/keys/zroot.key.gpg  discard
  zroot-b  PARTLABEL=zfs1  /etc/zfsbootmenu/keys/zroot.key.gpg  discard

Example 5: YubiKey challenge-response
-------------------------------------

Volumes enrolled with **yubikey-luks** using slot 2 and *HASH=1*::

  zroot-a  PARTLABEL=zfs0  none  discard,ykchalresp=2,ykchalresp-hash

Example 6: Volumes with TPM2 or FIDO2 tokens
--------------------------------------------

LUKS2 tokens enrolled with **systemd-cryptenroll** or **clevis** need no options; ZFSBootMenu finds them in the LUKS2
header. To keep a token from being used on one volume::

  backup0  PARTLABEL=backup0  none  no-tokens,noauto

Example 7: Waiting longer for a slow USB key stick
--------------------------------------------------

Some USB hubs and card readers take longer than 10 seconds to appear. Raise the wait on the ZFSBootMenu kernel
command line and keep the crypttab entry unchanged::

  # zbm-kcl -a zbm.luks_token_wait=30 /boot/efi/EFI/zbm/vmlinuz.EFI
  # cat /etc/zfsbootmenu/crypttab
  zroot0  PARTLABEL=zfs0  /keys/zroot.key:LABEL=KEYSTICK  discard

SEE ALSO
========

:doc:`zfsbootmenu-luks(7) </man/zfsbootmenu-luks.7>`, :doc:`zluks(8) </man/zluks.8>`,
:doc:`generate-zbm(5) </man/generate-zbm.5>`, **crypttab**\ (5), **cryptsetup**\ (8)
