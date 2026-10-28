ZFS Pools on LUKS Volumes
=========================

.. contents:: Contents
  :depth: 2
  :local:
  :backlinks: none

ZFSBootMenu can import pools whose vdevs live inside LUKS (dm-crypt) volumes. Before importing pools, it finds the LUKS
volumes, asks for a passphrase (or uses a key file or key script), and opens them. It then imports the pools on the
unlocked devices as usual. Right before it hands control to a boot environment with ``kexec``, ZFSBootMenu exports
those pools and closes the LUKS mappings again. Closing them makes the kernel wipe the volume keys from memory.

This is independent of ZFS native encryption. Both may be used at the same time; see :doc:`native-encryption`.

How it works
------------

During start-up ZFSBootMenu runs the following steps in order:

1. load the ZFS kernel modules and configure the hostid;
2. wait for any devices listed in ``zbm.wait_for``;
3. run *early-setup* hooks;
4. **unlock LUKS volumes**;
5. import pools and show the menu.

Because LUKS volumes are unlocked after *early-setup* hooks, a hook can prepare key material, for example by mounting a
key store. The contributed ``contrib/luks-unlock.sh`` hook is one such example.

When several volumes share a passphrase, which is typical for the members of a mirror or RAIDZ vdev, the passphrase is
asked for **once**. Every passphrase that unlocks a volume is tried silently on the remaining volumes before the user
is prompted again. Entered passphrases are kept only in a short-lived subshell and are discarded as soon as unlocking
finishes.

At each prompt:

* an **empty** passphrase skips the volume;
* after a configurable number of failed attempts (``tries=``, default 3), ZFSBootMenu offers to try again or skip the
  volume;
* if a volume is unlocked from somewhere else while the prompt is waiting, for example with ``zluks`` over SSH, the
  prompt notices and moves on.

If no pool can be imported afterwards, the normal ZFSBootMenu import loop takes over. Before every retry it looks for
LUKS volumes that appeared since the last attempt (slow USB enclosures and some SAS controllers enumerate late),
unlocks them, and imports again right away. Volumes you skipped are not offered again. From the recovery shell, use
``zluks`` to unlock volumes and then ``exit`` to resume.

The loop ends as soon as a pool can be imported, even ``DEGRADED``, so disks that appear after that are not used. To
make ZFSBootMenu wait for slow disks, list them in ``zbm.wait_for`` (it waits for them before unlocking anything), or
name their volumes in ``zbm.luks`` or the crypttab.

For every volume, ZFSBootMenu tries these methods in order and stops at the first that works:

1. a key script (``keyscript=``);
2. a key file, plain or GnuPG-encrypted;
3. LUKS2 tokens: TPM2 without a PIN and clevis first, then TPM2 with a PIN and FIDO2 security keys (see
   `Hardware keys and encrypted key files`_);
4. passphrases (and YubiKey responses) that unlocked other volumes;
5. YubiKey challenge-response (``ykchalresp``);
6. a passphrase prompt.

Within one unlock run, secrets are reused across volumes, so the members of a mirror or RAIDZ vdev ask only once. This
covers passphrases, YubiKey responses, TPM2 PINs, and GnuPG key files, which are decrypted only once.

Every interactive method can be skipped by answering with an empty line, which moves on to the next one, so the
LUKS passphrase always remains available as a fallback.

Choosing which volumes to unlock
--------------------------------

The volumes to unlock are chosen by the first of these that applies:

1. the ``zbm.luks`` kernel command-line option;
2. a crypttab file embedded in the image (``/etc/zfsbootmenu/crypttab``);
3. automatic discovery.

Automatic discovery (default)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

With no configuration at all, every LUKS volume on the system is a candidate. After unlocking, any volume whose
contents are recognized as something *other* than a ZFS vdev (an ext4 file system or swap, for example) is closed
again immediately. Volumes that should not be touched can be skipped by entering an empty passphrase, or excluded
entirely by configuring an explicit list as described below.

Discovered volumes are mapped as ``/dev/mapper/luks-<UUID>``.

Kernel command line
~~~~~~~~~~~~~~~~~~~

``zbm.luks=off``
  Disable LUKS unlocking.

``zbm.luks=auto``
  Force automatic discovery, even when the image contains a crypttab.

``zbm.luks=<device>[,<device>,...]``
  Unlock only the listed devices. A device may be a path below ``/dev`` or one of ``UUID=``, ``PARTUUID=``,
  ``LABEL=``, ``PARTLABEL=``, ``ID=`` (``/dev/disk/by-id``) or ``PATH=`` (``/dev/disk/by-path``). A bare LUKS UUID, or
  ``luks-<UUID>``, is also accepted. For example::

    zbm.luks=PARTLABEL=zfs0,PARTLABEL=zfs1

``zbm.luks_key=<path>[:<device>]``
  A key file used for every volume that has no key of its own. With ``:<device>``, the device is mounted read-only,
  the key file is read from ``<path>`` relative to the root of that device, and the device is unmounted again. This
  is useful for unlocking from a USB stick::

    zbm.luks_key=/keys/zroot.key:LABEL=KEYSTICK

  When the device is not there yet, as often happens with USB sticks that the kernel is still detecting, ZFSBootMenu
  waits for it for ``zbm.luks_token_wait`` seconds (default 10; any key ends the wait). It waits once per device in
  each unlock run, and not during pool-import retries. If the key file is missing or does not unlock a volume,
  ZFSBootMenu falls back to a passphrase prompt.

Kernel command-line options can be embedded in the image through ``Kernel.CommandLine`` in
:doc:`/man/generate-zbm.5`, or set at boot with ``zbm-kcl``.

crypttab
~~~~~~~~

For finer control, write a ``crypttab(5)``-style file. By default it is read from ``/etc/zfsbootmenu/crypttab`` when the
image is built. Another file may be selected with ``zfsbootmenu_luks_crypttab=<path>`` in a dracut configuration file
or in the ZFSBootMenu ``mkinitcpio.conf``. The file is copied into the image as ``/etc/zfsbootmenu/crypttab`` with mode
``0600``. An annotated example is installed as ``examples/crypttab`` with the ZFSBootMenu documentation.

Each line has four whitespace-separated fields::

  # <mapping name>  <device>          <key file>                  <options>
  zroot-a           PARTLABEL=zfs0    none                        discard
  zroot-b           PARTLABEL=zfs1    none                        discard
  tank0             UUID=0a1b...      /keys/tank.key:LABEL=USB    nofail,tries=1

The fields are:

*mapping name*
  The name of the mapping created below ``/dev/mapper``. ZFS finds data, special and log vdevs by their labels,
  whatever the mapping is called. Hot spares and cache (L2ARC) devices, however, are found by the path recorded in the
  pool, so on LUKS they show as ``UNAVAIL`` in ZFSBootMenu unless the mapping has the same name as on the running
  system. Automatic discovery names mappings ``luks-<UUID>``, the default of systemd and dracut; if your system uses
  other names, list the volumes here with those names.

*device*
  Any of the device forms accepted by ``zbm.luks``.

*key file*
  ``none`` or ``-`` to prompt for a passphrase; a path to a key file present in the running ZFSBootMenu system, for
  example one provided by an early-setup hook; or ``<path>:<device>`` to read the key from another device. Plain key
  files are **never** copied into the image (see `Security notes`_), and the build prints a warning if one would be
  needed. GnuPG-encrypted key files (``*.gpg``, ``*.asc``, ``*.pgp`` or the ``gpg`` option) are different: they are
  embedded in the image, because they are useless without their passphrase or smart card.

*options*
  A comma-separated list. ZFSBootMenu understands:

  ===============================  ===================================================================================
  ``discard``                      pass TRIM requests through (``--allow-discards``)
  ``readonly``, ``read-only``      open the mapping read-only
  ``tries=N``                      passphrase attempts per round (default 3; 0 means unlimited)
  ``noauto``                       do not unlock at start-up; the volume can still be opened with ``zluks open``
  ``nofail``                       do not wait or prompt if the device is missing
  ``keyscript=<path>``             run a script that prints the key on standard output (see below)
  ``header=<path>``                use a detached LUKS header
  ``keyfile-offset=N``             skip N bytes of the key file
  ``keyfile-size=N``               read at most N bytes of the key file
  ``key-slot=N``                   only try key slot N
  ``gpg``                          the key file is GnuPG-encrypted, whatever its name
  ``no-tokens``                    do not try LUKS2 tokens (TPM2, FIDO2, clevis) for this volume
  ``ykchalresp[=1|2]``             YubiKey challenge-response with the given slot (default 2)
  ``ykchalresp-hash``              use the SHA-256 of the passphrase as the challenge
  ``ykchalresp-concat``            the LUKS passphrase is the challenge followed by the response
  ``no-read-workqueue``,           dm-crypt performance flags
  ``no-write-workqueue``,
  ``same-cpu-crypt``,
  ``submit-from-crypt-cpus``
  ===============================  ===================================================================================

  Other options found in typical crypttab files (``luks``, ``initramfs``, ``x-initrd.attach`` ...) are ignored, so a
  copy of an existing ``/etc/crypttab`` can often be used as a starting point.

If a configured device is missing, ZFSBootMenu waits for it, retrying every ``zbm.retry_delay`` seconds, until it
appears or the user presses *Escape*. Devices marked ``nofail`` are skipped without waiting.

Key scripts
~~~~~~~~~~~

A ``keyscript=`` option names an executable that prints the key on standard output. This follows the Debian crypttab
convention: a bare name refers to ``/lib/cryptsetup/scripts/<name>``. The script receives the arguments and environment
variables of the Debian convention: the key file field as its first argument, plus ``CRYPTTAB_NAME``,
``CRYPTTAB_SOURCE``, ``CRYPTTAB_KEY`` and ``CRYPTTAB_OPTIONS``.

Key scripts referenced from the crypttab are installed into the image automatically, together with their shared
library dependencies. Any other programs a script needs (``gpg``, ``pcscd`` ...) must be added through the image
builder, for example with ``install_items`` in dracut or ``BINARIES``/``FILES`` in mkinitcpio. Key scripts are the
intended integration point for hardware tokens such as OpenPGP smart cards.

Hardware keys and encrypted key files
-------------------------------------

ZFSBootMenu supports the common ways of tying a LUKS volume to hardware:

======================================  ==================================================  ===========================
Method                                  Hardware                                            Set up with
======================================  ==================================================  ===========================
TPM2 token                              the computer's TPM 2.0                              ``systemd-cryptenroll``
FIDO2 token                             YubiKey 5, Nitrokey 3, SoloKey, other FIDO2 keys    ``systemd-cryptenroll``
clevis token                            TPM 2.0 (and tang servers on a network)             ``clevis luks bind``
GnuPG-encrypted key file                OpenPGP card: YubiKey, Nitrokey Pro/Start/3, ...;   ``gpg --encrypt``
                                        or a passphrase only
YubiKey challenge-response              YubiKey (HMAC-SHA1 slot)                            ``ykpersonalize``
======================================  ==================================================  ===========================

All of them work on top of an ordinary LUKS passphrase: keep that passphrase in a key slot as a fallback. No
configuration is needed for tokens, which ZFSBootMenu finds in the LUKS2 header; GnuPG key files and challenge-response
are configured in the crypttab. ``zluks list`` shows the tokens of every volume.

ZFSBootMenu waits ``zbm.luks_token_wait`` seconds (default 10) for a security key, smart card or key device to be
plugged in before moving on; pressing any key skips the wait, and ``0`` disables waiting. ``zbm.luks_tokens=0``
disables all tokens.

TPM2
~~~~

Enroll the TPM from the running system::

  systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7 /dev/disk/by-partlabel/zfs0
  # or, to also require a PIN:
  systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7 --tpm2-with-pin=yes /dev/disk/by-partlabel/zfs0

Without a PIN the volume unlocks with no interaction. With a PIN, ZFSBootMenu asks for it, and a PIN that worked is
tried once on further PIN-protected volumes. Wrong PINs count towards the TPM's dictionary-attack protection. When the
TPM locks up, ZFSBootMenu says so and moves on to other methods instead of asking again.

Choosing PCRs matters, because ZFSBootMenu itself is part of the boot chain:

* **PCR 7** (Secure Boot state) is stable across ZFSBootMenu, kernel and initramfs updates, and is the usual choice.
* PCRs 4, 8, 9 and 11 change whenever the ZFSBootMenu image or the kernel changes. Binding to them means re-enrolling
  after every update.
* ``--tpm2-pcrs=`` (no PCRs) together with a PIN works on systems without Secure Boot.

Volumes enrolled this way are opened through systemd's ``systemd-tpm2`` cryptsetup plugin when the image contains it
(images built on distributions with systemd, including the build container). Images without it use ZFSBootMenu's own
implementation based on ``tpm2-tools``. That implementation covers tokens created by
systemd 254 or later with or without a PIN and with or without a PCR policy. Signed PCR policies (``--tpm2-public-key``),
``--tpm2-pcrlock`` and tokens from older systemd versions need the plugin.

clevis tokens created with ``clevis luks bind -d <device> tpm2 '{"pcr_ids":"7"}'`` are unlocked as well, through the
clevis tools in the image. Tang policies work only if networking is set up in ZFSBootMenu (see :doc:`remote-access`).

FIDO2 security keys (YubiKey, Nitrokey, ...)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Enroll a key that supports the FIDO2 ``hmac-secret`` extension::

  systemd-cryptenroll --fido2-device=auto /dev/disk/by-partlabel/zfs0
  # by default this requires the key's PIN and a touch; see --fido2-with-client-pin= and --fido2-with-user-presence=

At boot, ZFSBootMenu waits for the key, asks for its PIN if the token requires one, and asks you to touch the key.
Images with systemd's ``systemd-fido2`` plugin use it. Otherwise ZFSBootMenu talks to the key with ``fido2-assert``
from libfido2, which then asks for the PIN itself. Each volume has its own credential, so a mirror of two volumes
enrolled with the same key needs two touches.

GnuPG and OpenPGP smart cards (YubiKey, Nitrokey, ...)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A random key file, added to a LUKS key slot and encrypted with GnuPG, can be protected by an OpenPGP key that lives on
a smart card such as a YubiKey or Nitrokey::

  dd if=/dev/urandom of=/root/zroot.key bs=64 count=1
  cryptsetup luksAddKey /dev/disk/by-partlabel/zfs0 /root/zroot.key
  mkdir -p /etc/zfsbootmenu/keys
  gpg --encrypt --recipient 0xYOURKEYID --output /etc/zfsbootmenu/keys/zroot.key.gpg /root/zroot.key
  shred -u /root/zroot.key

Then list the encrypted file in the crypttab and name the public key so it is copied into the image (in a dracut
configuration file, or in the ZFSBootMenu ``mkinitcpio.conf``)::

  # /etc/zfsbootmenu/crypttab
  zroot-a  PARTLABEL=zfs0  /etc/zfsbootmenu/keys/zroot.key.gpg  discard

  # /etc/zfsbootmenu/dracut.conf.d/gpg.conf
  zfsbootmenu_luks_gpg_keys="0xYOURKEYID"

At boot ZFSBootMenu asks for the card, reads it, and confirms that the card holds the key the file is encrypted to.
Only then does it ask for the card PIN, showing how many PIN attempts remain, so a missing or wrong card never uses up
PIN attempts. Touch the key if it is configured to require touch for decryption. The same key file can be encrypted to
several recipients (for example a primary and a backup card).

GnuPG's built-in CCID driver handles YubiKeys and Nitrokeys directly. For other readers, set
``zfsbootmenu_luks_pcscd=yes`` to include ``pcscd`` and its drivers; ZFSBootMenu starts it when needed.

A key file encrypted with a passphrase only (``gpg --symmetric``) also works. Because it is embedded in the image,
anyone with the image can attack its passphrase offline, so the build warns about it. Use a strong passphrase.

YubiKey challenge-response
~~~~~~~~~~~~~~~~~~~~~~~~~~

This is the scheme used by ``yubikey-luks``: a passphrase is sent to an HMAC-SHA1 slot of the YubiKey, and the response
is a LUKS passphrase::

  ykpersonalize -2 -ochal-resp -ochal-hmac -ohmac-lt64 -oserial-api-visible   # program slot 2 (once)
  printf '%s' 'my passphrase' | ykchalresp -2 -i- | tr -d '\n' > /root/yk.key
  cryptsetup luksAddKey /dev/disk/by-partlabel/zfs0 /root/yk.key && shred -u /root/yk.key

  # /etc/zfsbootmenu/crypttab
  zroot-a  PARTLABEL=zfs0  none  discard,ykchalresp=2

Volumes enrolled with ``yubikey-luks`` work unchanged. If ``yubikey-luks`` was set up with ``HASH=1`` or
``CONCATENATE=1``, add ``ykchalresp-hash`` or ``ykchalresp-concat`` respectively.

Tools in the recovery shell
---------------------------

The ``zluks`` command is available in the recovery shell and in SSH sessions (see :doc:`remote-access`)::

  zluks                      # unlock all configured volumes, as at start-up
  zluks open <device> [name] # unlock a single volume
  zluks close [name]         # export pools on, and close, mappings opened by ZFSBootMenu
  zluks status               # mappings opened by ZFSBootMenu and the pools that use them
  zluks list                 # every LUKS volume on the system

``cryptsetup`` and ``dmsetup`` are also available for manual work.

Booting the system afterwards
-----------------------------

ZFSBootMenu starts the boot environment with ``kexec``. The new kernel begins with no open dm-crypt mappings, so
**the boot environment's own initramfs must unlock the LUKS volumes again**, exactly as it would without ZFSBootMenu.

To avoid typing the passphrase twice, add a LUKS key slot backed by a key file and embed that key file in the boot
environment's initramfs. This is safe in this setup only because the initramfs lives in ``/boot`` *inside* the
encrypted pool, so it is never readable without unlocking the pool first. It is the same idea that
:doc:`native-encryption` describes for ZFS keys.

::

  # On the running system, once per LUKS volume
  dd if=/dev/urandom of=/etc/cryptsetup-keys.d/zroot.key bs=64 count=1
  chmod 0400 /etc/cryptsetup-keys.d/zroot.key
  cryptsetup luksAddKey /dev/disk/by-partlabel/zfs0 /etc/cryptsetup-keys.d/zroot.key

Then reference the key file from ``/etc/crypttab`` and include it in the initramfs:

* **dracut**::

    # /etc/crypttab
    luks-zfs0  UUID=<uuid>  /etc/cryptsetup-keys.d/zroot.key  luks,discard

    # /etc/dracut.conf.d/luks-key.conf
    install_items+=" /etc/cryptsetup-keys.d/zroot.key /etc/crypttab "

* **Debian / Ubuntu (initramfs-tools)**::

    # /etc/crypttab -- the "initramfs" option forces inclusion
    luks-zfs0  UUID=<uuid>  /etc/cryptsetup-keys.d/zroot.key  luks,discard,initramfs

    # /etc/cryptsetup-initramfs/conf-hook
    KEYFILE_PATTERN="/etc/cryptsetup-keys.d/*.key"

    # /etc/initramfs-tools/initramfs.conf
    UMASK=0077

* **mkinitcpio** (with the ``systemd`` and ``sd-encrypt`` hooks)::

    # /etc/crypttab.initramfs
    luks-zfs0  UUID=<uuid>  /etc/cryptsetup-keys.d/zroot.key  luks,discard

    # /etc/mkinitcpio.conf
    FILES+=(/etc/cryptsetup-keys.d/zroot.key)

Regenerate the initramfs afterwards, and always confirm that the resulting image is readable by root only.

Unlocking without a crypttab
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Both stages can do without a crypttab:

* **ZFSBootMenu** reads ``zbm.luks=`` and ``zbm.luks_key=`` from its own kernel command line (``Kernel.CommandLine`` in
  ``/etc/zfsbootmenu/config.yaml``). A GnuPG-encrypted key file named only on the command line is not embedded
  automatically; add it with dracut's ``install_items`` in ``/etc/zfsbootmenu/dracut.conf.d/``.
* **A systemd-based initramfs** in the boot environment reads ``luks.uuid=``, ``luks.key=`` and ``luks.options=``
  from its kernel command line, and ``luks.crypttab=no`` makes it ignore ``/etc/crypttab``. Set them in the
  ``org.zfsbootmenu:commandline`` property, which ZFSBootMenu passes to the boot environment::

    # U=$(cryptsetup luksUUID /dev/disk/by-partlabel/zfs0)
    # zfs set org.zfsbootmenu:commandline="quiet luks.crypttab=no luks.uuid=$U luks.key=$U=/etc/cryptsetup-keys.d/zroot.key" zroot/ROOT

    # /etc/dracut.conf.d/10-zfs-luks.conf
    hostonly_cmdline="no"
    add_dracutmodules+=" zfs crypt "
    install_items+=" /etc/cryptsetup-keys.d/zroot.key "

  ``hostonly_cmdline="no"`` stops dracut from embedding ``rd.luks`` parameters it detects on its own. On NVMe systems,
  also add ``add_drivers+=" nvme "``, because host-only detection does not always see through ZFS and LUKS to the
  disk.

TRIM and the dm-crypt work-queue flags need no crypttab options either when they are stored in the LUKS2 header with
``cryptsetup open --persistent``; every later activation applies them::

  # cryptsetup open --allow-discards --perf-no_read_workqueue --perf-no_write_workqueue --persistent \
      /dev/nvme0n1p2 luks-$(cryptsetup luksUUID /dev/nvme0n1p2)

Building images
---------------

LUKS support is built in and enabled by default. When ``cryptsetup`` is installed on the build host, the ZFSBootMenu
dracut module and mkinitcpio hook add the following to the image:

* ``cryptsetup``, ``dmsetup`` and ``udevadm``;
* the device-mapper udev rules;
* ``dm-crypt`` and the common kernel crypto modules (AES, XTS, SHA-2 and their accelerated variants), where these are
  built as modules;
* the crypttab and any key scripts and GnuPG-encrypted key files it references;
* when installed on the build host: systemd's ``systemd-tpm2`` and ``systemd-fido2`` cryptsetup plugins (with the
  libraries they load at run time), ``tpm2-tools`` for native TPM2 unlocking (with ``openssl`` in images of the legacy
  runtime, whose LUKS code also takes ``sha256sum`` and ``base64``), ``fido2-assert`` and
  ``fido2-token``, the clevis tools, GnuPG (``gpg``, ``gpg-agent``, ``scdaemon``), and ``ykchalresp``;
* TPM, USB and HID drivers for TPM chips, security keys and smart cards.

Options for ``dracut.conf.d`` or the ZFSBootMenu ``mkinitcpio.conf``:

``zfsbootmenu_luks=no``
  Leave out LUKS support entirely.

``zfsbootmenu_luks_crypttab=<path>``
  The crypttab to embed (default ``/etc/zfsbootmenu/crypttab``). It is ignored for release builds, which only use
  automatic discovery and the kernel command line.

``zfsbootmenu_luks_tokens=no``
  Leave out TPM2, FIDO2 and clevis support.

``zfsbootmenu_luks_token_plugins=no``
  Leave out systemd's cryptsetup token plugins and always use ZFSBootMenu's own TPM2 and FIDO2 code.

``zfsbootmenu_luks_clevis=no``
  Leave out clevis, even if it is installed.

``zfsbootmenu_luks_gpg=no``
  Leave out GnuPG.

``zfsbootmenu_luks_gpg_keys="<key id> ..."``
  OpenPGP public keys to export from the build host's keyring into the image, so key files encrypted to them (and to
  the cards holding them) can be decrypted.

``zfsbootmenu_luks_gpg_pubkeys="<file> ..."``
  Files holding OpenPGP public keys to import into the image.

``zfsbootmenu_luks_pcscd=yes``
  Include ``pcscd`` and its card reader drivers for readers that GnuPG's built-in driver does not handle.

``zfsbootmenu_luks_yubikey=no``
  Leave out ``ykchalresp``.

The build container (``packaging/container``) installs GnuPG with ``scdaemon``, libfido2, clevis, tpm2-tools, ykpers and
openssl together with systemd's cryptsetup token plugins, so its images support every method, including tokens that only
the plugins support; see :doc:`container-building`.

Volumes that use a cipher not covered by the default module list need their modules added explicitly, for example
``add_drivers+=" serpent_avx2 "`` for dracut or ``MODULES=(...)`` for mkinitcpio.

The dracut ``crypt`` module must **not** be included in ZFSBootMenu images, because it would try to unlock LUKS volumes
on its own before ZFSBootMenu starts. The default ZFSBootMenu dracut configuration omits it, and as a safeguard images
also carry ``rd.luks=0``. Likewise, do not add the ``encrypt`` or ``sd-encrypt`` hooks to the ZFSBootMenu
``mkinitcpio.conf``.

When building with the :doc:`container-building` helper, put the crypttab in the build directory and point at it from a
file in ``dracut.conf.d`` there::

  echo 'zfsbootmenu_luks_crypttab="/build/crypttab"' > /path/to/build/dracut.conf.d/luks.conf

Security notes
--------------

* **Never put LUKS key files inside a ZFSBootMenu image.** The image normally lives on an unencrypted EFI system
  partition. Use passphrases, a key on a separate device (``path:device``), or a key script.
* The crypttab itself contains no secrets, but it does reveal device identifiers.
* Mappings opened by ZFSBootMenu are closed before ``kexec``, which wipes their volume keys from kernel memory. The
  ``80-luks-close.sh`` teardown hook does this. It can be disabled with ``zbm.skip_hooks=80-luks-close.sh``, but there
  is rarely a reason to.
* **A TPM2 token without a PIN unlocks the pools for anyone who can start the computer.** ZFSBootMenu offers a recovery
  shell and lets you edit kernel command lines. Combine TPM-only unlocking with Secure Boot (bind to PCR 7) and a boot
  environment whose data is also protected otherwise, or require a PIN.
* Decrypted key material from GnuPG, TPM2, FIDO2 and challenge-response exists only in RAM (``/run``), and is deleted
  as soon as the volume is open. GnuPG's agents are stopped and their temporary home removed after every use.
* Pools are imported read-only by default, as always. When ZFSBootMenu needs write access (for snapshots, cloning,
  setting properties ...) it re-imports the pool; the LUKS mappings stay open until boot.

Troubleshooting
---------------

* Set ``loglevel=7`` on the ZFSBootMenu command line and use ``zlogtail`` in the recovery shell to see every
  ``cryptsetup`` call and its result.
* ``zluks list`` shows which LUKS volumes ZFSBootMenu can see. If a volume is missing, the storage driver for its disk
  is probably missing from the image.
* ``cryptsetup open`` failing with errors about unknown ciphers means a kernel crypto module is missing (see
  `Building images`_).
* A TPM2 token is ignored if no TPM device appears (``ls /dev/tpm*``). Check that the TPM is enabled in the firmware.
* A TPM2 token stops working after firmware, Secure Boot or ZFSBootMenu changes if it is bound to PCRs that changed.
  Unlock with the passphrase and re-enroll: ``systemd-cryptenroll --wipe-slot=tpm2 --tpm2-device=auto ...``.
* If a security key or card is not detected, check ``ls /dev/hidraw*`` (FIDO2) and ``gpg --card-status`` (OpenPGP) in
  the recovery shell. Missing USB host controller drivers are the usual cause on hostonly images.
