Testing the Rust Runtime in Shadow Mode
=======================================

ZFSBootMenu is being rewritten in Rust. Before the Rust runtime takes over a step of the boot, it has to make the same
decisions as today's runtime on real machines, with their own disks, pools and options. Shadow mode lets you check that
on your machine without changing how it boots: your image keeps booting with today's runtime, and just before each
boot, the Rust code works out what it *would* have decided and reports where the two differ.

If you can build your own ZFSBootMenu image, a few boots in shadow mode are a great help. A report of zero differences
is as useful as a report of one.

Is it safe?
-----------

Shadow mode never acts and never stops a boot:

* It runs only after ZFSBootMenu has chosen a boot environment, and today's runtime still does everything.
* It only reads. It can run a short list of read-only commands (``zfs list``, ``zfs get``, ``zpool list``,
  ``zpool get``, ``zpool status``, ``cryptsetup luksDump``, ``cryptsetup status``, ``blkid`` and a few more), never
  with a secret as input, and it refuses every command after five seconds.
* It never asks you anything, and its exit status is ignored, so a failure in shadow mode cannot hold up the boot.
* Its report holds no key material: pool names, dataset names, device paths, host IDs and kernel file names.

What it compares
----------------

Each boot, shadow mode compares these decisions:

* the host ID written to ``/etc/hostid``;
* which pools were imported, and that they were imported read-only;
* the mapping name of each LUKS volume ZFSBootMenu opened;
* the default boot environment;
* the kernel and initramfs of the boot environment that is booting;
* where the key of an encrypted boot environment came from: a passphrase prompt, a key file in the image, or a key
  file copied from its ``org.zfsbootmenu:keysource``.

A decision with nothing to compare, such as LUKS mappings on a machine without LUKS, is left out. A kernel that you
picked by hand in the menu shows as a difference, which is expected.

Building an image with shadow mode
----------------------------------

Shadow mode needs an image built from this source tree with today's runtime (``zfsbootmenu_runtime=legacy``, the
default) and with ``zbm``, the Rust program, inside it.

1. Build and install the Rust programs. ``make install`` puts ``zbm`` in ``/usr/libexec/zfsbootmenu/zbm``, where image
   builds look for it; ``zfsbootmenu_zbm=<path>`` points to another copy.

2. Turn shadow mode on.

   * With dracut, the default, create ``/etc/zfsbootmenu/dracut.conf.d/shadow.conf``::

       zfsbootmenu_shadow="yes"

   * With mkinitcpio (``Global.InitCPIO: true`` in ``config.yaml``), add the same line to
     ``/etc/zfsbootmenu/mkinitcpio.conf``.

   * With the build container (:doc:`container-building`), put ``shadow.conf`` in the ``dracut.conf.d`` subdirectory
     of the build directory, and give the container this source tree with ``zbm-builder.sh -l <path>``: without it,
     the container fetches the upstream tree, which has no shadow mode. The container builds ``zbm`` itself when it
     has ``cargo``.

3. Build the image as usual, for example with ``generate-zbm``. The build fails with
   ``zfsbootmenu_shadow: shadow mode needs zbm`` when it cannot find ``zbm``.

4. On UEFI machines, add ``zbm.shadow=efivar`` to ZFSBootMenu's kernel command line (``Kernel.CommandLine`` in
   ``config.yaml``, or ``zbm-kcl`` on the image). ZFSBootMenu's own files are gone once your system has booted, so
   this keeps the report in an EFI variable for you to read afterwards. It is written only when there are
   differences, and at most once per boot.

Keep a known-good image around while you test, as you would for any ZFSBootMenu change.

Reading the report
------------------

Just before the kernel starts, shadow mode prints one line for each difference and a summary on the console:

.. code-block:: text

  ZBM-SHADOW: default boot environment: legacy zroot/ROOT/void, rust zroot/ROOT/arch
  ZBM-SHADOW: 1 differences in 5 decisions (host ID, pools, default boot environment, kernel, key source)

With no differences, only the summary is printed, as ``ZBM-SHADOW: 0 differences in ...``. The summary also goes to
the kernel log at level *notice*, which is gone after the boot as well.

With ``zbm.shadow=efivar``, read the report after the boot:

* With ``zbm`` installed on the booted system, as root, with ``efivarfs`` mounted read-write::

    /usr/libexec/zfsbootmenu/zbm shadow report

  This prints the report and removes the variable. It prints ``no shadow report`` after a boot without differences.

* Without ``zbm``, read the variable directly (the first four bytes are the variable's attributes)::

    tail -c +5 /sys/firmware/efi/efivars/ZbmShadowSummary-5a2f3c1e-9b4d-4e7a-8c61-2d0f9b3e7a15

  Remove it with ``chattr -i`` and ``rm`` on the same file, so that the next report is written.

On a machine without UEFI, note the ``ZBM-SHADOW`` lines from the console, or from a serial console if you have one.

What to send
------------

Open an issue in the project's repository, with:

* the ``ZBM-SHADOW`` lines, or the summary line when there were no differences;
* the commit you built from (``git rev-parse --short HEAD``);
* ZFSBootMenu's kernel command line, and the ``zfsbootmenu_*`` settings of your image build;
* for a difference about pools or the default boot environment: ``zpool list -o name,bootfs,readonly`` and the
  ``org.zfsbootmenu:*`` properties of the boot environments involved (``zfs get -s local,inherited all <dataset> |
  grep org.zfsbootmenu``);
* for a difference about LUKS mappings: your ZFSBootMenu crypttab, if you use one, with any key file paths you prefer
  to keep private replaced;
* for a difference about the key source: the ``keylocation``, ``keyformat`` and ``org.zfsbootmenu:keysource``
  properties of the boot environment, and whether you booted it from the countdown or chose it in the menu.

None of this holds a passphrase or a key. Please do not send key files or the output of ``cryptsetup luksDump``.

Turning it off
--------------

Remove ``shadow.conf`` (or the line in ``mkinitcpio.conf``) and build the image again. ``zbm.skip_hooks=99-zbm-shadow.sh``
on ZFSBootMenu's command line turns it off for one boot without a new image.
