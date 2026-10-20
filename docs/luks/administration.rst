Administration
==============

.. contents:: Contents
  :depth: 2
  :local:
  :backlinks: none

This page covers the tasks of running pools on LUKS with ZFSBootMenu: managing keys and unlock methods, replacing and
adding disks, migrating existing pools, rebuilding the ZFSBootMenu image, and recovering when something goes wrong.

The examples follow the conventions of :doc:`vdev-configurations`: commands run as root (``#``), volumes are named
``luks-<UUID>``, and the helper ``luks_create`` and the array ``POOL_OPTS`` are defined as shown there.

Most key-management tasks apply to every LUKS volume of a pool. The following prints the LUKS devices behind the
pool's vdevs, for use in loops:

.. code-block:: bash

  pool_luks_devices() {
    local vdev
    for vdev in $( zpool status -LP "$1" | awk '$1 ~ "^/dev/" { print $1 }' ); do
      [ -e "/sys/class/block/${vdev##*/}/dm/uuid" ] || continue
      grep -q '^CRYPT-' "/sys/class/block/${vdev##*/}/dm/uuid" || continue
      echo "/dev/$( ls "/sys/class/block/${vdev##*/}/slaves" )"
    done
  }

.. code-block:: bash

  # pool_luks_devices zroot
  /dev/sda2
  /dev/sdb2

Key Management
--------------

Viewing key slots and tokens
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``cryptsetup luksDump`` shows the key slots and LUKS2 tokens of a volume. ``systemd-cryptenroll`` lists them in a
table:

.. code-block:: bash

  # cryptsetup luksDump /dev/sda2
  # systemd-cryptenroll /dev/sda2
  SLOT TYPE
     0 password
     1 tpm2

Inside ZFSBootMenu, ``zluks list`` shows the token types of every volume.

In scripts, read tokens from the JSON metadata instead of the text output. With systemd's token plugins installed,
cryptsetup 2.8 and later print the sections of all tokens after the first to standard error, so a pipe sees only the
first one:

.. code-block:: bash

  # cryptsetup luksDump --dump-json-metadata /dev/sda2 | grep '"type"'

Backing up LUKS headers
~~~~~~~~~~~~~~~~~~~~~~~

A damaged LUKS header makes a volume unreadable for good. Back up the header of every volume and keep the copies
offline, for example on an encrypted USB stick:

.. code-block:: bash

  # for dev in $( pool_luks_devices zroot ); do
      cryptsetup luksHeaderBackup "${dev}" \
        --header-backup-file "/mnt/usb/$( cryptsetup luksUUID "${dev}" ).header"
    done

To restore a header:

.. code-block:: bash

  # cryptsetup luksHeaderRestore /dev/sda2 --header-backup-file /mnt/usb/5d0c3a1e-....header

A header backup contains the key slots as they were when it was made. A passphrase removed later still opens a volume
restored from an older backup, so make new backups after removing keys.

Changing the passphrase
~~~~~~~~~~~~~~~~~~~~~~~

Use the same new passphrase on every volume, so that ZFSBootMenu keeps asking only once:

.. code-block:: bash

  # for dev in $( pool_luks_devices zroot ); do
      echo "${dev}"; cryptsetup luksChangeKey "${dev}"
    done

Adding a recovery passphrase
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A second passphrase, stored in a safe place, is a fallback for lost hardware keys:

.. code-block:: bash

  # for dev in $( pool_luks_devices zroot ); do
      cryptsetup luksAddKey "${dev}"
    done

Enrolling a TPM
~~~~~~~~~~~~~~~

Enroll the TPM on every volume and bind it to PCR 7, the Secure Boot state. Add ``--tpm2-with-pin=yes`` to require a PIN
as well; use the same PIN everywhere so that ZFSBootMenu asks once:

.. code-block:: bash

  # for dev in $( pool_luks_devices zroot ); do
      systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7 "${dev}"
    done
  # generate-zbm

The volumes then unlock at boot without a prompt. ZFSBootMenu uses systemd's plugin if the image contains it, and
otherwise its own implementation; see :doc:`/general/luks` for the PCR choices and their trade-offs.

.. warning::

  Without a PIN, anyone who can start the computer gets the pools unlocked, and ZFSBootMenu offers a recovery shell.
  Use a PIN, or rely on Secure Boot and PCR 7.

Re-enrolling the TPM after firmware or Secure Boot changes
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

When the bound PCRs change, the TPM no longer releases the key and ZFSBootMenu falls back to the passphrase. After
booting with the passphrase, replace the TPM enrollment:

.. code-block:: bash

  # for dev in $( pool_luks_devices zroot ); do
      systemd-cryptenroll --wipe-slot=tpm2 --tpm2-device=auto --tpm2-pcrs=7 "${dev}"
    done

Binding with clevis instead
~~~~~~~~~~~~~~~~~~~~~~~~~~~

clevis TPM2 policies work in every image, including those built without systemd's token plugins:

.. code-block:: bash

  # for dev in $( pool_luks_devices zroot ); do
      clevis luks bind -d "${dev}" tpm2 '{"pcr_ids":"7"}'
    done
  # clevis luks list -d /dev/sda2
  1: tpm2 '{"hash":"sha256","key":"ecc","pcr_ids":"7"}'

``clevis luks unbind -d <device> -s <slot>`` removes a binding.

Enrolling FIDO2 security keys
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Enroll a primary and a backup key (for example a YubiKey 5 or Nitrokey 3) on every volume. Plug in one key, run the
loop, then repeat with the other:

.. code-block:: bash

  # for dev in $( pool_luks_devices zroot ); do
      systemd-cryptenroll --fido2-device=auto "${dev}"
    done

By default, unlocking requires the key's PIN and a touch. Each enrollment creates a separate credential, so ZFSBootMenu
asks for a touch once per volume.

Using an OpenPGP smart card (YubiKey, Nitrokey)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A random key file is added to every volume, then encrypted to the OpenPGP keys on one or more cards. The encrypted file
is embedded in the ZFSBootMenu image.

1. Create the key file and add it to every volume:

   .. code-block:: bash

     # umask 077
     # dd if=/dev/urandom of=/root/zroot.key bs=64 count=1
     # for dev in $( pool_luks_devices zroot ); do
         cryptsetup luksAddKey "${dev}" /root/zroot.key
       done

2. Encrypt it to the primary and the backup card, then destroy the plain copy:

   .. code-block:: bash

     # mkdir -p /etc/zfsbootmenu/keys
     # gpg --encrypt --recipient 0xPRIMARYKEYID --recipient 0xBACKUPKEYID \
           --output /etc/zfsbootmenu/keys/zroot.key.gpg /root/zroot.key
     # shred -u /root/zroot.key

3. List the encrypted key file for every volume in the ZFSBootMenu crypttab:

   .. code-block:: bash

     # for dev in $( pool_luks_devices zroot ); do
         uuid="$( cryptsetup luksUUID "${dev}" )"
         echo "luks-${uuid} UUID=${uuid} /etc/zfsbootmenu/keys/zroot.key.gpg discard"
       done >> /etc/zfsbootmenu/crypttab

4. Include the public keys in the image, and rebuild it:

   .. code-block:: bash

     # echo 'zfsbootmenu_luks_gpg_keys="0xPRIMARYKEYID 0xBACKUPKEYID"' \
         > /etc/zfsbootmenu/dracut.conf.d/luks-gpg.conf
     # generate-zbm

At boot, ZFSBootMenu asks for the card, checks that it holds one of the keys, asks for the PIN once, and unlocks all
volumes with the decrypted key file.

Using YubiKey challenge-response
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

1. Program slot 2 of the YubiKey for HMAC-SHA1 challenge-response (this replaces anything stored in that slot):

   .. code-block:: bash

     # ykpersonalize -2 -ochal-resp -ochal-hmac -ohmac-lt64 -oserial-api-visible

2. Derive the LUKS passphrase from your chosen passphrase and add it to every volume:

   .. code-block:: bash

     # read -rs -p 'Passphrase: ' pass; echo
     # printf '%s' "${pass}" | ykchalresp -2 -i- | tr -d '\n' > /root/yk.key; unset pass
     # for dev in $( pool_luks_devices zroot ); do
         cryptsetup luksAddKey "${dev}" /root/yk.key
       done
     # shred -u /root/yk.key

3. Add ``ykchalresp=2`` to every volume's options in the ZFSBootMenu crypttab and rebuild the image.

Volumes set up with ``yubikey-luks`` work as they are; add ``ykchalresp-hash`` or ``ykchalresp-concat`` if it was
configured with ``HASH=1`` or ``CONCATENATE=1``.

Removing an unlock method
~~~~~~~~~~~~~~~~~~~~~~~~~

.. code-block:: bash

  # systemd-cryptenroll --wipe-slot=tpm2 /dev/sda2       # TPM2 enrollments
  # systemd-cryptenroll --wipe-slot=fido2 /dev/sda2      # FIDO2 enrollments
  # clevis luks unbind -d /dev/sda2 -s 1                 # a clevis binding
  # cryptsetup luksKillSlot /dev/sda2 3                  # any key slot by number

Never remove the last key slot that you can still use. Make a new header backup afterwards.

Using a key file on a USB stick
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A key file on a removable stick unlocks the pool without typing, while the passphrase keeps working when the stick is
absent. Anyone holding both the stick and the computer can unlock the pool, so store them apart.

1. Format the stick with a label (this erases it), and create the key file on it:

   .. code-block:: bash

     # mkfs.vfat -F 32 -n ZROOTKEY /dev/sdX1
     # mount /dev/disk/by-label/ZROOTKEY /mnt
     # dd if=/dev/urandom of=/mnt/zroot.key bs=64 count=1

2. Add it to every volume of the pool:

   .. code-block:: bash

     # for p in $(pool_luks_devices zroot); do
         cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 "$p" /mnt/zroot.key
       done
     # umount /mnt

3. Point ZFSBootMenu at it, either per volume in the crypttab (``/zroot.key:LABEL=ZROOTKEY`` in the key field) or
   for all volumes on the kernel command line, and rebuild the image:

   .. code-block:: bash

     # zbm-kcl -a 'zbm.luks_key=/zroot.key:LABEL=ZROOTKEY' /boot/efi/EFI/zbm/vmlinuz.EFI

ZFSBootMenu waits up to ``zbm.luks_token_wait`` seconds (default 10) for a stick that the kernel has not detected yet,
once per stick and boot. Raise the value for slow hubs or card readers. To retire a stick, remove its slot with
``cryptsetup luksRemoveKey <device> /mnt/zroot.key`` on every volume while the stick is mounted.

Unlocking without a crypttab
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

An existing system can drop both crypttabs:

1. Move the volume list into ZFSBootMenu's kernel command line, then remove its crypttab:

   .. code-block:: bash

     # for p in $(pool_luks_devices zroot); do echo "UUID=$(cryptsetup luksUUID "$p")"; done
     # $EDITOR /etc/zfsbootmenu/config.yaml     # Kernel.CommandLine: "... zbm.luks=UUID=<a>,UUID=<b>"
     # mv /etc/zfsbootmenu/crypttab /etc/zfsbootmenu/crypttab.unused
     # generate-zbm

   GnuPG-encrypted key files that the crypttab used to embed must now be listed with ``install_items`` in a file in
   ``/etc/zfsbootmenu/dracut.conf.d/``, and named with ``zbm.luks_key=``.

2. Give the boot environment's systemd initramfs ``luks.uuid=`` and ``luks.key=`` for each volume, as in Example 12
   of :doc:`vdev-configurations`, set ``hostonly_cmdline="no"`` in its dracut configuration, rename
   ``/etc/crypttab`` and rebuild the initramfs.

3. Before rebooting, confirm that the new initramfs holds the key file and no crypttab:

   .. code-block:: bash

     # lsinitrd /boot/initramfs-$(uname -r).img | grep -E 'cryptsetup-keys.d|systemd-cryptsetup$'
     # lsinitrd -f etc/crypttab /boot/initramfs-$(uname -r).img     # prints nothing

Keep a live medium at hand for the first reboot; see `Recovery`_.

NVMe SSDs
---------

Sector size
  Many NVMe SSDs can switch from 512-byte to 4096-byte logical blocks. ``nvme id-ns -H /dev/nvme0n1`` lists the
  formats; ``nvme format /dev/nvme0n1 --lbaf=<n> --force`` switches, **erasing the SSD**. Format LUKS volumes with
  ``--sector-size 4096`` and create pools with ``ashift=12`` in either case. With 512-byte logical blocks, the LUKS
  partition's size must then be a multiple of 4 KiB (see Example 12 in :doc:`vdev-configurations`).

Flags in the LUKS header
  ``cryptsetup open --allow-discards --perf-no_read_workqueue --perf-no_write_workqueue --persistent`` stores TRIM and
  the work-queue flags in the LUKS2 header, where ZFSBootMenu, systemd and cryptsetup apply them on every activation.
  ``cryptsetup luksDump <device> | grep Flags`` shows them; ``cryptsetup refresh`` with the same options changes them
  on an open volume.

TRIM
  With ``autotrim=on`` and discards allowed, freed space is trimmed continuously. A periodic full TRIM is still
  useful: ``zpool trim zroot``, or the ``zfs-trim-monthly@zroot.timer`` unit shipped with OpenZFS. Watch progress
  with ``zpool status -t``.

Health
  ``nvme smart-log /dev/nvme0`` reports ``critical_warning``, ``percentage_used`` (wear), ``media_errors`` and
  ``available_spare``. Replace an SSD whose spare capacity runs low as described in `Replacing a failed disk`_.

Drivers
  ZFSBootMenu images built without host-only mode include the NVMe driver. For a boot environment initramfs built
  with dracut in host-only mode, add ``add_drivers+=" nvme "``: host-only detection does not always see through ZFS
  and LUKS to the SSD.

Pool Maintenance
----------------

Booting with a failed disk
~~~~~~~~~~~~~~~~~~~~~~~~~~

When a disk has failed or is missing, ZFSBootMenu unlocks the volumes that are present, imports the pool
``DEGRADED`` and boots it, as long as the pool's redundancy covers the missing disk. The booted system's initramfs must
then also do without the missing volume:

* dracut waits for every volume named with ``rd.luks.uuid=`` on the command line, and after its timeout drops to its
  emergency shell. For one boot, press *MOD+E* in ZFSBootMenu and remove the missing volume's ``rd.luks.uuid=``
  (and its ``rd.luks.key=``). The change does not persist.
* With a crypttab in the initramfs, systemd waits for every device listed without the ``nofail`` option, and the boot
  fails if one does not appear. Mark the volumes of redundant pools ``nofail`` so that a missing disk does not stop the
  boot; see **crypttab**\ (5).

Then replace the disk as described below.

Replacing a failed disk
~~~~~~~~~~~~~~~~~~~~~~~

1. Identify the failed vdev:

   .. code-block:: bash

     # zpool status -P zroot
       ...
       mirror-0                     DEGRADED
         /dev/mapper/luks-5d0c...   ONLINE
         12345678901234567890       UNAVAIL   was /dev/mapper/luks-6e1d...

2. Partition the new disk as the old one was (see :doc:`vdev-configurations`), then create its LUKS volume with the
   same passphrase as the others:

   .. code-block:: bash

     # VDEVS=()
     # luks_create /dev/disk/by-id/ata-NEWDISK-part2

3. Replace the failed vdev and wait for the resilver to finish:

   .. code-block:: bash

     # zpool replace zroot 12345678901234567890 "${VDEVS[0]}"
     # zpool wait -t resilver zroot

4. Enroll the same unlock methods (TPM, FIDO2, key files) on the new volume as on the others, and back up its header.

5. In */etc/crypttab* and */etc/zfsbootmenu/crypttab*, replace the line of the old volume with one for the new volume.
   Then rebuild the system's initramfs and the ZFSBootMenu image (``generate-zbm``).

6. For a boot disk, recreate its EFI system partition and copy the ZFSBootMenu executable to it (see
   :doc:`/general/mdraid-esp`).

Turning a single disk into a mirror
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Attach a new LUKS volume to the existing one:

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/ata-DISK2-part2
  # zpool attach zroot /dev/mapper/luks-5d0c3a1e-... "${VDEVS[0]}"
  # zpool wait -t resilver zroot

The same command adds a third disk to a two-way mirror.

Adding a vdev
~~~~~~~~~~~~~

Add a new data vdev with the same layout as the existing ones:

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/ata-DISK{5,6}-part2
  # zpool add zroot mirror "${VDEVS[@]}"

Expanding a RAIDZ vdev
~~~~~~~~~~~~~~~~~~~~~~

OpenZFS 2.3 and later can add a disk to an existing RAIDZ vdev. The pool's ZFS version must support the
``raidz_expansion`` feature, and so must the ZFS version in the ZFSBootMenu image (images built in the build container
have OpenZFS 2.4.4). Once the expansion starts, ZFS versions older than 2.3 can no longer import the pool; see
:doc:`zfs-versions`:

.. code-block:: bash

  # VDEVS=()
  # luks_create /dev/disk/by-id/ata-DISK7-part2
  # zpool attach tank raidz2-0 "${VDEVS[0]}"
  # zpool wait -t raidz_expand tank

Upgrading pool features
~~~~~~~~~~~~~~~~~~~~~~~

Enable new pool features only after updating ZFSBootMenu and every other system that imports the pool; see
:doc:`zfs-versions` for the order and the commands.

Removing an auxiliary vdev
~~~~~~~~~~~~~~~~~~~~~~~~~~

Log, cache and spare vdevs can be removed at any time. Close the LUKS volume afterwards and delete its crypttab lines:

.. code-block:: bash

  # zpool remove tank /dev/mapper/luks-9a72f853-...
  # cryptsetup close luks-9a72f853-...

Migrating an Unencrypted Pool to LUKS
-------------------------------------

.. warning::

  ``cryptsetup luksFormat`` does not overwrite existing data. Unencrypted data stays readable on a disk until it is
  overwritten. Before formatting a disk that held unencrypted pool data, wipe it: use ``blkdiscard`` on SSDs, or
  overwrite the whole device on hard disks. Back up the pool before migrating.

Mirrors
~~~~~~~

Convert one side of the mirror at a time, so the pool stays redundant except during each resilver:

1. Detach the second disk, then wipe and repartition it:

   .. code-block:: bash

     # zpool detach zroot /dev/disk/by-id/ata-DISK2-part2

2. Create its LUKS volume and attach it to the remaining disk:

   .. code-block:: bash

     # VDEVS=()
     # luks_create /dev/disk/by-id/ata-DISK2-part2
     # zpool attach zroot /dev/disk/by-id/ata-DISK1-part2 "${VDEVS[0]}"
     # zpool wait -t resilver zroot

3. Repeat for the first disk, attaching its new LUKS volume to the volume created in step 2.

4. Configure the system and ZFSBootMenu as described in :ref:`luks-making-pool-bootable`.

RAIDZ and dRAID
~~~~~~~~~~~~~~~

A RAIDZ or dRAID vdev cannot be detached disk by disk. Create a new pool on LUKS volumes, then copy the data with
``zfs send`` and ``zfs receive``:

.. code-block:: bash

  # zfs snapshot -r tank@migrate
  # zfs send -R tank@migrate | zfs receive -F newtank
  # zpool export tank

When there are no spare disks for a new pool, the disks can instead be converted one at a time with
``zpool replace``. The vdev then runs with reduced redundancy during each resilver.

The ZFSBootMenu Image
---------------------

Rebuilding after changes
~~~~~~~~~~~~~~~~~~~~~~~~

Rebuild the image after changing */etc/zfsbootmenu/crypttab*, the build options, the GnuPG public keys, or the tools
installed on the build host:

.. code-block:: bash

  # generate-zbm

Checking the image contents
~~~~~~~~~~~~~~~~~~~~~~~~~~~

To confirm that the crypttab and the unlock tools are in a dracut-built image:

.. code-block:: bash

  # lsinitrd /boot/efi/EFI/zbm/initramfs-bootmenu.img | grep -E 'crypttab|zluks|cryptsetup|gpg|tpm2|fido2|clevis'
  # lsinitrd -f etc/zfsbootmenu/crypttab /boot/efi/EFI/zbm/initramfs-bootmenu.img

Use ``lsinitcpio`` for images built with mkinitcpio.

When only the unified EFI executable is built (``Components.Enabled: false``), extract its initramfs first, and read
its embedded command line with ``zbm-kcl``:

.. code-block:: bash

  # objcopy -O binary --only-section=.initrd /boot/efi/EFI/zbm/vmlinuz.EFI /run/zbm-initrd.img
  # lsinitrd /run/zbm-initrd.img | grep -E 'crypttab|cryptsetup|gpg|nvme'
  # zbm-kcl /boot/efi/EFI/zbm/vmlinuz.EFI
  # rm /run/zbm-initrd.img

No plain key file may ever appear in this listing.

The ZFS version in the image determines which pools it can import. Run ``zfs version`` in a recovery shell of the
running image; see :doc:`zfs-versions`.

Building in the container
~~~~~~~~~~~~~~~~~~~~~~~~~

The upstream ZFSBootMenu source tree does not contain these features. Build the container from this repository, and
point ``zbm-builder.sh`` at the source tree; ``localhost/zbm-builder`` is its default image:

.. code-block:: bash

  # git clone https://github.com/kiloclaw/zfsbootmenu-Luks-GPG-Yubikey-Nitrokey-TPM2 /usr/src/zbm
  # cd /usr/src/zbm && podman build -t localhost/zbm-builder releng/docker
  # mkdir -p /root/zbm-build/dracut.conf.d && cd /root/zbm-build
  # cp /etc/zfsbootmenu/crypttab ./crypttab
  # echo 'zfsbootmenu_luks_crypttab="/build/crypttab"' > dracut.conf.d/luks.conf
  # /usr/src/zbm/zbm-builder.sh -l /usr/src/zbm

The build directory is mounted in the container as */build*, and the images appear in its *build* subdirectory.
Inside the container:

* GnuPG public keys must be given as files: copy them into the build directory and set
  ``zfsbootmenu_luks_gpg_pubkeys="/build/pubkey.asc"``. ``zfsbootmenu_luks_gpg_keys`` would read the container's empty
  keyring.
* GnuPG-encrypted key files must be in the build directory and named in the crypttab by their path in the container,
  such as */build/keys/zroot.key.gpg*. The image then contains them at that path.

Recovery
--------

Unlocking from the recovery shell
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

When volumes were skipped or could not be unlocked, ZFSBootMenu keeps retrying the pool import. Press *Escape* for a
recovery shell, then:

.. code-block:: bash

  # zluks list
  # zluks
  # exit

``zluks`` repeats the start-up unlock; ``zluks open`` unlocks a single volume. After ``exit``, ZFSBootMenu imports the
pools.

Unlocking remotely
~~~~~~~~~~~~~~~~~~

With remote access configured (see :doc:`/general/remote-access`), log in over SSH while the console waits, and run
``zluks``. The console prompt notices the unlocked volume and continues.

TPM refuses the PIN
~~~~~~~~~~~~~~~~~~~

After too many wrong PINs, the TPM enters dictionary-attack lockout, and ZFSBootMenu falls back to other methods. Boot
with the passphrase, then clear the lockout (this needs the TPM's lockout authorization, usually empty) or wait for it
to expire:

.. code-block:: bash

  # tpm2_dictionarylockout --clear-lockout

The boot environment's ZFS key is rejected
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

If the ZFS native-encryption key is wrong, ZFSBootMenu drops to the recovery shell. The LUKS volumes stay unlocked and
the pools stay imported. Load the key by hand, then return to the menu:

.. code-block:: bash

  # zfs load-key -L prompt zroot/ROOT/os
  # exit

A cache or spare vdev shows UNAVAIL
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The volume is open under a different name than the one recorded in the pool. Name it as on the running system, in the
ZFSBootMenu crypttab or with ``zluks open <device> <name>``; see :doc:`vdev-configurations`.

The USB key stick is ignored
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

ZFSBootMenu waited ``zbm.luks_token_wait`` seconds and then asked for the passphrase. In the recovery shell, check that
the stick is visible with ``ls /dev/disk/by-label/`` and readable with ``mount -o ro /dev/disk/by-label/<label> /mnt``.
Raise ``zbm.luks_token_wait`` for slow hubs. If the stick never appears, add ``add_drivers+=" usb_storage uas "`` to a
file in ``/etc/zfsbootmenu/dracut.conf.d/`` and rebuild the image.

The boot environment asks for the passphrase again
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Its initramfs could not use its own key file. After booting, check ``cat /proc/cmdline`` for the ``luks.*`` (or
``rd.luks.*``) parameters and the initramfs for the key file:

.. code-block:: bash

  # lsinitrd /boot/initramfs-$(uname -r).img | grep -E 'cryptsetup-keys.d|crypttab|systemd-cryptsetup$'

A missing key file means the dracut ``install_items`` line is wrong; a missing ``systemd-cryptsetup`` means the dracut
*crypt* module was not included.

See Also
--------

* :doc:`zluks(8) </man/zluks.8>`, :doc:`zfsbootmenu-crypttab(5) </man/zfsbootmenu-crypttab.5>`,
  :doc:`zfsbootmenu-luks(7) </man/zfsbootmenu-luks.7>`
* **cryptsetup**\ (8), **systemd-cryptenroll**\ (1), **clevis-luks-bind**\ (1), **gpg**\ (1), **ykpersonalize**\ (1),
  **zpool-replace**\ (8), **zpool-attach**\ (8), **zpool-wait**\ (8)
