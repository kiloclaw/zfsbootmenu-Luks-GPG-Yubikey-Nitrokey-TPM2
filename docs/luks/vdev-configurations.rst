vdev Configurations on LUKS
===========================

.. contents:: Contents
  :depth: 2
  :local:
  :backlinks: none

Introduction
------------

A ZFS pool is built from virtual devices (vdevs): data vdevs, which hold the pool's data in a single-disk, mirror,
RAIDZ or dRAID layout, and optional special, dedup, log, cache and spare vdevs. See **zpoolconcepts**\ (7) for the
details of each type.

To store a pool on LUKS, each disk or partition that would hold a vdev member is first formatted as a LUKS volume. The
volume is opened, and ZFS is given the opened device, */dev/mapper/<name>*, instead of the raw disk. ZFS is unaware of
the encryption, so every vdev type and layout works unchanged. ZFSBootMenu unlocks the volumes at boot and imports the
pool as usual; see :doc:`/general/luks`.

This page shows, step by step, how to create each common configuration. All examples share the preparation steps in
`Preparing LUKS Volumes`_ and the finishing steps in `Making the Pool Bootable`_.

Conventions
-----------

* Commands are run as root, shown with a ``#`` prompt.
* Disks are named through */dev/disk/by-id*, which stays stable across reboots. The examples use short placeholder
  names such as ``ata-DISK1``; substitute your own (``ls -l /dev/disk/by-id``).
* Every LUKS volume is opened as ``luks-<UUID>``, the name used by systemd-cryptsetup, dracut and ZFSBootMenu's
  automatic discovery. Using the same names everywhere matters for cache and spare vdevs, which ZFS finds by path.
* All volumes of a pool get the same passphrase, so that ZFSBootMenu asks for it once.
* Pools are created with the options recommended by the ZFSBootMenu installation guides. Set them once:

  .. code-block:: bash

    POOL_OPTS=( -o ashift=12 -o autotrim=on -o compatibility=openzfs-2.3-linux
                -O compression=lz4 -O acltype=posixacl -O xattr=sa -O relatime=on -m none )

.. warning::

  Formatting a device as LUKS destroys its contents. Check every device name twice.

Preparing LUKS Volumes
----------------------

Step 1: Partition the disks
~~~~~~~~~~~~~~~~~~~~~~~~~~~

Disks that ZFSBootMenu boots from need an unencrypted EFI system partition, since the firmware must read the
ZFSBootMenu executable. The rest of the disk becomes the LUKS volume. For each boot disk:

.. code-block:: bash

  # sgdisk --zap-all /dev/disk/by-id/ata-DISK1
  # sgdisk -n1:1M:+512M -t1:EF00 -c1:esp0 \
           -n2:0:0      -t2:8309 -c2:zfs0 /dev/disk/by-id/ata-DISK1

Type ``8309`` is *Linux LUKS*. Disks that only hold data can use a single partition (``sgdisk -n1:0:0 -t1:8309``), or
be formatted as LUKS as a whole.

Step 2: Define a helper that creates and opens LUKS volumes
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The following function formats each device it is given as LUKS2, opens it as ``luks-<UUID>`` and appends the opened
device to the array ``VDEVS``. It asks for the passphrase twice per device:

.. code-block:: bash

  luks_create() {
    local dev uuid
    for dev in "$@"; do
      cryptsetup luksFormat --type luks2 "${dev}" || return 1
      uuid="$( cryptsetup luksUUID "${dev}" )" || return 1
      cryptsetup open --allow-discards --persistent "${dev}" "luks-${uuid}" || return 1
      VDEVS+=( "/dev/mapper/luks-${uuid}" )
    done
  }

``--allow-discards --persistent`` stores the TRIM flag in the LUKS2 header, so the volume passes TRIM requests through
wherever it is opened (needed for ``autotrim=on``). TRIM reveals which blocks are unused; omit both options if that
matters to you.

Step 3: Create the LUKS volumes
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Each example below starts with ``VDEVS=()`` followed by ``luks_create`` for its devices. Afterwards, ``VDEVS`` holds
the opened devices in order:

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/ata-DISK1-part2 /dev/disk/by-id/ata-DISK2-part2
  # printf '%s\n' "${VDEVS[@]}"
  /dev/mapper/luks-5d0c3a1e-...
  /dev/mapper/luks-6e1d4b2f-...

Data vdev Layouts
-----------------

Example 1: Single disk
~~~~~~~~~~~~~~~~~~~~~~

A pool on one LUKS volume. It has no redundancy: a failed disk loses the pool.

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/nvme-DISK1-part2
  # zpool create -f "${POOL_OPTS[@]}" zroot "${VDEVS[0]}"

Example 2: Two-way mirror
~~~~~~~~~~~~~~~~~~~~~~~~~

The usual layout for a boot pool. Either disk can fail.

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/ata-DISK1-part2 /dev/disk/by-id/ata-DISK2-part2
  # zpool create -f "${POOL_OPTS[@]}" zroot mirror "${VDEVS[@]}"

Example 3: Three-way mirror
~~~~~~~~~~~~~~~~~~~~~~~~~~~

Any two of three disks can fail.

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/ata-DISK{1,2,3}-part2
  # zpool create -f "${POOL_OPTS[@]}" zroot mirror "${VDEVS[@]}"

Example 4: Striped mirrors
~~~~~~~~~~~~~~~~~~~~~~~~~~

Two mirror vdevs in one pool ("RAID 10"). One disk in each mirror can fail.

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/ata-DISK{1,2,3,4}-part2
  # zpool create -f "${POOL_OPTS[@]}" zroot \
      mirror "${VDEVS[0]}" "${VDEVS[1]}" \
      mirror "${VDEVS[2]}" "${VDEVS[3]}"

Example 5: RAIDZ
~~~~~~~~~~~~~~~~

RAIDZ spreads data and parity across all disks of the vdev. ``raidz1``, ``raidz2`` and ``raidz3`` survive the loss of
one, two or three disks. A RAIDZ2 vdev of six disks:

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/ata-DISK{1,2,3,4,5,6}-part2
  # zpool create -f "${POOL_OPTS[@]}" tank raidz2 "${VDEVS[@]}"

Example 6: dRAID
~~~~~~~~~~~~~~~~

dRAID (OpenZFS 2.1 or later) is a RAIDZ variant with distributed spare capacity that rebuilds quickly. The following
creates a dRAID2 vdev of 7 disks: groups of 4 data disks plus 2 parity, and 1 distributed spare (``draid2:4d:1s``):

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/ata-DISK{1,2,3,4,5,6,7}
  # zpool create -f "${POOL_OPTS[@]}" tank draid2:4d:1s "${VDEVS[@]}"

These disks hold only data, so they are formatted as LUKS as a whole.

Auxiliary vdevs
---------------

Special, log, cache and spare vdevs hold pool data too, and must also be on LUKS. An unencrypted log device, for
example, holds the plaintext of recent synchronous writes. They can be given when the pool is created, or added later
with ``zpool add``.

Example 7: Special allocation class
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A special vdev stores metadata, and optionally small blocks, on fast devices. Losing it loses the pool, so mirror it:

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/nvme-FAST1-part2 /dev/disk/by-id/nvme-FAST2-part2
  # zpool add tank special mirror "${VDEVS[@]}"
  # zfs set special_small_blocks=64K tank

A dedup vdev is added the same way with ``dedup`` instead of ``special``.

Example 8: Log (SLOG)
~~~~~~~~~~~~~~~~~~~~~

A separate intent log speeds up synchronous writes:

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/nvme-FAST1-part3 /dev/disk/by-id/nvme-FAST2-part3
  # zpool add tank log mirror "${VDEVS[@]}"

Example 9: Cache (L2ARC)
~~~~~~~~~~~~~~~~~~~~~~~~

A cache device extends the read cache. It needs no redundancy:

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/nvme-FAST1-part4
  # zpool add tank cache "${VDEVS[0]}"

Example 10: Hot spare
~~~~~~~~~~~~~~~~~~~~~

A hot spare replaces a failed disk automatically when the ZFS event daemon is running:

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/ata-DISK7-part2
  # zpool add tank spare "${VDEVS[0]}"

.. note::

  ZFS finds cache and spare vdevs by the path recorded in the pool, not by their labels. They are available in
  ZFSBootMenu, and in the booted system's initramfs, only if their LUKS volumes are opened under the same names as when
  they were added. With ``luks-<UUID>`` names this happens automatically. With other names, list the volumes in the
  ZFSBootMenu crypttab under those names (see :doc:`zfsbootmenu-crypttab(5) </man/zfsbootmenu-crypttab.5>`).
  Otherwise they show as ``UNAVAIL`` in ZFSBootMenu, which does not affect booting.

Example 11: A complete layout
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A RAIDZ2 pool with a special mirror, a log mirror, a cache device and a hot spare, created in one command. Each group
of devices is prepared separately:

.. code-block:: bash

  # VDEVS=(); luks_create /dev/disk/by-id/ata-DISK{1,2,3,4}-part2; DATA=( "${VDEVS[@]}" )
  # VDEVS=(); luks_create /dev/disk/by-id/nvme-FAST{1,2}-part2;    SPECIAL=( "${VDEVS[@]}" )
  # VDEVS=(); luks_create /dev/disk/by-id/nvme-FAST{1,2}-part3;    LOG=( "${VDEVS[@]}" )
  # VDEVS=(); luks_create /dev/disk/by-id/nvme-FAST1-part4;        CACHE=( "${VDEVS[@]}" )
  # VDEVS=(); luks_create /dev/disk/by-id/ata-DISK5-part2;         SPARE=( "${VDEVS[@]}" )
  # zpool create -f "${POOL_OPTS[@]}" zroot \
      raidz2 "${DATA[@]}" \
      special mirror "${SPECIAL[@]}" \
      log mirror "${LOG[@]}" \
      cache "${CACHE[@]}" \
      spare "${SPARE[@]}"
  # zpool status -P zroot

.. _luks-making-pool-bootable:

Making the Pool Bootable
------------------------

Step 1: Create a boot environment
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Create the boot environment as in the ZFSBootMenu installation guides. ZFS native encryption can be used on top of
LUKS; ZFSBootMenu then asks for the ZFS key after unlocking the LUKS volumes (see :doc:`/general/native-encryption`):

.. code-block:: bash

  # zfs create -o mountpoint=none zroot/ROOT
  # zfs create -o mountpoint=/ -o canmount=noauto zroot/ROOT/os
  # zpool set bootfs=zroot/ROOT/os zroot

Step 2: Let the installed system unlock the volumes
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

ZFSBootMenu closes the LUKS volumes before it starts the system, so the system's initramfs must unlock them again. List
every volume of the pool in */etc/crypttab*, using the same ``luks-<UUID>`` names:

.. code-block:: bash

  # for m in "${DATA[@]}" "${SPECIAL[@]}" "${LOG[@]}" "${CACHE[@]}" "${SPARE[@]}"; do
      name="${m#/dev/mapper/}"
      echo "${name} UUID=${name#luks-} none luks,discard"
    done >> /etc/crypttab

To avoid a second passphrase prompt, add a key file to every volume and embed it in the system's initramfs, as shown in
:doc:`/general/luks`.

Step 3: Configure and build ZFSBootMenu
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Without further configuration, ZFSBootMenu offers every LUKS volume for unlocking and names them ``luks-<UUID>``, which
suits pools created as above. To restrict it to these volumes, copy the system's crypttab, then rebuild the image:

.. code-block:: bash

  # cp /etc/crypttab /etc/zfsbootmenu/crypttab
  # generate-zbm

To use a TPM, FIDO2 key, OpenPGP card or YubiKey instead of a passphrase, enroll it on every volume as described in
:doc:`administration`, then rebuild the image.

Step 4: Verify
~~~~~~~~~~~~~~

Reboot into ZFSBootMenu. After unlocking, press *Escape* to stop the countdown and *MOD+R* (*Alt+R*) to open a recovery shell,
then check the volumes and the pool:

.. code-block:: bash

  # zluks status
  # zpool status -P zroot

Every vdev, including the cache and spare, should be ``ONLINE`` (spares ``AVAIL``). Type ``exit`` to return to the
menu.

See Also
--------

* :doc:`administration`: replacing disks, growing pools and managing keys
* :doc:`zfsbootmenu-luks(7) </man/zfsbootmenu-luks.7>`, :doc:`zluks(8) </man/zluks.8>`
* **zpoolconcepts**\ (7), **zpool-create**\ (8), **zpool-add**\ (8), **cryptsetup**\ (8)
