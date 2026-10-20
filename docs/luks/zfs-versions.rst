ZFS Versions and Pool Features
==============================

.. contents:: Contents
  :depth: 2
  :local:
  :backlinks: none

Introduction
------------

A pool is imported twice on every boot: first by ZFSBootMenu, with the ZFS kernel module and tools in its image, and
then by the booted system, with its own. Both must support every feature that is *active* on the pool. This page
describes the ZFS version of the images built in the build container, how to keep new pools importable by the systems
that need them, and what happens when a version is too old. See **zpool-features**\ (7) for the features themselves.

LUKS does not change any of this: ZFS does not know that its devices are encrypted.

ZFS in ZFSBootMenu Images
-------------------------

Images built in the build container (``releng/docker``, Ubuntu 26.10) contain OpenZFS 2.4.4, both the tools and the
kernel module, with the Linux 7.3 kernel. Images built on a host contain the host's ZFS.

To see the version of a running image, open a recovery shell in ZFSBootMenu (*Escape*, then *MOD+R*) and run:

.. code-block:: bash

  # zfs version
  zfs-2.4.4-1ubuntu4
  zfs-kmod-2.4.4-1ubuntu4

Choosing a Compatibility Set
----------------------------

The ``compatibility`` pool property limits the features ``zpool create`` and ``zpool upgrade`` enable to those of a
named OpenZFS release (see **zpoolprops**\ (7)). Choose the set of the oldest ZFS version that must import the pool:
the ZFSBootMenu image, the booted system, and any rescue or live system.

``openzfs-2.4-linux``
  Every feature of OpenZFS 2.4. Recommended when ZFSBootMenu is built in the build container and the booted system
  has OpenZFS 2.4 or later. The examples in :doc:`vdev-configurations` use this set.

``openzfs-2.3-linux``, ``openzfs-2.2-linux`` ...
  For pools that a system with an older ZFS must also import, for example a live system used for installation and
  rescue.

No compatibility property (``off``)
  ``zpool create`` enables every feature the creating ZFS supports. Such a pool can be imported only by the same or a
  newer ZFS version once the features are in use.

``zpool upgrade`` enables the features the compatibility set allows, except ``dynamic_gang_header``: OpenZFS 2.4
never enables it on upgrade, whatever the compatibility set (``zpool upgrade`` marks it with ``(*)``). ``zpool create``
enables it like any other feature of the set. On an upgraded pool, enable it with
``zpool set feature@dynamic_gang_header=enabled <pool>``.

The property is set at creation with ``-o compatibility=<set>``, or later with ``zpool set``.

Features Added in OpenZFS 2.3 and 2.4
-------------------------------------

An *enabled* feature becomes *active* only when it is used. Older ZFS versions cannot import a pool on which an active
feature is unsupported, unless the feature is read-only compatible: then they can import it read-only.

======================== ======= ============================================================ ==================
Feature                  Release Becomes active when                                          Read-only import
                                                                                              by older ZFS
======================== ======= ============================================================ ==================
``raidz_expansion``      2.3     a disk is attached to a RAIDZ vdev (``zpool attach``)        no
``fast_dedup``           2.3     the first deduplicated block is written to a new dedup table yes
``longname``             2.3     a name longer than 255 bytes is created (``longname=on``)    no
``large_microzap``       2.3     a micro ZAP grows beyond 128 KiB (``zap_micro_max_size``)    yes
``block_cloning_endian`` 2.4     block cloning (``cp --reflink``) creates its first table     yes
``dynamic_gang_header``  2.4     gang blocks must be nested, on a vdev with sectors > 512 B   no
``physical_rewrite``     2.4     ``zfs rewrite -P`` is first used                             yes
======================== ======= ============================================================ ==================

ZFS writes a gang block when it cannot allocate a block in one piece, which happens on a nearly full or badly
fragmented pool. ``dynamic_gang_header`` becomes active when the pieces of a gang block do not fit either; the larger
gang headers it allows then hold more pieces.

All of them work on pools whose vdevs are LUKS volumes, and ZFSBootMenu images built in the build container import such
pools; see :doc:`testing`.

Importing With an Older ZFS
---------------------------

ZFSBootMenu imports pools read-only. When its ZFS is older than the pool's:

* A pool whose newer features are only *enabled* is imported. ZFSBootMenu shows *Unsupported features detected*, and
  does not allow read-write operations on the pool, such as snapshots or changes to boot environment properties. Its
  boot environments can still be booted.
* A pool whose unsupported *active* features are all read-only compatible is imported in the same way.
* A pool with an unsupported active feature that is not read-only compatible, such as ``raidz_expansion`` after a
  RAIDZ expansion, cannot be imported. ZFSBootMenu still unlocks its LUKS volumes. If it can import no pool at all, it
  stays in its import retry loop; if other pools were imported, it boots from those or, without a boot environment,
  drops to its emergency shell. In the recovery shell, ``zpool import`` names the unsupported features.

The booted system must support the active features of its pools as well; ZFSBootMenu does not check that.

The cure is an image with a newer ZFS: rebuild the build container and the image (see
:doc:`/general/container-building`).

.. note::

  An image built on another host, such as an older image kept as a fallback or a release image, may have another
  host ID than the booted system. The root pool stays imported when the booted system powers off, so ZFS reports it as
  *last accessed by another system*, and ZFSBootMenu adopts the pool's host ID only when no pool could be imported at
  all. Give such an image the right host ID with ``spl_hostid=`` or name the root pool with ``zbm.prefer=`` on its
  command line; see :doc:`zfsbootmenu(7) </man/zfsbootmenu.7>`.

Upgrading a Pool
----------------

Update every system that imports the pool before enabling new features, starting with ZFSBootMenu:

1. Rebuild the build container and the ZFSBootMenu image, and check its ZFS version as shown above.

2. Update ZFS on the booted system and on any rescue or live system.

3. Raise the compatibility set, then enable the features it allows:

   .. code-block:: bash

     # zpool set compatibility=openzfs-2.4-linux zroot
     # zpool upgrade zroot

4. ``zpool upgrade`` leaves ``dynamic_gang_header`` disabled. Enable it as well:

   .. code-block:: bash

     # zpool set feature@dynamic_gang_header=enabled zroot

5. Check the result:

   .. code-block:: bash

     # zpool get -H -o property,value all zroot | grep feature@

Features stay *enabled* until they are used. After a feature that is not read-only compatible becomes active, older
ZFS versions, including older ZFSBootMenu release images, can no longer import the pool. Keep a current ZFSBootMenu
image on every EFI system partition.

Checking Versions and Features
------------------------------

``zfs version``
  ZFS tools and kernel module versions.

``zpool get compatibility <pool>``
  The pool's compatibility set.

``zpool status <pool>``
  Notes when supported features are not enabled.

``zpool get -H -o property,value all <pool> | grep feature@``
  Every feature and its state: ``disabled``, ``enabled`` or ``active``.

See Also
--------

* **zpool-features**\ (7), **zpoolprops**\ (7), **zpool-upgrade**\ (8), **zpool-attach**\ (8), **zfs-rewrite**\ (8)
* :doc:`vdev-configurations`, :doc:`administration`, :doc:`testing`
