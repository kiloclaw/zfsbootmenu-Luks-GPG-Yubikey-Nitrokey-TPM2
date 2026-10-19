Commands, Tools and Options
===========================

.. contents:: Contents
  :depth: 2
  :local:
  :backlinks: none

This page lists everything involved in running pools on LUKS with ZFSBootMenu. The authoritative reference is in the
manual pages :doc:`zfsbootmenu-luks(7) </man/zfsbootmenu-luks.7>`,
:doc:`zfsbootmenu-crypttab(5) </man/zfsbootmenu-crypttab.5>` and :doc:`zluks(8) </man/zluks.8>`.

Commands in the ZFSBootMenu Image
---------------------------------

These commands are available in the recovery shell and over SSH. Optional tools are present when they were installed
on the build host and not switched off with a build option.

========================================  ============================================================================
Command                                   Purpose
========================================  ============================================================================
``zluks``                                 list, unlock and close LUKS volumes; see :doc:`zluks(8) </man/zluks.8>`
``cryptsetup``                            open, inspect and close LUKS volumes directly
``dmsetup``                               inspect device-mapper devices
``zpool``, ``zfs``                        import pools, load ZFS keys, inspect vdevs
``gpg``, ``gpgconf``                      decrypt GnuPG key files; ``gpg --card-status`` checks an OpenPGP card
``tpm2_*`` (tpm2-tools)                   native TPM2 unlocking, ``tpm2_getcap`` and other TPM inspection
``clevis``, ``clevis-luks-*``             clevis tokens, e.g. ``clevis luks list -d <device>``
``fido2-token``, ``fido2-assert``         native FIDO2 unlocking; ``fido2-token -L`` lists security keys
``ykchalresp``, ``ykinfo``                YubiKey challenge-response
``pcscd``                                 PC/SC daemon, with ``zfsbootmenu_luks_pcscd=yes``
========================================  ============================================================================

Tools on the Running System
---------------------------

These tools are used on the installed system to prepare volumes and build images:

========================================  ============================================================================
Tool                                      Used to
========================================  ============================================================================
``cryptsetup``                            format volumes, add and remove key slots, back up headers
``systemd-cryptenroll``                   enroll TPM2 chips and FIDO2 security keys as LUKS2 tokens
``clevis luks bind``                      bind volumes to TPM2 (or tang) policies
``gpg``                                   encrypt key files to OpenPGP cards or with a passphrase
``ykpersonalize``, ``ykchalresp``         program and use YubiKey challenge-response
``tpm2_dictionarylockout``                clear a TPM dictionary-attack lockout
``generate-zbm``                          build ZFSBootMenu images; see :doc:`generate-zbm(8) </man/generate-zbm.8>`
``zbm-builder.sh``                        build images in a container; see :doc:`administration`
``zbm-kcl``                               set ZFSBootMenu command-line options; see :doc:`zbm-kcl(8) </man/zbm-kcl.8>`
``lsinitrd``, ``lsinitcpio``              list the contents of an image
========================================  ============================================================================

Kernel Command-Line Parameters
------------------------------

====================================  =========================================================================
Parameter                             Effect
====================================  =========================================================================
``zbm.luks=off``                      do not unlock LUKS volumes
``zbm.luks=auto``                     unlock every LUKS volume, ignoring the crypttab
``zbm.luks=<device>[,...]``           unlock only the listed devices
``zbm.luks_key=<path>[:<dev>]``       key file for volumes without their own; ``*.gpg`` files are decrypted
``zbm.luks_tokens=0``                 do not use LUKS2 tokens (TPM2, FIDO2, clevis)
``zbm.luks_token_wait=<seconds>``     how long to wait for a security key or card (default 10)
``zbm.skip_hooks=80-luks-close.sh``   keep volumes open at hand-off (not recommended)
====================================  =========================================================================

crypttab Options
----------------

The ZFSBootMenu crypttab (*/etc/zfsbootmenu/crypttab*) accepts these options; see
:doc:`zfsbootmenu-crypttab(5) </man/zfsbootmenu-crypttab.5>` for details.

==========================================================  =========================================================
Option                                                      Effect
==========================================================  =========================================================
``discard``                                                 pass TRIM through
``readonly``                                                open read-only
``tries=<n>``                                               passphrase attempts per round (default 3, 0 = unlimited)
``noauto``                                                  do not unlock at start-up
``nofail``                                                  do not wait for a missing device
``keyscript=<path>``                                        key from a script
``header=<path>``                                           detached LUKS header
``keyfile-offset=<n>``, ``keyfile-size=<n>``                part of a key file
``key-slot=<n>``                                            only this key slot
``gpg``                                                     key file is GnuPG-encrypted
``no-tokens``                                               skip LUKS2 tokens
``ykchalresp[=<slot>]``                                     YubiKey challenge-response (slot 1 or 2)
``ykchalresp-hash``, ``ykchalresp-concat``                  ``yubikey-luks`` HASH and CONCATENATE modes
``no-read-workqueue``, ``no-write-workqueue``,              dm-crypt performance flags
``same-cpu-crypt``, ``submit-from-crypt-cpus``
==========================================================  =========================================================

Build Options
-------------

Set in a dracut configuration file (in the directory named by *Global.DracutConfDir*, by default
*/etc/zfsbootmenu/dracut.conf.d*) or in the ZFSBootMenu *mkinitcpio.conf*.

======================================  ===========================  ==============================================
Option                                  Default                      Effect
======================================  ===========================  ==============================================
``zfsbootmenu_luks``                    ``yes``                      LUKS support as a whole
``zfsbootmenu_luks_crypttab``           */etc/zfsbootmenu/crypttab*  crypttab to embed (not in release builds)
``zfsbootmenu_luks_tokens``             ``yes``                      TPM2, FIDO2 and clevis support
``zfsbootmenu_luks_token_plugins``      ``yes``                      systemd's cryptsetup token plugins
``zfsbootmenu_luks_clevis``             ``yes``                      clevis
``zfsbootmenu_luks_gpg``                ``yes``                      GnuPG and ``scdaemon``
``zfsbootmenu_luks_gpg_keys``           (none)                       public keys to export from the host keyring
``zfsbootmenu_luks_gpg_pubkeys``        (none)                       public key files to import
``zfsbootmenu_luks_pcscd``              ``no``                       ``pcscd`` and card reader drivers
``zfsbootmenu_luks_yubikey``            ``yes``                      ``ykchalresp`` and ``ykinfo``
======================================  ===========================  ==============================================

Each optional component is included only if it is installed on the build host.

Files
-----

==============================================  =============================================================
File                                            Purpose
==============================================  =============================================================
*/etc/zfsbootmenu/crypttab*                     volumes to unlock (build host and image)
*/etc/zfsbootmenu/keys/*                        a conventional place for GnuPG-encrypted key files
*/etc/zfsbootmenu/dracut.conf.d/*.conf*         build options for dracut
*/etc/zfsbootmenu/gnupg/*                       public keyring in the image
*/usr/share/examples/zfsbootmenu/crypttab*      annotated example crypttab
*/zfsbootmenu/luks-mappings*                    in the image: mappings opened by ZFSBootMenu
*/zfsbootmenu/luks-skipped*                     in the image: volumes skipped by the user
==============================================  =============================================================

Environment Variables
---------------------

These variables change the behaviour of ``zluks`` when set in the recovery shell:

=============================  ===========================================================================
Variable                       Effect
=============================  ===========================================================================
``LUKS_TPM2_TCTI``             TPM2 TCTI (default ``device:/dev/tpmrm0``)
``LUKS_NO_TOKEN_PLUGINS``      ignore systemd's token plugins, use the native TPM2 and FIDO2 code
``LUKS_DEFAULT_TRIES``         passphrase attempts per round (default 3)
``ZBM_CRYPTTAB``               crypttab to read (default */etc/zfsbootmenu/crypttab*)
=============================  ===========================================================================
