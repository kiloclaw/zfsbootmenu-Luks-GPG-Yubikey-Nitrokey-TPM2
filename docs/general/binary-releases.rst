Binary Releases
===============

ZFSBootMenu releases include pre-built ``release`` and ``recovery`` images, each distributed as an EFI executable or a
"component" archive consisting of a separate kernel and initramfs image. The EFI executables should be directly bootable
by most UEFI firmware implementations or boot managers including rEFInd, gummiboot and systemd-boot. The separate kernel
and initramfs components may also be used on UEFI systems; the kernels used for ZFSBootMenu binary releases always
include the built-in EFI stub and may be directly bootable (provided the UEFI implementation can pass command-line
arguments, including the path to the associated initramfs image, to the kernel) or loaded by a boot manager. In
addition, the separate components may be booted by any standard BIOS boot loader (*e.g.*, syslinux) on legacy hardware.

Each release also carries ``zfsbootmenu-debug-x86_64-v<version>.tar.xz``, the debug files of the project's own
programs in its images (``zbm`` today), named by their build IDs; the other programs of an image, from the build
container's distribution, have none in it. Booting needs none of it: a developer uses it to turn a crash report of a
release image (``zbm debug crash``) into functions, files and lines. Like every asset, it is listed in ``sha256.txt``
and signed.

Release images
~~~~~~~~~~~~~~

Release images include all user-space tools necessary for full functionality within ZFSBootMenu, and include a minimal
selection of additional tools that might be helpful in a pre-boot environment. In general, release images are
recommended for normal system operation.

The extra tooling includes:

.. include:: _include/release.rst

Recovery images
~~~~~~~~~~~~~~~

The tools available in recovery images are a super-set of the tools included in release images. Recovery images may be
useful for rebuilding unbootable systems from within the ZFSBootMenu emergency shell, and includes basic components for
network access as well as utilities to manipulate disks and file systems. It may be desirable to keep a recovery image
installed alongside the standard release image, and configure a backup boot option pointing to this recovery image.

The extra tooling includes:

.. include:: _include/recovery.rst
