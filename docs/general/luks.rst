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

If no pool can be imported afterwards, the normal ZFSBootMenu import loop takes over. From the recovery shell, use
``zluks`` to unlock volumes and then ``exit`` to resume.

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

  If the key file is missing or does not unlock a volume, ZFSBootMenu falls back to a passphrase prompt.

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
  The name of the mapping created below ``/dev/mapper``. It only matters inside ZFSBootMenu; the boot environment
  chooses its own names.

*device*
  Any of the device forms accepted by ``zbm.luks``.

*key file*
  ``none`` or ``-`` to prompt for a passphrase; a path to a key file present in the running ZFSBootMenu system, for
  example one provided by an early-setup hook; or ``<path>:<device>`` to read the key from another device. Key files
  are **never** copied into the image (see `Security notes`_), and the build prints a warning if one would be needed.

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

Building images
---------------

LUKS support is built in and enabled by default. When ``cryptsetup`` is installed on the build host, the ZFSBootMenu
dracut module and mkinitcpio hook add the following to the image:

* ``cryptsetup``, ``dmsetup`` and ``udevadm``;
* the device-mapper udev rules;
* ``dm-crypt`` and the common kernel crypto modules (AES, XTS, SHA-2 and their accelerated variants), where these are
  built as modules;
* the crypttab and any key scripts it references.

Options for ``dracut.conf.d`` or the ZFSBootMenu ``mkinitcpio.conf``:

``zfsbootmenu_luks=no``
  Leave out LUKS support entirely.

``zfsbootmenu_luks_crypttab=<path>``
  The crypttab to embed (default ``/etc/zfsbootmenu/crypttab``). It is ignored for release builds, which only use
  automatic discovery and the kernel command line.

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
