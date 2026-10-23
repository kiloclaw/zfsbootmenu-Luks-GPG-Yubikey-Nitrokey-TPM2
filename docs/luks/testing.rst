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

Virtual-machine tests (``crates/zbm-vmtest``)
  A Rust program that boots ZFSBootMenu images in QEMU and follow every boot from the firmware or kernel to the booted
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
  QEMU (``qemu-system-x86_64``) with OVMF firmware, ``swtpm``, ``tpm2-tools``, ``systemd-cryptenroll`` (in the
  ``systemd-cryptsetup`` package on Ubuntu 24.04 and Debian 13 or later), clevis and GnuPG (for the key-management
  setup), ``sgdisk``, ``sfdisk``, ``mkfs.ext4``, ``mkfs.vfat``, mtools, the Rust toolchain (to build the harness), and Docker or
  Podman with the build container image (``localhost/zbm-builder``, see :doc:`/general/container-building`).
  ``runtime-rust-crash`` also runs ``cargo xtask symbolize`` on the host, which needs the Rust toolchain and
  ``addr2line`` (binutils); started with ``sudo``, it runs as the user who started it, with their
  *~/.cargo/bin*. They use about 8 GB in the work directory. With KVM, the whole run takes about
  an hour; with software emulation (TCG), several hours. The harness writes partition tables for 4 KiB sectors itself
  and maps partitions of disk images to loop devices by offset, so it needs neither util-linux 2.40 nor a kernel that
  scans partitions on loop devices.

Running the Tests
-----------------

.. code-block:: bash

  # tests/luks/unit/run-all.sh
  # tests/luks/unit/run-in-container.sh
  $ cargo build --release -p zbm-vmtest
  # target/release/zbm-vmtest run
  # target/release/zbm-vmtest run layouts-main complex-hotplug

``zbm-vmtest run`` runs every VM test, or the named ones, one virtual machine at a time, and prints a summary. Setups
must run before the scenarios that use their disks (``complex-setup`` before ``complex-healthy`` and so on); a setup
recreates its disks. The groups (``layouts``, ``complex``, ``keys``, ``firmware`` and ``procedures``) use separate
disks, as does ``runtime``, so several ``zbm-vmtest run`` processes can run different groups at the same time, given the
memory (4 GB per virtual machine) and processors. Set ``ZBMTEST_REUSE=1`` for parallel runs, so that they do not
rebuild the shared ZFSBootMenu images at the same time.

Environment variables of the VM tests:

=======================  ===========================================================================================
Variable                 Meaning
=======================  ===========================================================================================
``ZBMTEST_WORK``         work directory for disks, images and logs (default */var/tmp/zbm-luks-test*)
``ZBMTEST_IMAGE``        build container image (default ``localhost/zbm-builder``)
``ZBMTEST_RUNTIME``      ``docker`` or ``podman`` (default: ``docker`` when installed)
``ZBMTEST_ACCEL``        ``kvm`` or ``tcg`` (default: ``kvm`` when */dev/kvm* is usable)
``ZBMTEST_REUSE``        ``1`` reuses ZFSBootMenu test images built earlier in the work directory from the same
                         inputs (crypttab, embedded files, configuration); after a change to the code in the
                         images, run once without it
``ZBMTEST_OLD_KERNEL``,  kernel and initramfs of a ZFSBootMenu image with an older ZFS, for the ``oldzfs`` tests;
``ZBMTEST_OLD_INITRD``   those tests are skipped when unset, and pass the test images' host ID with ``spl_hostid=``
``ZBMTEST_ZBM_RUNTIME``  ``legacy`` (the default) or ``rust``: the runtime of the ZFSBootMenu images. With ``rust``,
                         image and result names end in ``-rust`` (``complex-healthy-rust``); the ``runtime`` tests
                         choose their runtime themselves
``ZBMTEST_SHADOW``       ``0`` builds legacy images without shadow mode. By default they run ``zbm shadow`` before
                         each boot, and a test fails when a summary on its serial consoles reports differences
                         (*docs/dev/shadow-mode.md*)
=======================  ===========================================================================================

Each VM test writes to ``$ZBMTEST_WORK/tests/<test>/``:

* ``steps.log``: every command, prompt and check;
* ``serial-<boot>.log``: the serial console of each boot;
* ``result.json``: the number of passed and failed checks and the failures;
* ``qemu-<boot>.log``: QEMU's own messages.

The harness is built as yourself and run as root; ``cargo xtask test vm [<test>...]`` does both, running the harness
with ``sudo -E`` unless it already runs as root. ``zbm-vmtest list`` lists the tests in the order ``run`` runs them. ``zbm-vmtest compare <work>
<work>`` compares the checks of two runs (two ``ZBMTEST_WORK`` directories, such as those of two commits) message by
message. The harness reads the serial console from a Unix socket in a thread, which feeds the log, a terminal emulator
(the screen as a user sees it) and the text that patterns match, with the terminal's escape sequences removed. A test
that cannot go on (a setup command that fails, a message that does not come in time) records a failed check with the
reason and still writes its result. ``zbm-vmtest run runtime-record-zfs`` (``cargo xtask record-zfs``) and
``runtime-record-fixtures`` are not tests: they record what ``zfs`` and ``zpool`` print as the fixtures of
``zbm-zfs``.

The harness replaced a Python one, whose results it reproduced check for check before the Python harness was removed
(``REFACTOR-ROADMAP.md``, R3.6).

The unit suites write their output to */var/tmp/zbm-luks-unit*, the tool versions to *tools.txt*, and the details of
each case to */tmp/z?/cases*. Run one ``unit/run-all.sh`` at a time: two runs share that directory, the loop devices
backed by its images, and the ports and pid files of their software TPMs. A suite waits for a lock
(*/run/lock/zbm-luks-unit.lock*) while a suite of another run holds it, but the two runs still write the same output
files. Each suite hides the host's */dev/hidraw\** devices, which the code would take for FIDO2 keys, and the
``blkid`` stub's device searches find only loop devices, so that discovery leaves the host's own LUKS disks alone.

The suites load the code they test through ``unit/impl.sh``, which ``ZBMTEST_IMPL`` selects: ``legacy`` (the default)
for the Bash libraries, ``rust`` for the Rust runtime of the port. ``impl.sh`` also provides, and documents, every seam
through which the suites observe and control the code under test: the log file, the recorded and answered timed
prompts, the stand-ins for open volumes, and the six operations the suites call. A new case uses only these seams, so
that it runs against both implementations; the cases that reach inside the legacy code are marked as legacy-only, and
Rust unit tests cover them for the Rust runtime.

With ``ZBMTEST_IMPL=rust``, the suites run a test build of ``zbm`` (the Cargo feature ``test-seams``), whose hidden
command ``zbm-luks-test`` runs the operations with the seams. ``impl.sh`` uses ``ZBMTEST_ZBM``, by default
*target/test-seams/debug/zbm*, and builds it with ``cargo`` when it is missing:

.. code-block:: bash

  $ CARGO_TARGET_DIR=target/test-seams cargo build -p zbm --features test-seams
  # ZBMTEST_IMPL=rust tests/luks/unit/run-all.sh

Images never contain the test build; CI checks that the release ``zbm`` has no ``zbm-luks-test`` command.

The suites are also ported to Rust, case for case and check for check, in *crates/zbm-luks/tests/devices*. The Rust
suites call the LUKS code directly instead of through the seams, with the same real volumes, software TPM, clevis and
GnuPG, and the same stand-ins for device-mapper and security keys. They run the 252 checks of
``ZBMTEST_IMPL=rust``; the three legacy-only checks are unit tests. As root, in the build container or on a host with
the tools above:

.. code-block:: bash

  # cargo xtask test devices

The Bash suites stay until the legacy runtime is removed: they are the only suites that run against it.

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

``layouts``: main vdev layouts
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

``complex``: complex pools, failures and races
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

``keys``: key management
  Four partitions enrolled on the host with ``systemd-cryptenroll`` against a software TPM (PIN 1234 on two volumes;
  on the third a TPM2 token with another PIN plus a clevis TPM2 binding as second token) and a GnuPG-encrypted key
  file on the fourth; pools created with ZFS 2.4.4.

  * ``keys-plugin``: systemd's token plugin; a wrong PIN, the right PIN (reused for the second volume), the third
    volume opened silently by its second token, the GnuPG passphrase; boot into the boot environment.
  * ``keys-native``: the same with ``zfsbootmenu_luks_token_plugins="no"`` (``tpm2-tools``).
  * ``keys-lockout``: the TPM allows one wrong PIN; ZFSBootMenu explains the lockout and falls back to the passphrase.
  * ``keys-notpm``: no TPM; the TPM2 volumes fall back to the passphrase without a PIN prompt.

``firmware``: UEFI boot from NVMe, without a crypttab
  An NVMe disk with 4 KiB sectors, an EFI system partition holding the ZFSBootMenu EFI image as
  *EFI/BOOT/BOOTX64.EFI*, and a LUKS2 partition with 4 KiB encryption sectors and the TRIM and work-queue flags stored
  in its header. ZFSBootMenu selects the volume with ``zbm.luks=UUID=``; the boot environment uses ``luks.uuid=``,
  ``luks.key=`` and ``luks.crypttab=no``.

  * ``firmware-boot``: one passphrase prompt; in the boot environment no prompt, 4096-byte sectors, flags active.
  * ``firmware-usbkey``: ``zbm.luks_key=`` names a key file on a FAT USB stick; no prompt.
  * ``firmware-usblate``: the stick is inserted while ZFSBootMenu waits for it; no prompt.
  * ``firmware-nostick``: no stick; ZFSBootMenu waits, then asks for the passphrase.

``procedures``: the documentation, as written
  The commands of :doc:`vdev-configurations` and of the pool sections of :doc:`administration` are extracted from the
  documentation and run with ZFS 2.4.4 and cryptsetup's default key derivation. Placeholder device names
  such as *ata-DISK1-part2* are links to virtual disks; only values the documentation abbreviates (UUIDs, GUIDs) are
  filled in. It runs Examples 1-12 (without the ``nvme-cli`` and installed-system steps of Example 12), the refusal of
  ``zpool add`` for a two-way special mirror on RAIDZ2, ``pool_luks_devices``, removing a cache vdev, replacing a failed
  disk, RAIDZ expansion, turning a single disk into a mirror, adding a vdev, upgrading pools created with the
  ``openzfs-2.3-linux`` set as in :doc:`zfs-versions` (``zpool upgrade`` leaves ``dynamic_gang_header`` disabled, with
  the 2.4 set and with the property ``off``) and making the pool bootable, then reboots and checks the verification
  step.

``runtime``: start-up and pool import with each runtime, without LUKS
  Two unencrypted pools hold the test boot environment as their ``bootfs``: ``tplain`` on one disk, exported by this
  host, and ``tforeign`` on another, left imported by a host with the ID ``deadbeef``. The tests check the runtime
  itself (Phase 5 and package A of Phase 6 in ``PORT-ROADMAP.md``); every scenario runs with the legacy image first,
  which is the reference, then with an image built with ``zfsbootmenu_runtime="rust"`` (``runtime-rust-*``).

  * ``runtime-setup``: creates the pools in the recovery shell of the legacy image.
  * ``runtime-legacy``, ``runtime-rust``: ``tplain`` boots after the countdown; process 1 is the runtime's
    start-up program (the Rust supervisor), and hooks see ``BOOTFS``.
  * ``-hostid``: ``tforeign`` alone: host ID matching imports it, and the boot environment gets ``spl_hostid``.
  * ``-strict``: ``tforeign`` alone with ``zbm.import_policy=strict``: no import, the retry prompt appears.
  * ``-force``: ``tforeign`` alone with ``zbm.import_policy=force``: imported at once.
  * ``-prefer``: both pools with ``zbm.prefer=tplain!!``: only ``tplain`` is imported and booted.
  * ``-late``: the disk of ``tplain`` is plugged in during the import retry loop.
  * ``-waitfor``: ``zbm.wait_for`` names that disk, which is plugged in while ZFSBootMenu waits.
  * ``-hooks``: ``zbm.hookroot`` adds a teardown hook from a FAT disk; ``zbm.skip_hooks`` disables a test hook.
  * ``-signals``: the signal dispositions (``SigBlk``, ``SigIgn``, ``SigCgt``) of an early-setup hook, of ``fzf`` in
    the menu and of the recovery shell, on the console. The legacy run records them; the Rust run must reproduce
    those of the hook and the shell (its menu arrives with package E).

* ``runtime-rust-pilot`` runs the pilots of ``REFACTOR-ROADMAP.md`` in the Rust image, from the recovery shell on the
  serial console: the new menu (``zbm-menu-pilot``) with ``TERM=vt100`` and ``TERM=linux``, where typing filters the
  list and Enter chooses ``tplain/ROOT/be``, with the frame times and serial bytes it reports; and
  ``zluks --pilot open`` on a LUKS volume formatted in the VM, refused with a wrong key file and opened with the
  right one, with its JSON journal.

* ``runtime-rust-autosize`` boots the Rust image, with the ``virtio_gpu`` driver added, on a virtio GPU with a
  3840×2400 display, and runs ``zbm console-autosize --console /dev/tty1`` from the recovery shell on the serial
  console. With the EDID, which QEMU gives 100 dpi, it must load the 20-pixel font (384×120); with ``edid=off``, the
  32-pixel font of the legacy rule (240×75); with ``zbm.autosize=48``, the 24-pixel font doubled (160×50). At boot
  the hook must leave the serial console alone. A second image, built with ``ConsoleFont: spleen`` in the
  ``generate-zbm`` configuration, must hold the four Spleen fonts and their licence and no Terminus font, load
  Spleen 8×16 with the EDID (480×150), and the native Spleen 32×64 with ``zbm.autosize=64`` (120×37).

* ``runtime-rust-colours`` checks that the console fonts of the Rust image leave the console its 16 colours
  (ROADMAP.md #20). The image adds the ``virtio_gpu`` driver and, as a control, the whole 512-glyph Terminus 8×14. For
  each font, the test draws a bar of the 16 foreground colours on ``/dev/tty1`` (the 8 normal ones, and the 8 bright
  ones bold gives, as the menu's theme draws them), takes a screenshot with the QEMU monitor's ``screendump``, kept as
  ``<boot>-<font>.ppm``, and counts the colours of the bar's line. On QEMU's standard VGA, the VGA text console, the
  image's 256-glyph font must show 16 colours and the whole font 8, which shows that the test sees the loss; on a
  virtio GPU, the framebuffer console, the image's font must show 16, and the count of the whole font is noted.

* ``runtime-rust-drm`` runs the menu prototype on the display without a terminal (ROADMAP.md #19, step 1), in a Rust
  image built with the ``menu-demo`` feature and the ``virtio_gpu`` and ``evdev`` drivers. It boots on a virtio GPU and
  on QEMU's standard VGA in a VESA mode (``vga=0x318``), which ``simpledrm`` drives. Each time, ``zbm debug
  menu-demo --wait --dump /tmp/menu.txt`` runs in the background from the serial console: card0's device must be
  driven by ``virtio-pci`` (the PCI function under ``virtio_gpu``) or ``simple-framebuffer``, the screen's text must
  show the main view, a QEMU screenshot (``<boot>-menu.ppm``) at least 6 colours, Down typed with ``sendkey`` must move
  the selection, Enter must boot and end the demo, and afterwards the kernel console must draw a 16-colour bar again
  (``<boot>-console.ppm``).

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

With ``ZBMTEST_IMPL=rust``, the suites pass 252 checks against the Rust runtime; the other three reach inside the
Bash code, and Rust unit tests cover them.

Host tools: cryptsetup 2.7.0, systemd 255, GnuPG 2.4.4, tpm2-tools 5.6, clevis 20, swtpm 0.7.3. Container tools:
cryptsetup 2.8.7, systemd 261, GnuPG 2.4.9, tpm2-tools 5.7, clevis 23, swtpm 0.10.1.

Virtual-machine tests
~~~~~~~~~~~~~~~~~~~~~

The images were built by the build container from this source tree. When a test found a problem, the problem was
fixed and the tests that exercise the changed code ran again on rebuilt images; the table lists the last run of each
test. Durations are with software emulation and up to three virtual machines at a time.

==========================  ======  ======  ========
Test                        Passed  Failed  Duration
==========================  ======  ======  ========
``layouts-main``            44      0       16 min
``layouts-parity``          37      0       21 min
``layouts-parity-oldzfs``   16      0       8 min
``complex-setup``           19      0       7 min
``complex-healthy``         37      0       16 min
``complex-crypttab``        36      0       15 min
``complex-degraded``        30      0       16 min
``complex-hotplug``         36      0       14 min
``complex-hotplug-auto``    36      0       16 min
``complex-badkey``          4       0       12 min
``complex-skip-recover``    40      0       16 min
``complex-oldzfs``          4       0       3 min
``keys-setup``              9       0       3 min
``keys-plugin``             13      0       6 min
``keys-native``             13      0       8 min
``keys-lockout``            13      0       8 min
``keys-notpm``              13      0       8 min
``firmware-setup``          3       0       2 min
``firmware-boot``           14      0       3 min
``firmware-usbkey``         14      0       3 min
``firmware-usblate``        14      0       4 min
``firmware-nostick``        14      0       4 min
``procedures``              78      0       47 min
``runtime-setup``           3       0       1 min
``runtime-legacy``          15      0       4 min
``runtime-rust``            15      0       4 min
``runtime-legacy-hostid``   15      0       5 min
``runtime-rust-hostid``     15      0       4 min
``runtime-legacy-strict``   2       0       1 min
``runtime-rust-strict``     2       0       1 min
``runtime-legacy-force``    3       0       1 min
``runtime-rust-force``      3       0       1 min
``runtime-legacy-prefer``   16      0       5 min
``runtime-rust-prefer``     16      0       4 min
``runtime-legacy-late``     15      0       3 min
``runtime-rust-late``       15      0       5 min
``runtime-legacy-waitfor``  15      0       3 min
``runtime-rust-waitfor``    15      0       4 min
``runtime-legacy-hooks``    13      0       3 min
``runtime-rust-hooks``      13      0       3 min
``runtime-legacy-signals``  3       0       1 min
``runtime-rust-signals``    9       0       1 min
Total                       740     0
==========================  ======  ======  ========

Virtual-machine tests with the Rust runtime
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The same tests, with images built with ``zfsbootmenu_runtime="rust"`` (``ZBMTEST_ZBM_RUNTIME=rust``), on
22 October 2026, and ``runtime-rust-pilot``, which exists only for the Rust image. The ``complex`` tests fail because their boot pool's root is a locked ZFS encryption root, which the
Rust runtime does not handle yet: it refuses to boot and opens the recovery shell. In ``complex-degraded`` and the two
hot-plug tests, every LUKS, import and teardown check passes and only the boot environment's checks fail; the same
holds for ``complex-healthy`` and ``complex-crypttab``, rerun later that day. ``complex-badkey`` and
``complex-skip-recover`` ended without a result; they need a rerun. The ``oldzfs`` tests were skipped, as no
OpenZFS 2.2 image was given.

==============================  ======  ======  ========
Test                            Passed  Failed  Duration
==============================  ======  ======  ========
``layouts-main-rust``           44      0       11 min
``layouts-parity-rust``         37      0       11 min
``complex-setup-rust``          19      0       6 min
``complex-healthy-rust``        13      14      6 min
``complex-crypttab-rust``       13      13      8 min
``complex-degraded-rust``       9       13      4 min
``complex-hotplug-rust``        13      13      4 min
``complex-hotplug-auto-rust``   13      13      5 min
``keys-setup-rust``             9       0       3 min
``keys-plugin-rust``            13      0       5 min
``keys-native-rust``            13      0       6 min
``keys-lockout-rust``           13      0       6 min
``keys-notpm-rust``             13      0       5 min
``firmware-setup-rust``         3       0       2 min
``firmware-boot-rust``          14      0       5 min
``firmware-usbkey-rust``        14      0       5 min
``firmware-usblate-rust``       14      0       5 min
``firmware-nostick-rust``       14      0       3 min
``procedures-rust``             78      0       27 min
``runtime-setup``               3       0       2 min
``runtime-rust``                15      0       4 min
``runtime-rust-hostid``         15      0       5 min
``runtime-rust-strict``         2       0       2 min
``runtime-rust-force``          3       0       2 min
``runtime-rust-prefer``         16      0       5 min
``runtime-rust-late``           15      0       6 min
``runtime-rust-waitfor``        15      0       6 min
``runtime-rust-hooks``          13      0       5 min
``runtime-rust-signals``        9       0       2 min
``runtime-rust-pilot``          13      0       1 min
Total                           454     90
==============================  ======  ======  ========

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
* A mapping was not recorded when its name, read as a pattern, matched one recorded earlier: ``a.b`` after ``axb``.
  The teardown hook closes only recorded mappings, so such a volume stayed open, with its key in memory, when the boot
  environment started. Names are now compared as text (``edge.sh``; found while porting the code to Rust).
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
  pool did not finish within 15 minutes under emulation. ``complex-setup`` forces gang blocks only for 128 KiB records in
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
