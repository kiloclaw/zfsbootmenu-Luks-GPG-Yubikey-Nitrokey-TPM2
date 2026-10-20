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

    POOL_OPTS=( -o ashift=12 -o autotrim=on -o compatibility=openzfs-2.4-linux
                -O compression=lz4 -O acltype=posixacl -O xattr=sa -O relatime=on -m none )

  ``compatibility=openzfs-2.4-linux`` limits the pool to the features of OpenZFS 2.4. The ZFSBootMenu build container
  provides OpenZFS 2.4.4, and every system that imports the pool, including the booted system and any rescue system,
  needs OpenZFS 2.4 or later. If one of them runs an older version, use the matching set, such as
  ``openzfs-2.3-linux``. See :doc:`zfs-versions`.

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
  # luks_create /dev/disk/by-id/ata-DISK1-part2
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

ZFS refuses vdevs smaller than 64 MiB (*device is less than the minimum size*), and a LUKS2 header takes 16 MiB of the
partition, so a partition for a small log or special vdev needs at least 80 MiB.

Example 7: Special allocation class
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A special vdev stores metadata, and optionally small blocks, on fast devices. Losing it loses the pool, so give it at
least as much redundancy as the data vdevs: a two-way mirror for a mirror or RAIDZ1 pool, a three-way mirror for
RAIDZ2:

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/nvme-FAST{1,2,3}-part2
  # zpool add tank special mirror "${VDEVS[@]}"
  # zfs set special_small_blocks=64K tank

``zpool add`` checks that a new special or dedup vdev is as redundant as the data vdevs. On a RAIDZ or dRAID pool it
accepts a mirror with one device more than the parity level, such as the three-way mirror for RAIDZ2 above, and
refuses other widths with *mismatched replication level*. ``-f`` overrides the check, but the pool is then only as
redundant as its weakest vdev.

A dedup vdev holds the deduplication tables of datasets with ``dedup=on``. It is added the same way:

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/nvme-FAST{1,2,3}-part5
  # zpool add tank dedup mirror "${VDEVS[@]}"

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
  # VDEVS=(); luks_create /dev/disk/by-id/nvme-FAST{1,2,3}-part2;  SPECIAL=( "${VDEVS[@]}" )
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

Example 12: A mirror of two NVMe SSDs, unlocked without a crypttab
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A boot pool mirrored across two NVMe SSDs, with 4 KiB sectors throughout. TRIM and the dm-crypt work-queue flags are
stored in the LUKS2 headers, and both ZFSBootMenu and the boot environment are told what to unlock on their kernel
command lines, so no crypttab is needed anywhere. The single-SSD form of this procedure was tested in a UEFI virtual
machine with an emulated NVMe SSD.

1. Optionally switch each SSD to 4 KiB sectors. ``nvme id-ns -H`` lists the supported formats; use the number of a
   ``4096 bytes`` format without metadata. This erases the SSD:

   .. code-block:: bash

     # nvme id-ns -H /dev/nvme0n1 | grep '^LBA Format'
     # nvme format /dev/nvme0n1 --lbaf=1 --force
     # nvme format /dev/nvme1n1 --lbaf=1 --force

2. Partition both SSDs as in `Preparing LUKS Volumes`_ (an ESP and a type ``8309`` partition on each). On an SSD
   that keeps 512-byte sectors, 4 KiB encryption sectors need a partition whose size is a multiple of 4 KiB.
   ``sgdisk -n2:0:0`` ends the partition on the last usable sector, so its size usually is not; give it a size in MiB
   instead, such as ``-n2:0:+900000M`` (``sfdisk`` ends partitions on 1 MiB boundaries by itself). Otherwise
   ``luksFormat`` stops with *Device size is not aligned to requested sector size*.

   Create the LUKS volumes with 4 KiB encryption sectors and open them with the flags stored in the header:

   .. code-block:: bash

     # for p in /dev/disk/by-id/nvme-SSD{1,2}-part2; do
         cryptsetup luksFormat --type luks2 --sector-size 4096 "$p" &&
         cryptsetup open --allow-discards --perf-no_read_workqueue --perf-no_write_workqueue \
           --persistent "$p" "luks-$(cryptsetup luksUUID "$p")"
       done
     # U1=$(cryptsetup luksUUID /dev/disk/by-id/nvme-SSD1-part2)
     # U2=$(cryptsetup luksUUID /dev/disk/by-id/nvme-SSD2-part2)

3. Create the pool:

   .. code-block:: bash

     # zpool create -f "${POOL_OPTS[@]}" zroot mirror /dev/mapper/luks-$U1 /dev/mapper/luks-$U2
     # zpool status -P zroot

4. Tell the boot environment's systemd initramfs to unlock both volumes with one key file that has a slot on each
   (``luks.crypttab=no`` makes systemd ignore ``/etc/crypttab``):

   .. code-block:: bash

     # K=/etc/cryptsetup-keys.d/zroot.key
     # zfs set org.zfsbootmenu:commandline="quiet luks.crypttab=no luks.uuid=$U1 luks.key=$U1=$K luks.uuid=$U2 luks.key=$U2=$K" zroot/ROOT

   After installing the operating system, create the key file, add it to both volumes, and include it and the NVMe
   driver in the initramfs:

   .. code-block:: bash

     # install -d -m 0700 /etc/cryptsetup-keys.d && install -m 0400 /dev/null $K
     # dd if=/dev/urandom of=$K bs=64 count=1
     # for p in /dev/disk/by-id/nvme-SSD{1,2}-part2; do
         cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 "$p" $K
       done
     # cat /etc/dracut.conf.d/10-zfs-luks.conf
     hostonly="yes"
     hostonly_cmdline="no"
     add_dracutmodules+=" zfs crypt "
     add_drivers+=" nvme "
     install_items+=" /etc/cryptsetup-keys.d/zroot.key "

5. Tell ZFSBootMenu which volumes to unlock, in ``/etc/zfsbootmenu/config.yaml``, and build it:

   .. code-block:: yaml

     Kernel:
       CommandLine: "quiet loglevel=0 zbm.luks=UUID=<U1>,UUID=<U2>"

   ZFSBootMenu asks for the passphrase once, for both SSDs. A random key file needs no slow key derivation, which is
   why it is added with ``--pbkdf pbkdf2 --pbkdf-force-iterations 1000``; the passphrase slots keep the default
   memory-hard Argon2id.

For a complete single-disk installation along these lines, see the Gentoo guide ``docs/guides/gentoo/zfs-luks-v4.md``
in the repository.

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
