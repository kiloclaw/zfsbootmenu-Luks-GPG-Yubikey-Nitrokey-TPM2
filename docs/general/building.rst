Building ZFSBootMenu
====================

.. contents:: Contents
  :depth: 2
  :local:
  :backlinks: none

This page takes you from a checkout of this repository to a ZFSBootMenu image on your EFI system partition, including
LUKS volumes and hardware keys. Each step links to the page that describes it in detail.

What you get
------------

A build produces one or both of:

* an **EFI bundle**, a single UEFI executable that holds the kernel, the ZFSBootMenu initramfs and the kernel command
  line (``vmlinuz.EFI`` by default);
* **components**, a separate kernel and initramfs (``vmlinuz-bootmenu`` and ``initramfs-bootmenu.img`` by default),
  for a boot manager such as rEFInd or for an ``efibootmgr`` entry with its own command line.

The image runs one of two runtimes:

* the **shell runtime** (``zfsbootmenu_runtime=legacy``), written in Bash, with the ``fzf`` menu. It is the default.
* the **Rust runtime** (``zfsbootmenu_runtime=rust``), the program ``zbm`` with the new menu. It is opt-in and will
  replace the shell runtime after a beta. Some features exist only here: the locked menu, measured boot, YubiKey
  challenge-response over ``hidraw``, and keeping decrypted keys only in process memory.

Both runtimes unlock LUKS volumes with passphrases, key files, TPM2, FIDO2 (YubiKey, Nitrokey), GnuPG smart cards and
YubiKey challenge-response; see :doc:`luks`.

.. note::

  ZFSBootMenu does not replace the initramfs of your operating system. It is a separate image that finds and boots
  that initramfs.

Choose a way to build
---------------------

There are two ways to build an image from this repository:

A. **In the build container** (recommended). The container is Ubuntu 26.10 with its Linux 7.3 kernel and OpenZFS 2.4.4
   module, and every LUKS and hardware-key tool. It needs only ``podman`` or ``docker`` on the host, whatever
   distribution the host runs. It builds with dracut only.
B. **On the host**, with ``make install`` and ``generate-zbm``. The image then uses the host's kernel, ZFS modules and
   tools, and you can build with dracut or mkinitcpio. You install every dependency yourself.

Both ways need the source tree of this repository. The upstream ZFSBootMenu tree has none of this fork's LUKS or Rust
code.

Get the source
--------------

Clone this repository:

.. code-block:: bash

  git clone https://github.com/kiloclaw/zfsbootmenu-Luks-GPG-Yubikey-Nitrokey-TPM2 /usr/src/zbm

The rest of this page uses ``/usr/src/zbm``. To build another branch or tag, check it out in the clone first
(``git -C /usr/src/zbm checkout <branch>``).

The Rust programs build offline: ``.cargo/config.toml`` points Cargo at the crates vendored in ``vendor/``. Only the
compiler must be installed:

* ``rust-toolchain.toml`` selects Rust 1.97.0. With ``rustup``, Cargo installs that toolchain on first use, which needs
  the network once.
* The oldest supported compiler is ``rust-version = "1.88"`` in ``Cargo.toml``. A distribution's ``cargo`` and ``rustc``
  of that version or later also work; the build container uses Ubuntu's.

Path A: build in the container
------------------------------

:doc:`container-building` describes the container and its helper in detail, and :doc:`container-building/example`
shows how to manage images with it.

1. Install the container tool
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Install ``podman`` (preferred) or ``docker``. The ``Dockerfile`` uses ``COPY --chmod``, so it needs podman, buildah or
Docker 23 or later.

2. Build the container image
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The image is not published in a registry. Build it from the root of the checkout:

.. code-block:: bash

  cd /usr/src/zbm && podman build -t localhost/zbm-builder packaging/container

``docker build`` takes the same arguments. ``localhost/zbm-builder:latest`` is the default image of
``zbm-builder.sh``. Build arguments such as ``PACKAGES`` (extra Ubuntu packages) and ``KERNEL_FLAVOUR`` are described
in ``packaging/container/README.md``. Rebuild the image after ``packaging/container/`` changes.

3. Prepare the build directory
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The build directory holds your configuration and receives the images. It is mounted in the container as ``/build``.

.. code-block:: bash

  mkdir -p /root/zbm-build/dracut.conf.d

It may contain:

``config.yaml``
  The :doc:`generate-zbm configuration </man/generate-zbm.5>`. Without it, the container uses
  ``config/zbm-builder/config.yaml`` from the checkout, which builds both the EFI bundle and the components, without
  version numbers, with the command line ``zfsbootmenu ro quiet loglevel=4 nomodeset``. A ``config.yaml`` here
  **replaces** that file as a whole, so start from a copy:

  .. code-block:: bash

    cp /usr/src/zbm/config/zbm-builder/config.yaml /root/zbm-build/config.yaml

  The container always sets ``Global.ManageImages``, the two ``ImageDir`` keys and removes ``Global.BootMountPoint``,
  so images always land in the ``build`` subdirectory. ``Global.InitCPIO`` is refused, because the container has no
  mkinitcpio.

``dracut.conf.d/*.conf``
  dracut configuration and ZFSBootMenu build options (``zfsbootmenu_*`` variables, see
  :ref:`zfsbootmenu(7) <zbm-dracut-options>`). These files are read after the container's defaults, and a file with
  the same name as a default replaces it.

``hostid``
  The hostid to put in the image. ``zbm-builder.sh`` copies the host's ``/etc/hostid`` here when the file is missing,
  unless you pass ``-H``. ``-R`` removes an existing copy first.

``hooks/``
  :ref:`User hooks <user-hooks>`, in the subdirectories ``early-setup.d``, ``setup.d``, ``load-key.d``,
  ``boot-sel.d`` and ``teardown.d``. ``zbm-builder.sh`` then writes ``dracut.conf.d/user_hooks.conf``, which sets
  ``zfsbootmenu_hook_root=/build/hooks``. That file stays after the run.

``rc.d/``
  Executables that run inside the container before the build.

``crypttab``, ``keys/``
  LUKS configuration; see `LUKS and hardware keys`_.

Absolute paths in these files are paths inside the container, below ``/build``.

4. Run the build
~~~~~~~~~~~~~~~~

Always name the checkout with ``-l``:

.. code-block:: bash

  /usr/src/zbm/packaging/container/zbm-builder.sh -b /root/zbm-build -l /usr/src/zbm

.. warning::

  Without ``-l`` (or ``-L``), the container downloads the **upstream** ZFSBootMenu tree from GitHub, which has none of
  this fork's LUKS or Rust code. Several older pages show ``zbm-builder.sh`` without ``-l``; with this repository, add
  it.

``-l`` mounts the checkout read-only at ``/zbm``. ``-L`` mounts it with a temporary writable overlay instead (the
``O`` volume option of podman). The container then builds ``zbm``, ``brush`` and ``generate-zbm`` from the checkout
with Cargo, offline, and runs that ``generate-zbm``.

Other useful options of ``zbm-builder.sh`` (``zbm-builder.sh -h`` lists them all):

``-M z``
  Relabel the mounted volumes on SELinux hosts. With dracut, also pass ``-O --env=DRACUT_NO_XATTR=1``. See
  :doc:`container-building`.

``-O <arg>``
  Pass one argument to ``podman run``. For example, keep Cargo's build between runs:

  .. code-block:: bash

    zbm-builder.sh -l /usr/src/zbm -O --env=CARGO_TARGET_DIR=/build/cargo-target

``-c <file>``
  A Bash file to source for more settings; by default ``zbm-builder.conf`` in the build directory.

``-- <args>``
  Arguments for the container's entrypoint, ``build-init.sh``. Among them, ``-e <statement>`` changes the
  configuration with ``yq-go``, ``-p <package>`` installs an Ubuntu package first, and ``-k <X.Y>`` builds with
  another Ubuntu kernel series. Arguments after a second ``--`` go to ``generate-zbm``. For example, to build only
  the EFI bundle and show every command ``generate-zbm`` runs:

  .. code-block:: bash

    zbm-builder.sh -b /root/zbm-build -l /usr/src/zbm -- -e .Components.Enabled=false -- --debug

  ``-p`` and ``-k`` need the network inside the container.

For reproducible images, set ``SOURCE_DATE_EPOCH``; ``zbm-builder.sh`` passes it into the container:

.. code-block:: bash

  SOURCE_DATE_EPOCH="$(git -C /usr/src/zbm log -1 --format=%ct)" \
    /usr/src/zbm/packaging/container/zbm-builder.sh -b /root/zbm-build -l /usr/src/zbm

5. Find the images
~~~~~~~~~~~~~~~~~~

With the default configuration, the images are in ``/root/zbm-build/build``:

* ``vmlinuz.EFI``, the EFI bundle;
* ``vmlinuz-bootmenu`` and ``initramfs-bootmenu.img``, the components;
* ``debug/``, the debug information of ``zbm``, used to read its crash reports.

On the next build, the previous images are renamed with ``-backup`` (``vmlinuz-backup.EFI``,
``vmlinuz-bootmenu-backup``, ``initramfs-bootmenu-backup.img``). Continue with `Install and boot`_.

Path B: build on the host
-------------------------

.. warning::

  The "source" tabs of the distribution guides under *Installation Guides* download the upstream tree and run
  ``make core dracut``. That installs neither ``zbm`` nor ``generate-zbm``, and the build then fails. With this
  repository, follow the steps below. Their package lists also name Perl modules that only the old Perl
  ``generate-zbm`` used; the Rust ``generate-zbm`` needs no Perl.

1. Install the dependencies
~~~~~~~~~~~~~~~~~~~~~~~~~~~

Package names differ between distributions, so this list names programs. The build container's package list
(``packaging/container/setup-image.sh``, Ubuntu names) is a complete reference.

To build the programs:

* ``cargo`` and ``rustc`` (see `Get the source`_), ``make`` and ``git``;
* ``strip`` from binutils, which ``make install`` uses to strip the programs.

To build every image, on the build host:

* OpenZFS: ``zfs``, ``zpool``, ``zdb``, ``mount.zfs``, its udev rules, and its kernel modules for the kernel the image
  uses;
* dracut or mkinitcpio;
* ``fzf`` and ``kexec`` (kexec-tools). Both are required for either runtime;
* ``bash``, util-linux (``lsblk``, ``blkid``, ``setsid``, ``mount``), kmod (``insmod``, ``modinfo``, ``depmod``),
  procps (``ps``), ``tput`` (ncurses) and the usual core utilities;
* ``strings`` from binutils, with which ``generate-zbm`` reads kernel versions;
* optionally ``mbuffer``, ``column`` and ``less``. Images are built without them, with a warning.

For an EFI bundle, the systemd EFI stub, ``/usr/lib/systemd/boot/efi/linuxx64.efi.stub`` on x86_64
(``linuxaa64.efi.stub`` on aarch64), or another stub named in ``EFI.Stub``. Its package is ``systemd-boot-efi`` on
Debian and Ubuntu, ``systemd-boot-unsigned`` on AlmaLinux, ``systemd-boot-efistub`` on Void, and ``sys-apps/systemd``
with ``USE=boot`` on Gentoo.

For the Rust runtime, the keyboard layouts are compiled from the host's XKB data (``/usr/share/X11/xkb``,
xkeyboard-config) and compose rules (``/usr/share/X11/locale``, libX11). Without them the build only warns.

For LUKS and hardware keys, see `LUKS and hardware keys`_.

On Debian and Ubuntu, answer *No* when the ``kexec-tools`` package asks whether it should handle reboots.

2. Build and install
~~~~~~~~~~~~~~~~~~~~

.. code-block:: bash

  cd /usr/src/zbm
  make
  sudo make install

``make`` runs ``cargo build --release --locked`` for ``zbm``, ``brush``, ``generate-zbm`` and ``zbm-kcl``.
``make install`` puts:

* ``generate-zbm`` and ``zbm-kcl`` in ``/usr/bin``;
* ``zbm`` and ``brush`` in ``/usr/libexec/zfsbootmenu``, where every image build looks for ``zbm``;
* the shell runtime in ``/usr/share/zfsbootmenu``;
* the dracut modules ``90zfsbootmenu`` and ``90zfsbootmenu-measured`` in ``/usr/lib/dracut/modules.d``, and the
  mkinitcpio hooks in ``/usr/lib/initcpio``;
* ``config.yaml``, ``mkinitcpio.conf`` and ``dracut.conf.d/`` in ``/etc/zfsbootmenu``;
* the manual pages, and the ``contrib/`` hooks, ``examples/`` and ``splash.bmp`` in
  ``/usr/share/examples/zfsbootmenu``.

.. warning::

  ``make install`` overwrites ``/etc/zfsbootmenu/config.yaml``, ``/etc/zfsbootmenu/mkinitcpio.conf`` and the files
  it installs in ``/etc/zfsbootmenu/dracut.conf.d/``. Copy your changes away before you install again.

``make install`` also runs ``./scripts/version.sh -u``, which writes the version into
``legacy/zfsbootmenu/zbm-release`` and ``crates/zbm-platform/src/version.rs``. Cargo then builds the programs again,
as the user who runs ``make install``, so root needs the Rust toolchain on its ``PATH``. Alternatively, install into a
staging directory as your user:

.. code-block:: bash

  make install DESTDIR=/tmp/zbm-root

Other variables of the ``Makefile``: ``PREFIX`` (default ``/usr``), ``CONFDIR`` (default ``/etc/zfsbootmenu``), and
``INSTALL_STRIP=`` (empty) to keep the line tables in the installed programs. With another ``PREFIX``, set
``zfsbootmenu_zbm``, ``zfsbootmenu_module_root`` and ``zfsbootmenu_hook_root`` to match (see
:ref:`zfsbootmenu(7) <zbm-dracut-options>`), because their defaults are ``/usr`` paths.

3. Configure generate-zbm
~~~~~~~~~~~~~~~~~~~~~~~~~

``/etc/zfsbootmenu/config.yaml`` is described in :doc:`generate-zbm(5) </man/generate-zbm.5>`. The installed file
has ``ManageImages: false``, so ``generate-zbm`` does nothing until you change it. For an EFI bundle on an ESP mounted
at ``/boot/efi``, make sure these keys are set:

.. code-block:: yaml

  Global:
    ManageImages: true
    BootMountPoint: /boot/efi
  Components:
    Enabled: false
  EFI:
    ImageDir: /boot/efi/EFI/zbm
    Versions: false
    Enabled: true
  Kernel:
    CommandLine: ro quiet loglevel=0

``generate-zbm --enable`` sets ``Global.ManageImages: true`` in place and exits without building.

The keys that matter most:

``Global.BootMountPoint``
  ``generate-zbm`` mounts this directory if it is not mounted (it needs an ``/etc/fstab`` entry) and unmounts it at
  the end.

``EFI.Enabled``, ``Components.Enabled``
  Which images to build. Each enabled section needs its ``ImageDir``.

``EFI.Versions``, ``Components.Versions``
  ``false`` or ``0`` keeps one image and a ``-backup`` copy; a number *N* keeps versioned images. Write a number, not
  ``true``: the code reads ``true`` as no versions, although :doc:`generate-zbm(5) </man/generate-zbm.5>` says it
  acts as ``1``.

``Kernel.CommandLine``
  The command line of the EFI bundle, with ZFSBootMenu options such as ``zbm.timeout`` and ``zbm.luks`` (see
  :ref:`zfsbootmenu(7) <cli-parameters>`). It is not used for components.

``Kernel.Version``, ``Kernel.Path``
  Which kernel from ``/boot`` to use. By default, the highest version among ``/boot/vmlinuz*``. ``%{current}`` in
  ``Kernel.Version`` stands for ``uname -r``. The ZFS modules for that kernel must be installed.

``EFI.SplashImage``
  ``/etc/zfsbootmenu/splash.bmp``, but ``make install`` puts the logo elsewhere, and a missing file is skipped without
  a message. Copy it:

  .. code-block:: bash

    cp /usr/share/examples/zfsbootmenu/splash.bmp /etc/zfsbootmenu/splash.bmp

4. Choose dracut or mkinitcpio
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Without ``Global.InitCPIO``, ``generate-zbm`` uses dracut when ``dracut`` is on the ``PATH`` and mkinitcpio otherwise.
``Global.InitCPIO: true`` or ``--initcpio`` selects mkinitcpio. :doc:`mkinitcpio` describes its configuration.

Build options (``zfsbootmenu_*`` variables) go into:

* with dracut, a ``.conf`` file in ``/etc/zfsbootmenu/dracut.conf.d/`` (``Global.DracutConfDir``). This must not be
  the system's ``/etc/dracut.conf.d``.
* with mkinitcpio, ``/etc/zfsbootmenu/mkinitcpio.conf`` (``Global.InitCPIOConfig``).

They are listed in :ref:`zfsbootmenu(7) <zbm-dracut-options>`. They are not ``config.yaml`` keys.

The installed dracut configuration adds the ``zfsbootmenu`` and ``bash`` modules and leaves out ``crypt``, ``systemd``
and GPU drivers, among others. Do not add dracut's ``crypt`` module, or the ``encrypt`` or ``sd-encrypt`` mkinitcpio
hooks: ZFSBootMenu unlocks LUKS volumes itself.

5. Run generate-zbm
~~~~~~~~~~~~~~~~~~~

Look at what would happen first, then build:

.. code-block:: bash

  generate-zbm --dry-run --debug
  generate-zbm

``--dry-run`` prints one ``Would ...`` line for each change and makes none. ``--debug`` prints every command it runs.
:doc:`generate-zbm(8) </man/generate-zbm.8>` lists the other options.

With the configuration above, the EFI bundle is ``/boot/efi/EFI/zbm/vmlinuz.EFI`` and the previous one becomes
``vmlinuz-backup.EFI``. The name comes from the kernel's file name (``Kernel.Prefix`` overrides it). Components are
named ``vmlinuz-bootmenu`` and ``initramfs-bootmenu.img``, or carry the ZFSBootMenu version and a revision when
``Components.Versions`` is a number.

Executables in ``Global.PreHooksDir`` and ``Global.PostHooksDir`` run before and after each build.

LUKS and hardware keys
----------------------

:doc:`luks` describes every method; :doc:`/luks/administration` gives step-by-step procedures. This section lists
what each method needs at build time.

What goes into the image
~~~~~~~~~~~~~~~~~~~~~~~~

LUKS support is on by default (``zfsbootmenu_luks``), but the LUKS tools are added only when ``cryptsetup`` is
installed on the build host. Every other method is added only when its tools are installed there too:

==============================  ==========================================================================
Method                          Needed on the build host
==============================  ==========================================================================
Passphrase, key files           ``cryptsetup``, ``dmsetup``, ``udevadm``
TPM2 token                      ``tpm2-tools``; or systemd's ``libcryptsetup-token-systemd-tpm2.so``
                                plugin (needed for signed PCR policies and ``--tpm2-pcrlock``)
FIDO2 (YubiKey, Nitrokey 3)     ``fido2-token`` and ``fido2-assert`` (libfido2); or systemd's
                                ``libcryptsetup-token-systemd-fido2.so`` plugin
clevis token                    ``clevis``, ``jose``, ``luksmeta``, ``tpm2-tools``
GnuPG smart card                ``gpg`` with ``gpg-agent`` and ``scdaemon``; ``pcscd`` only for readers
                                GnuPG's own driver does not handle (``zfsbootmenu_luks_pcscd=yes``)
YubiKey challenge-response      ``ykchalresp`` and ``ykinfo`` (ykpers); the Rust runtime reaches the key
                                over ``hidraw`` without them
==============================  ==========================================================================

A method whose tools are missing is left out, mostly with an informational message. A dracut build without
``cryptsetup`` leaves LUKS out silently. The build container has all of these except ``pcscd``.

Images from a dracut ``hostonly`` build may lack the TPM, USB and HID drivers of the machine that boots them. The
container always builds with ``hostonly="no"``. Key sticks (``path:device`` keys) also need the USB storage drivers,
which the build does not add by itself:

.. code-block:: bash

  # /etc/zfsbootmenu/dracut.conf.d/usb.conf
  add_drivers+=" usb_storage uas "

The build-time crypttab
~~~~~~~~~~~~~~~~~~~~~~~

Without configuration, ZFSBootMenu offers every LUKS volume at boot. To choose volumes, keys and options, write a
crypttab (:doc:`zfsbootmenu-crypttab(5) </man/zfsbootmenu-crypttab.5>`, example in ``examples/crypttab``). The build
reads ``zfsbootmenu_luks_crypttab`` (default ``/etc/zfsbootmenu/crypttab``) and embeds it as
``/etc/zfsbootmenu/crypttab``, mode ``0600``::

  # <name>  <device>          <key file>                            <options>
  zroot-a   PARTLABEL=zfs0    none                                  discard
  zroot-b   PARTLABEL=zfs1    /etc/zfsbootmenu/keys/zroot.key.gpg   discard

In the container, copy it into the build directory and point at it:

.. code-block:: bash

  cp /etc/zfsbootmenu/crypttab /root/zbm-build/crypttab
  echo 'zfsbootmenu_luks_crypttab="/build/crypttab"' > /root/zbm-build/dracut.conf.d/luks.conf

The build handles the key field as follows:

* ``none`` or ``-``: nothing is embedded; ZFSBootMenu asks for the passphrase or uses a token.
* ``<path>:<device>``: nothing is embedded; the key is read from the device at boot.
* a GnuPG-encrypted file (``*.gpg``, ``*.asc``, ``*.pgp`` or the ``gpg`` option): embedded at the same path, mode
  ``0600``. The build refuses a file that is not encrypted, and warns about one encrypted with a passphrase only.
* any other file: **not embedded**; the build warns.
* ``keyscript=<name>``: the script ``/lib/cryptsetup/scripts/<name>`` (or an absolute path) is embedded. Programs it
  calls must be added by hand (dracut ``install_items``, mkinitcpio ``BINARIES`` or ``FILES``).

In the container, name key files and key scripts by their path in the container, such as
``/build/keys/zroot.key.gpg``, and keep them in the build directory. That path is also their path in the image.

Release and recovery builds (``zfsbootmenu_release_build=1``) never embed a crypttab or key files.

.. warning::

  Never put a plain LUKS key file into a ZFSBootMenu image. The image sits on the unencrypted EFI system partition.
  A key file named only in ``zbm.luks_key`` and added with ``install_items`` is not checked by the build: add only
  GnuPG-encrypted files that way.

Enrolling keys
~~~~~~~~~~~~~~

Enrol keys from the running system. Keep a passphrase in a key slot as a fallback.

TPM2 (see :doc:`luks`, *TPM2*, for the choice of PCRs):

.. code-block:: bash

  systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7 /dev/disk/by-partlabel/zfs0
  # or, to also require a PIN:
  systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7 --tpm2-with-pin=yes /dev/disk/by-partlabel/zfs0

FIDO2 security key (YubiKey 5, Nitrokey 3, or another key with ``hmac-secret``). Each volume needs its own touch at
boot:

.. code-block:: bash

  systemd-cryptenroll --fido2-device=auto /dev/disk/by-partlabel/zfs0

GnuPG key file on an OpenPGP card (YubiKey, Nitrokey):

.. code-block:: bash

  umask 077
  dd if=/dev/urandom of=/root/zroot.key bs=64 count=1
  cryptsetup luksAddKey /dev/disk/by-partlabel/zfs0 /root/zroot.key
  mkdir -p /etc/zfsbootmenu/keys
  gpg --encrypt --recipient 0xPRIMARY --recipient 0xBACKUP \
    --output /etc/zfsbootmenu/keys/zroot.key.gpg /root/zroot.key
  shred -u /root/zroot.key
  echo 'zroot-a PARTLABEL=zfs0 /etc/zfsbootmenu/keys/zroot.key.gpg discard' >> /etc/zfsbootmenu/crypttab
  echo 'zfsbootmenu_luks_gpg_keys="0xPRIMARY 0xBACKUP"' > /etc/zfsbootmenu/dracut.conf.d/luks-gpg.conf

``zfsbootmenu_luks_gpg_keys`` exports the public keys from the keyring of the user who runs the build. In the
container, that keyring is empty: export the public keys to a file in the build directory and name the file instead:

.. code-block:: bash

  echo 'zfsbootmenu_luks_gpg_pubkeys="/build/pubkey.asc"' >> /root/zbm-build/dracut.conf.d/luks.conf

Install ``gpg`` on the build host before you build. Without it, the build cannot read the key file and refuses to
embed it.

YubiKey challenge-response (compatible with ``yubikey-luks``):

.. code-block:: bash

  ykpersonalize -2 -ochal-resp -ochal-hmac -ohmac-lt64 -oserial-api-visible
  read -rs -p 'Passphrase: ' pass; echo
  printf '%s' "${pass}" | ykchalresp -2 -i- | tr -d '\n' > /root/yk.key; unset pass
  cryptsetup luksAddKey /dev/disk/by-partlabel/zfs0 /root/yk.key && shred -u /root/yk.key
  # crypttab: zroot-a  PARTLABEL=zfs0  none  discard,ykchalresp=2

Key file on a USB stick, and the other procedures: :doc:`/luks/administration`.

Rebuild the image after every change to the crypttab, the build options, the GnuPG public keys or the tools installed
on the build host.

.. warning::

  A TPM2 token without a PIN unlocks the pools for anyone who can switch the computer on, and ZFSBootMenu offers a
  recovery shell. Bind it to PCR 7 with Secure Boot, or require a PIN.

The boot environment's own initramfs must unlock the LUKS volumes again after ZFSBootMenu boots it. See
:doc:`luks`, *Booting the system afterwards*.

Optional features
-----------------

The Rust runtime and the new menu
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Set the build option in a dracut configuration file (or in ``/etc/zfsbootmenu/mkinitcpio.conf``):

.. code-block:: bash

  echo 'zfsbootmenu_runtime=rust' > /etc/zfsbootmenu/dracut.conf.d/runtime.conf
  # in the container:
  echo 'zfsbootmenu_runtime=rust' > /root/zbm-build/dracut.conf.d/runtime.conf

The image then needs ``zbm`` (``zfsbootmenu_zbm``), and takes ``brush`` from beside it; without ``brush`` the build
warns and the shell is Bash. Add ``init_on_free=1`` to ``Kernel.CommandLine`` so that the kernel zeroes the memory it
frees. ``zbm.menu=classic`` on the command line keeps the classic menu for this release. See
:ref:`zfsbootmenu(7) <zbm-dracut-options>` for the options of the Rust runtime, such as ``zfsbootmenu_console_font``
and ``zfsbootmenu_keyboard_layouts``.

Locked menu
~~~~~~~~~~~

With the Rust runtime, a PIN can protect everything but the default boot. ``zbm lock hash`` asks for the PIN twice
and prints a line ``zfsbootmenu_lock_hash='$argon2id$...'``. Put that line, with its single quotes, into a dracut
configuration file:

.. code-block:: bash

  /usr/libexec/zfsbootmenu/zbm lock hash

The lock holds only for a signed image under Secure Boot. An image with the shell runtime fails to build with this
option. See ``zfsbootmenu_lock_hash`` in :ref:`zfsbootmenu(7) <zbm-dracut-options>`.

Measured boot
~~~~~~~~~~~~~

Images with the Rust runtime measure the boot environment into PCRs 8 and 9 when the machine has a TPM. There is no
switch. The image needs ``tpm2_pcrread`` and ``tpm2_pcrextend``, which the build adds only with the LUKS token tools,
so ``zfsbootmenu_luks=no`` or ``zfsbootmenu_luks_tokens=no`` leaves them out. In the boot environment, the dracut
module ``zfsbootmenu-measured`` (installed by ``make install``) or the mkinitcpio hook ``zfsbootmenu-measured`` keeps
the event log. See :doc:`luks`, *Measured boot*.

Remote access over SSH
~~~~~~~~~~~~~~~~~~~~~~

:doc:`remote-access` describes dropbear for dracut and mkinitcpio. For the container, ``contrib/remote-ssh-build.sh``
wraps ``zbm-builder.sh``. Pass ``-l`` to it, so that it builds this repository:

.. code-block:: bash

  cd /usr/src/zbm
  BUILD_DIR=~/zbm-ssh ZBM_BUILDER=$PWD/packaging/container/zbm-builder.sh contrib/remote-ssh-build.sh -l "$PWD"

An SSH login is root in ZFSBootMenu. Restrict the keys, and consider the locked menu.

Recovery image
~~~~~~~~~~~~~~

The release and recovery configurations are in ``config/zfsbootmenu``. To build a recovery image in the container:

.. code-block:: bash

  mkdir -p ~/zbm-rec/dracut.conf.d
  cp /usr/src/zbm/config/zfsbootmenu/recovery.yaml ~/zbm-rec/config.yaml
  cp /usr/src/zbm/config/zfsbootmenu/recovery.conf.d/*.conf ~/zbm-rec/dracut.conf.d/
  /usr/src/zbm/packaging/container/zbm-builder.sh -b ~/zbm-rec -l /usr/src/zbm

Recovery images add disk, network and EFI tools. They use only automatic LUKS discovery and the command line, never a
crypttab.

Secure Boot signing
~~~~~~~~~~~~~~~~~~~

``generate-zbm`` does not sign images. ``contrib/zbm-sign.pl`` is a ``generate-zbm`` post-hook that signs them with
``sbctl`` or ``sbsigntools``. It is a Perl script and needs Perl with ``YAML::PP``. Install it into
``Global.PostHooksDir`` and add a ``SecureBoot`` section to ``/etc/zfsbootmenu/config.yaml``:

.. code-block:: bash

  install -m 0755 -D /usr/share/examples/zfsbootmenu/hooks/zbm-sign.pl /etc/zfsbootmenu/generate-zbm.post.d/zbm-sign.pl

.. code-block:: yaml

  SecureBoot:
    SignBackup: true
    DeleteUnsigned: false
    SignMethod: sbctl
    KeyDir: /etc/sbkeys

The script's comments describe the keys. The container has neither ``sbctl`` nor ``sbsigntools``; sign its images on
the host. Changing an image's command line with ``zbm-kcl`` removes the signature, so sign it again afterwards.

Install and boot
----------------

Copy the images to the ESP
~~~~~~~~~~~~~~~~~~~~~~~~~~

``generate-zbm`` on the host writes the images to ``ImageDir`` directly. Copy container images yourself, keeping the
previous one as the backup:

.. code-block:: bash

  mkdir -p /boot/efi/EFI/zbm
  cp /root/zbm-build/build/vmlinuz.EFI /boot/efi/EFI/zbm/vmlinuz.EFI

To let the container write to the ESP directly, mount it with ``RUNTIME_ARGS`` and ``BUILD_ARGS`` in
``zbm-builder.conf``, as :doc:`container-building/example` shows.

Firmware entries
~~~~~~~~~~~~~~~~

Create an entry for the bundle and one for its backup, where ``$BOOT_DISK`` and ``$BOOT_PART`` name the ESP (for
``/dev/sda1``: ``/dev/sda`` and ``1``):

.. code-block:: bash

  efibootmgr -c -d "$BOOT_DISK" -p "$BOOT_PART" \
    -L "ZFSBootMenu (Backup)" \
    -l '\EFI\ZBM\VMLINUZ-BACKUP.EFI'

  efibootmgr -c -d "$BOOT_DISK" -p "$BOOT_PART" \
    -L "ZFSBootMenu" \
    -l '\EFI\ZBM\VMLINUZ.EFI'

For components, rEFInd and a direct ``efibootmgr`` entry with ``initrd=``, see :doc:`uefi-booting`. Legacy BIOS
systems can boot the components with syslinux; ``examples/syslinux.cfg`` and ``contrib/syslinux-update.sh`` help.

Point ZFSBootMenu at your boot environment
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

ZFSBootMenu boots the pool's ``bootfs`` after a countdown, with the command line from the
``org.zfsbootmenu:commandline`` property:

.. code-block:: bash

  zpool set bootfs=zroot/ROOT/${ID} zroot
  zfs set org.zfsbootmenu:commandline="quiet" zroot/ROOT

Do not set ``root=`` in that property. :doc:`bootenvs-and-you` explains what ZFSBootMenu treats as a boot environment.

Check the result
----------------

* Read the command line of an EFI bundle, and change it without a rebuild (:doc:`zbm-kcl(8) </man/zbm-kcl.8>`):

  .. code-block:: bash

    zbm-kcl /boot/efi/EFI/zbm/vmlinuz.EFI
    zbm-kcl -r zbm.timeout -a zbm.timeout=15 /boot/efi/EFI/zbm/vmlinuz.EFI

* List what a dracut image contains (``lsinitcpio`` for mkinitcpio images), and confirm that no plain key file is in
  it:

  .. code-block:: bash

    lsinitrd /boot/efi/EFI/zbm/initramfs-bootmenu.img | grep -E 'crypttab|zluks|cryptsetup|gpg|tpm2|fido2|clevis'
    lsinitrd -f etc/zfsbootmenu/crypttab /boot/efi/EFI/zbm/initramfs-bootmenu.img

  :doc:`/luks/administration` shows how to extract the initramfs of an EFI bundle first.

* On the first boot:

  * the menu shows, or the countdown to ``bootfs`` runs and can be interrupted;
  * LUKS volumes unlock with the methods you enrolled, and the passphrase still works;
  * the pools import, and ``zfs version`` in the recovery shell shows the ZFS version you expect;
  * the boot environment starts, and its own initramfs unlocks its volumes;
  * the backup firmware entry boots too.

Troubleshooting
---------------

``zbm, which plans every ZFSBootMenu image, is not at /usr/libexec/zfsbootmenu/zbm``
  ``zbm`` is not installed. ``make core dracut`` does not install it; run ``make install``, or set ``zfsbootmenu_zbm``.

``failed to install essential executable '<name>'``
  A required program is missing on the build host, often ``fzf`` or ``kexec``. Install it.

``Unable to locate libgcc_s.so``
  Install the GCC runtime library, or set ``zfsbootmenu_skip_gcc_s=yes``.

``Unable to find UEFI stub loader at default locations``
  Install the systemd EFI stub, or set ``EFI.Stub``.

``ManageImages not enabled, no action taken``
  Set ``Global.ManageImages: true``, or run ``generate-zbm --enable``.

``SPL (...) and system (...) hostids do not match!``
  The hostid of the loaded ZFS module and ``/etc/hostid`` disagree. The image takes ``/etc/hostid`` (in the
  container, the build directory's ``hostid``). The ``spl_hostid`` option in :doc:`zfsbootmenu(7) </man/zfsbootmenu.7>`
  sets the hostid at boot.

``zfsbootmenu_runtime=...: unknown runtime; use zfsbootmenu_runtime=legacy or rust``
  Fix the value of ``zfsbootmenu_runtime``.

``the build container has no mkinitcpio``
  Remove ``Global.InitCPIO`` from the container's ``config.yaml``.

``... uses key file '...', which is NOT embedded in the image``
  A plain key file is named in the crypttab. Use a GnuPG-encrypted file, a ``path:device`` key or a token.

``... is not GnuPG-encrypted; refusing to embed it in the image``
  The file is not encrypted, or ``gpg`` is missing on the build host.

The image has none of this fork's LUKS features
  The container built the upstream tree. Pass ``-l /usr/src/zbm`` to ``zbm-builder.sh``.

A security key, card or key stick is not found at boot
  The image lacks the USB or HID drivers. Build with ``hostonly="no"``, and add ``usb_storage`` and ``uas`` for key
  sticks. :doc:`luks`, *Troubleshooting*, lists more checks.

``loglevel=7`` on the ZFSBootMenu command line and ``zlogtail`` in the recovery shell show what happened during
start-up.

..
  vim: softtabstop=2 shiftwidth=2 textwidth=120
