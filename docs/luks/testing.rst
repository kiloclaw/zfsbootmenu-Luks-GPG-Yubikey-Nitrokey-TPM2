Testing
=======

.. contents:: Contents
  :depth: 2
  :local:
  :backlinks: none

Introduction
------------

The LUKS support of ZFSBootMenu is tested at two levels, both kept in ``tests/luks``:

Unit suites (``tests/luks/unit``)
  Shell scripts that run ZFSBootMenu's LUKS code (``zfsbootmenu-luks.sh`` and ``zfsbootmenu-luks-keys.sh``) against
  real LUKS1 and LUKS2 headers on loop devices, a software TPM, real ``systemd-cryptenroll``, clevis and GnuPG, and
  stand-ins for FIDO2 keys and YubiKeys. Device-mapper and ``zpool`` are replaced by stubs, so the suites check the
  decisions the code makes: what it asks, which method it uses, what it opens and closes, and what it leaves behind.

Virtual-machine tests (``tests/luks/vm``)
  Python scripts that boot ZFSBootMenu images in QEMU and follow every boot from the firmware or kernel to the booted
  system. The images are built by the build container from the source tree under test, so they contain OpenZFS 2.4.4
  and the container's cryptsetup, systemd and GnuPG. Every pool is created with the ZFS 2.4.4 tools of those images,
  and the boot environment that ZFSBootMenu hands over to also runs OpenZFS 2.4.4 (Linux 7.3, dracut with the ``zfs``
  and ``crypt`` modules, systemd-cryptsetup). The tests cover the vdev layouts, key management, root mount failures,
  boot-time races and the procedures in this documentation.

Requirements
------------

Both levels run as root on a Linux host.

Unit suites
  ``cryptsetup`` 2.4 or later, ``systemd-cryptenroll``, ``tpm2-tools``, ``swtpm`` and ``swtpm_setup``, clevis with
  its TPM2 pin, GnuPG, ``e2fsprogs``, ``dosfstools``, ``sfdisk`` and ``unshare`` (util-linux). Each suite runs in a
  private mount namespace with its own empty ``/dev/mapper``, and detaches only the loop devices it created, so it does
  not touch the host's device-mapper devices. To run them with the tool versions of the build container instead of the
  host's, use ``run-in-container.sh``: it needs Docker or Podman and runs the container privileged with the host's
  ``/dev``.

Virtual-machine tests
  QEMU (``qemu-system-x86_64``) with OVMF firmware, ``swtpm``, ``tpm2-tools``, ``systemd-cryptenroll``, clevis and
  GnuPG (for the key-management setup), ``sgdisk``, ``sfdisk``, ``mkfs.ext4``, ``mkfs.vfat``, mtools, Python 3 with
  ``pexpect``, and Docker or Podman with the build container image (``localhost/zbm-builder``, see
  :doc:`/general/container-building`). They use about 8 GB in the work directory. With KVM, the whole run takes about
  an hour; with software emulation (TCG), several hours. The harness writes partition tables for 4 KiB sectors itself
  and maps partitions of disk images to loop devices by offset, so it needs neither util-linux 2.40 nor a kernel that
  scans partitions on loop devices.

Running the Tests
-----------------

.. code-block:: bash

  # tests/luks/unit/run-all.sh
  # tests/luks/unit/run-in-container.sh
  # tests/luks/vm/run-all.sh
  # tests/luks/vm/run-all.sh layouts-main complex-hotplug

``vm/run-all.sh`` runs every VM test, or the named ones, one virtual machine at a time, and prints a summary. Setups
must run before the scenarios that use their disks (``complex-setup`` before ``complex-healthy`` and so on); a
setup recreates its disks. The groups (``layouts``, ``complex``, ``keys``, ``firmware`` and ``procedures``) use
separate disks, so several ``run-all.sh`` processes can run different groups at the same time, given the memory
(4 GB per virtual machine) and processors. Set ``ZBMTEST_REUSE=1`` for parallel runs, so that they do not rebuild the
shared ZFSBootMenu images at the same time.

Environment variables of the VM tests:

=======================  ===========================================================================================
Variable                 Meaning
=======================  ===========================================================================================
``ZBMTEST_WORK``         work directory for disks, images and logs (default */var/tmp/zbm-luks-test*)
``ZBMTEST_IMAGE``        build container image (default ``localhost/zbm-builder``)
``ZBMTEST_RUNTIME``      ``docker`` or ``podman`` (default: ``docker`` when installed)
``ZBMTEST_ACCEL``        ``kvm`` or ``tcg`` (default: ``kvm`` when */dev/kvm* is usable)
``ZBMTEST_REUSE``        ``1`` reuses ZFSBootMenu test images built earlier in the work directory
``ZBMTEST_OLD_KERNEL``,  kernel and initramfs of a ZFSBootMenu image with an older ZFS, for the ``oldzfs`` tests;
``ZBMTEST_OLD_INITRD``   those tests are skipped when unset, and pass the test images' host ID with ``spl_hostid=``
=======================  ===========================================================================================

Each VM test writes to ``$ZBMTEST_WORK/tests/<test>/``:

* ``steps.log``: every command, prompt and check;
* ``serial-<boot>.log``: the serial console of each boot;
* ``result.json``: the number of passed and failed checks and the failures.

The unit suites write their output to */var/tmp/zbm-luks-unit*, the tool versions to *tools.txt*, and the details of
each case to */tmp/z?/cases*.

Unit Suites
-----------

``unlock.sh``
  Core unlocking and teardown: automatic discovery (one prompt for volumes with a shared passphrase, non-ZFS volumes
  closed again), empty answers, end of input, retries, ``zbm.luks=off``, ``zbm.luks=`` lists and missing devices,
  ``zbm.luks_key=path:LABEL=``, crypttab key files, ``noauto``, ``nofail``, key scripts, missing key files, duplicate
  devices, volumes already open, the teardown (exporting pools on LUKS, closing mappings, keeping busy ones),
  ``zluks close``, and device specification resolution.

``tokens.sh``
  Key management with a software TPM: TPM2 tokens with and without a PIN through systemd's plugin and the native
  code, wrong and empty PINs, PCR policies after the PCR changed, the TPM dictionary-attack lockout, clevis TPM2,
  disabling tokens; GnuPG key files encrypted symmetrically and to a public key, with the secret key or card absent;
  YubiKey challenge-response in both modes; FIDO2 tokens with and without a PIN, and with the key absent; secrets
  shared by several volumes.

``edge.sh``
  Failure paths and races: a TPM2 token whose key slot was removed; TPM2 and FIDO2 tokens on one volume without a TPM;
  listing every token of a volume, also when ``cryptsetup luksDump`` prints later tokens to standard error (systemd 261
  token plugins); a clevis token behind a TPM2 token with a PIN, and in a detached header; LUKS1 volumes; key scripts
  versus tokens; detached headers; wrong key files; GnuPG files with two recipients; unlocking without a crypttab;
  devices and FIDO2 keys that appear while ZFSBootMenu waits; volumes opened by another session during a prompt; two
  sessions unlocking one volume; the import retry loop (skipped volumes not offered again, nothing new to do,
  ``zbm.luks=off``, no ``cryptsetup``); everything failing; pools on a partition of a mapping and mixed LUKS and plain
  pools at teardown.

``config.sh``
  Configuration parsing: crypttab without a final newline, with CRLF line endings, tabs, blank lines and comments;
  upper-case ``UUID=``, ``PARTUUID=`` and bare UUIDs; ``luks-<UUID>`` names; repeated devices and names; mapping
  names already in use; unknown device specifications; non-LUKS devices; invalid ``key-slot=``, ``keyfile-offset=``,
  ``keyfile-size=`` and ``tries=``; key files inside larger files; empty and directory key files; passphrases with
  spaces, quotes, backslashes, ``$``, non-ASCII characters, a leading dash and 400 characters; volumes with different
  passphrases; USB key sticks that arrive late, never arrive (with and without ``zbm.luks_token_wait=0``), serve two
  volumes, are missing during the import retry, or lack the key file.

Virtual-Machine Tests
---------------------

Images and boot environment
  The ZFSBootMenu images are built by the build container from the source tree, with test hooks that print
  ``zpool status``, ``zluks status`` and the state at teardown (devices still mapped, pools still imported). The test
  boot environment is built by the container too: the Ubuntu kernel, a dracut initramfs with the ZFS 2.4.4 ``zfs``
  module and systemd-cryptsetup, which unlocks the volumes with key files and ``rd.luks.uuid=``/``rd.luks.key=`` or
  ``luks.*`` parameters (no crypttab), and a minimal root file system whose ``/sbin/init`` reports the kernel, the ZFS
  version, the root file system, the device-mapper table, every pool's status and feature states, writes to the root
  dataset, scrubs every pool and powers off.

Following a boot
  The harness answers prompts (LUKS passphrases, ZFS keys, TPM2 PINs, GnuPG passphrases) and records them, so tests
  check exactly which prompts appeared. It hot-plugs disks and USB sticks through the QEMU monitor.

``layouts.py``: main vdev layouts
  Pools are created in ZFSBootMenu's recovery shell with ``compatibility=openzfs-2.4-linux`` (except one), data is
  written, then the machine boots again with automatic unlocking.

  * ``layouts-main``: single disk, 2-way mirror, 3-way mirror, striped mirrors and RAIDZ1 on 13 LUKS volumes; the
    root is on the 2-way mirror.
  * ``layouts-parity``: RAIDZ2, RAIDZ3 created without a compatibility set (every default feature, zstd and blake3
    active), and dRAID ``draid2:3d:1s`` as the root pool, on 15 volumes; the RAIDZ2 pool also activates the read-only
    compatible features ``fast_dedup``, ``block_cloning_endian`` and ``physical_rewrite``.
  * ``layouts-parity-oldzfs``: the parity pools booted by an image with an older ZFS: every pool imported with the
    *unsupported features* warning, the RAIDZ2 pool despite its active read-only compatible features, and the
    ZFS 2.4.4 boot environment booted.

  Checked: one passphrase prompt for all volumes; every pool ``ONLINE`` in ZFSBootMenu with its vdev layout on LUKS
  mappings; a clean teardown; in the boot environment, ZFS 2.4.4, the root on the expected pool, every pool imported
  ``ONLINE``, the root writable, every scrub clean; ``dynamic_gang_header`` enabled after ``zpool create``, with and
  without a compatibility set.

``complex.py``: complex pools, failures and races
  A RAIDZ2 pool with special, dedup and log mirrors, a cache and a spare, expanded by RAIDZ expansion; a mirror of a
  LUKS volume with its own passphrase and a plain disk; a mirror of a LUKS partition and a plain partition; the boot
  environment's root uses ZFS native encryption. The setup activates ``raidz_expansion``, ``fast_dedup``,
  ``block_cloning``, ``block_cloning_endian``, ``longname``, ``physical_rewrite``, ``dynamic_gang_header`` (by forcing
  gang blocks), ``zstd_compress``, ``blake3``, ``allocation_classes`` and ``encryption``.

  * ``complex-healthy``: automatic unlocking with two passphrases; every pool ``ONLINE``, cache ``ONLINE``, spare
    ``AVAIL``, five RAIDZ2 members after expansion; the ZFS key asked by ZFSBootMenu and by the boot environment;
    every feature still active in the boot environment.
  * ``complex-crypttab``: the same through an embedded crypttab mixing ``UUID=`` (also upper case), ``ID=`` and
    ``PARTLABEL=``.
  * ``complex-degraded``: a RAIDZ2 member and the plain mirror half missing; both pools ``DEGRADED`` in ZFSBootMenu
    and in the boot environment, whose command line for this boot leaves out the missing volume
    (``zbm.kcl_override``, as *MOD+E* would).
  * ``complex-hotplug``: ``zbm.luks=`` lists every volume and one disk is missing; it is plugged in while ZFSBootMenu
    waits.
  * ``complex-hotplug-auto``: only the auxiliary disks are present; the others, LUKS volumes and the unencrypted
    halves of the mirrors, are plugged in during one interval of the import retry loop (``zbm.retry_delay=60``) and
    unlocked before the next import.
  * ``complex-badkey``: three wrong ZFS keys end in the recovery shell (a root mount failure) with every volume open
    and every pool imported; the key can be loaded there.
  * ``complex-skip-recover``: every passphrase prompt skipped; only the mirrors with unencrypted halves import
    (``DEGRADED``), so ZFSBootMenu finds no boot environment and opens its emergency shell; ``zluks unlock`` opens the
    volumes there, and after the shell ZFSBootMenu imports ``tank`` and boots it from its menu.
  * ``complex-oldzfs``: an image with an older ZFS unlocks the volumes and imports the pools without unsupported
    active features, but not the RAIDZ2 pool with ``raidz_expansion`` active, so it boots nothing; ``zpool import``
    names the unsupported feature.

``keys.py``: key management
  Four partitions enrolled on the host with ``systemd-cryptenroll`` against a software TPM (PIN 1234 on two volumes;
  on the third a TPM2 token with another PIN plus a clevis TPM2 binding as second token) and a GnuPG-encrypted key
  file on the fourth; pools created with ZFS 2.4.4.

  * ``keys-plugin``: systemd's token plugin; a wrong PIN, the right PIN (reused for the second volume), the third
    volume opened silently by its second token, the GnuPG passphrase; boot into the boot environment.
  * ``keys-native``: the same with ``zfsbootmenu_luks_token_plugins="no"`` (``tpm2-tools``).
  * ``keys-lockout``: the TPM allows one wrong PIN; ZFSBootMenu explains the lockout and falls back to the passphrase.
  * ``keys-notpm``: no TPM; the TPM2 volumes fall back to the passphrase without a PIN prompt.

``firmware.py``: UEFI boot from NVMe, without a crypttab
  An NVMe disk with 4 KiB sectors, an EFI system partition holding the ZFSBootMenu EFI image as
  *EFI/BOOT/BOOTX64.EFI*, and a LUKS2 partition with 4 KiB encryption sectors and the TRIM and work-queue flags stored
  in its header. ZFSBootMenu selects the volume with ``zbm.luks=UUID=``; the boot environment uses ``luks.uuid=``,
  ``luks.key=`` and ``luks.crypttab=no``.

  * ``firmware-boot``: one passphrase prompt; in the boot environment no prompt, 4096-byte sectors, flags active.
  * ``firmware-usbkey``: ``zbm.luks_key=`` names a key file on a FAT USB stick; no prompt.
  * ``firmware-usblate``: the stick is inserted while ZFSBootMenu waits for it; no prompt.
  * ``firmware-nostick``: no stick; ZFSBootMenu waits, then asks for the passphrase.

``procedures.py``: the documentation, as written
  The commands of :doc:`vdev-configurations` and of the pool sections of :doc:`administration` are extracted from the
  documentation (``docs.py``) and run with ZFS 2.4.4 and cryptsetup's default key derivation. Placeholder device names
  such as *ata-DISK1-part2* are links to virtual disks; only values the documentation abbreviates (UUIDs, GUIDs) are
  filled in. It runs Examples 1-12 (without the ``nvme-cli`` and installed-system steps of Example 12), the refusal of
  ``zpool add`` for a two-way special mirror on RAIDZ2, ``pool_luks_devices``, removing a cache vdev, replacing a failed
  disk, RAIDZ expansion, turning a single disk into a mirror, adding a vdev, upgrading pools created with the
  ``openzfs-2.3-linux`` set as in :doc:`zfs-versions` (``zpool upgrade`` leaves ``dynamic_gang_header`` disabled, with
  the 2.4 set and with the property ``off``) and making the pool bootable, then reboots and checks the verification
  step.

Results
-------

Latest run: 20 October 2026, with software emulation (TCG). The build container image was Ubuntu 26.10 with
Linux 7.3.0-9-generic, OpenZFS 2.4.4 (``zfs-2.4.4-1ubuntu4``), cryptsetup 2.8.7, systemd 261 and dracut 112. The
``oldzfs`` tests used a ZFSBootMenu image built on Ubuntu 24.04 with OpenZFS 2.2.2 and Linux 6.8.

Unit suites
~~~~~~~~~~~

The same results on the host and in the build container:

=============  ======  ======
Suite          Passed  Failed
=============  ======  ======
``unlock.sh``  52      0
``tokens.sh``  51      0
``edge.sh``    81      0
``config.sh``  64      0
=============  ======  ======

Host tools: cryptsetup 2.7.0, systemd 255, GnuPG 2.4.4, tpm2-tools 5.6, clevis 20, swtpm 0.7.3. Container tools:
cryptsetup 2.8.7, systemd 261, GnuPG 2.4.9, tpm2-tools 5.7, clevis 23, swtpm 0.10.1.

Virtual-machine tests
~~~~~~~~~~~~~~~~~~~~~

The images were built by the build container from this source tree. When a test found a problem, the problem was
fixed and the tests that exercise the changed code ran again on rebuilt images; the table lists the last run of each
test. Durations are with software emulation and up to three virtual machines at a time.

=========================  ======  ======  ========
Test                       Passed  Failed  Duration
=========================  ======  ======  ========
``layouts-main``           44      0       16 min
``layouts-parity``         37      0       21 min
``layouts-parity-oldzfs``  16      0       8 min
``complex-setup``          19      0       7 min
``complex-healthy``        37      0       16 min
``complex-crypttab``       36      0       15 min
``complex-degraded``       30      0       16 min
``complex-hotplug``        36      0       14 min
``complex-hotplug-auto``   36      0       16 min
``complex-badkey``         4       0       12 min
``complex-skip-recover``   40      0       16 min
``complex-oldzfs``         4       0       3 min
``keys-setup``             9       0       3 min
``keys-plugin``            13      0       6 min
``keys-native``            13      0       8 min
``keys-lockout``           13      0       8 min
``keys-notpm``             13      0       8 min
``firmware-setup``         3       0       2 min
``firmware-boot``          14      0       3 min
``firmware-usbkey``        14      0       3 min
``firmware-usblate``       14      0       4 min
``firmware-nostick``       14      0       4 min
``procedures``             78      0       47 min
Total                      537     0
=========================  ======  ======  ========

Problems found and fixed
~~~~~~~~~~~~~~~~~~~~~~~~

Each fix of ZFSBootMenu's code has a regression test that fails without it.

* With cryptsetup 2.8 and systemd 261's token plugins, ``cryptsetup luksDump`` prints the sections of all tokens
  after the first to standard error, so ZFSBootMenu saw only the first token of a volume. Tokens are now read from the
  JSON metadata (``edge.sh``).
* A clevis token behind another token, such as a TPM2 token with a PIN, did not unlock its volume in images with
  systemd's token plugins: ``clevis luks pass`` finds the token in the ``luksDump`` text, which lacks it there.
  ZFSBootMenu asked for the TPM2 PIN and then the passphrase. clevis tokens are now decrypted from the token JSON,
  which also works with detached headers (``keys-plugin``, ``edge.sh``).
* Boot-time race in the import retry loop: each retry imported first and unlocked LUKS volumes on late disks only
  when the import failed. When unencrypted devices of another pool arrived together with the late LUKS disks, that
  pool imported, the loop ended, and the LUKS volumes, including those of the boot pool, were never unlocked:
  ZFSBootMenu found no boot environment. Each retry now unlocks late volumes before it imports
  (``complex-hotplug-auto``, ``edge.sh``).
* After ``zluks unlock`` in the emergency shell that ZFSBootMenu opens when it finds no boot environment (here
  because only pools with unencrypted devices could be imported), leaving the shell did not import the pools that had
  become importable, so ZFSBootMenu found no boot environment again, although **zluks**\ (8) says that it retries the
  import. ``zfsbootmenu-init`` now imports them, read-only as at start-up, before it shows the menu again
  (``complex-skip-recover``).
* Two sessions unlocking at the same time, such as the console and an SSH session, could record one volume
  twice, so ``zluks status`` listed it twice (15 of 200 rounds of the race test). Recording is now serialized
  (``edge.sh``).
* With ``zbm.luks_token_wait=08`` or ``09``, ZFSBootMenu did not wait for the key device (a shell arithmetic
  error), and ``010`` waited 8 seconds instead of 10. The value is now read as a decimal number of up to six digits
  (``config.sh``).
* The unit suites closed device-mapper devices and loop devices of the host when cleaning up. Each suite now runs in a
  private mount namespace with its own ``/dev/mapper`` and detaches only its own loop devices.
* Documentation: Example 1 of :doc:`vdev-configurations` named an NVMe device although the conventions and the
  other examples use *ata-DISK1*. Example 12 makes switching the SSDs to 4 KiB sectors optional, but on an SSD that
  keeps 512-byte sectors, ``luksFormat --sector-size 4096`` fails on a partition running to the end of the disk, whose
  size is not a multiple of 4 KiB; the example now says to give the partition a size in MiB. Example 7 said that
  ``zpool add`` needs ``-f`` for a special mirror on a RAIDZ pool; OpenZFS 2.4 accepts a mirror whose redundancy
  matches the parity level without it.

Observations
~~~~~~~~~~~~

* Forcing gang blocks for every block larger than 8 KiB (``metaslab_force_ganging=8192``, a ZFS debugging
  parameter) while writing to a dataset with ``dedup=on``, ``compression=zstd`` and ``checksum=blake3`` on the complex
  pool did not finish within 15 minutes under emulation. ``complex.py`` forces gang blocks only for 128 KiB records in
  a dataset without deduplication and compression.
* With a disk missing, the boot environment's dracut initramfs waited for the volume named with ``rd.luks.uuid=``
  and dropped to its emergency shell, after ZFSBootMenu had imported the degraded pools. :doc:`administration`
  describes booting with a failed disk.
* Boot-time race: when the late disks were plugged in one by one while the retry loop ran every 4 seconds,
  ZFSBootMenu unlocked the volumes present at one retry, imported all three pools ``DEGRADED`` and booted; the volumes
  of the disks that arrived seconds later stayed locked. The loop is designed to end once a pool imports;
  :doc:`/general/luks` and **zfsbootmenu-luks**\ (7) now explain how to wait for slow disks (``zbm.wait_for``).
  Pools imported only through unencrypted devices stay imported read-only at ``kexec``, as in ZFSBootMenu without
  LUKS, which only exports pools it imported read-write.
* Once, the emulator hung while the guest kernel started its second processor, before ZFSBootMenu ran
  (``layouts-main``, in its final run); the test passed when run again.
* An image with another host ID than the booted system does not import the booted system's root pool as long as it
  can import other pools; see the note in :doc:`zfs-versions`. The ``oldzfs`` tests therefore pass the host ID with
  ``spl_hostid=``.

Not Covered
-----------

* Real YubiKey, Nitrokey and other FIDO2 or OpenPGP devices: the tests use stand-ins for them.
* Images built with mkinitcpio: the build container has no mkinitcpio.
* Secure Boot.
* Clevis tang policies, which need a network server.
* Boot environments whose initramfs is not built by dracut (for example mkinitcpio or initramfs-tools).
* KVM: the latest run used software emulation only, which changes timing but not the code paths.
