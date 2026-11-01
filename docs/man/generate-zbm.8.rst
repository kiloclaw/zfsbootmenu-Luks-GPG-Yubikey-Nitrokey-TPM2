============
generate-zbm
============

.. KEEP IN SYNC WITH crates/zbm-host/src/generate_zbm

.. only:: html

  .. toctree::
    :hidden:

    generate-zbm.5

SYNOPSIS
========

**generate-zbm** [options]

OPTIONS
=======

Where noted, command-line options supersede options in the :doc:`generate-zbm(5) </man/generate-zbm.5>` configuration file.

Options take the spellings shown here: a long option as **--name** with its value as **--name=value** or the next
argument, and a short one as **-x**, grouped with others or with its value attached. The other spellings that earlier
versions took (a unique start of a long name such as **--ver**, **+name**, **-name** with one dash, and **--noname**)
still work in this release, each with a warning that names the spelling to use; they stop working in the next
release. Operands may come anywhere, and are ignored; **--** ends the options.

**--version|-v** *zbm-version*

  Override the ZFSBootMenu version in output files; supersedes *Global.Version*

**--kernel|-k** *kernel-path*

  Manually specify a specific kernel; supersedes *Kernel.Path*

**--kver|-K** *kernel-version*

  Manually specify a specific kernel version; supersedes *Kernel.Version*

**--prefix|-p** *image-prefix*

  Manually specify the output image prefix; supersedes *Kernel.Prefix*

**--initcpio|-i**

  Force the use of mkinitcpio instead of dracut.

**--no-initcpio|-i**

  Force the use of dracut instead of mkinitcpio.

**--confd|-C** *config-path*

  Specify initramfs configuration path

  * For dracut: supersedes *Global.DracutConfDir*

  * For mkinitcpio: supersedes *Global.InitCPIOConfig*

**--hookd|-H** *hookd-path*

  Specify mkinitcpio hook directory; supersedes *Global.InitCPIOHookDirs*

  May be specified more than once. Ignored when using dracut.

**--cmdline|-l** *options*

  Override the kernel command line; supersedes *Kernel.CommandLine*

**--bootdir|-b** *boot-path*

  Specify the path to search for kernel files; default: */boot*

**--config|-c** *conf-file*

  Specify the path to a configuration file; default: */etc/zfsbootmenu/config.yaml*

**--enable**

  Set the *Global.ManageImages* option to true, enabling image generation.

**--disable**

  Set the *Global.ManageImages* option to false, disabling image generation.

**--debug|-d**

  Enable debug output

**--showver|-V**

  Print ZFSBootMenu version and quit.

**--dry-run|-n**

  Print what would be done (mount the boot partition, run hooks, build and install images, keep backups
  and prune old versions, change *Global.ManageImages*), one line each starting with *Would*, and do none
  of it. The programs that only read, such as *strings* on the kernels, still run.

ENVIRONMENT
===========

**SOURCE_DATE_EPOCH**

  When set, images are built to be reproducible: **dracut** gets **--reproducible**, **mkinitcpio** uses the
  variable for the times it records, and the EFI image carries it as its time stamp, so two builds from the same
  files give the same bytes. Without it, the EFI image carries the stub's own time stamp.

SEE ALSO
========

:doc:`generate-zbm(5) </man/generate-zbm.5>` :doc:`zfsbootmenu(7) </man/zfsbootmenu.7>`
