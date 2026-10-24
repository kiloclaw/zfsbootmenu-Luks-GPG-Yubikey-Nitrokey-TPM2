=========
zbm-debug
=========

SYNOPSIS
========

**/libexec/zbm debug** **config**

**/libexec/zbm debug** **state**

**/libexec/zbm debug** **journal** [**--level** *level*]

**/libexec/zbm debug** **commands**

**/libexec/zbm debug** **luks**

**/libexec/zbm debug** **pools**

**/libexec/zbm debug** **crash** [**--list**]

**/libexec/zbm debug** **saved**

**/libexec/zbm debug** **save**

DESCRIPTION
===========

**zbm debug** shows what the ZFSBootMenu runtime was told, what it did and what it sees, from the recovery shell or
an SSH session. It is meant for finding out why a boot went wrong, and for bug reports. Every report only reads: none
of them imports, unlocks, closes or changes anything, and none of them prints key material.

**zbm debug** is part of the **zbm** program, which images built with the Rust runtime (and other images when **zbm**
was found on the build host) carry at */libexec/zbm*. The journal, the command list and the saved command line are
written by the Rust runtime only.

SUBCOMMANDS
===========

**config**

  Print the command line that start-up read, then every ZFSBootMenu option of the kernel command line, one per line:
  its name, the value in force, and where that value came from. The origin is **command line** when the option was
  given, **command line, as** *alias* when it was given under an older name, **default** when it was not, and
  **default:** *name*\ **=**\ *value* **is not valid** when it was given a value ZFSBootMenu refused. The warnings
  the options give follow, as start-up gave them. Options are described in
  :doc:`zfsbootmenu(7) </man/zfsbootmenu.7>`.

**state**

  Print the files of the state directory, */zfsbootmenu*, by name, with their size, and the contents of the files of
  4096 bytes or less. The journal and the crash reports are left out, as they have reports of their own, and so are
  the key files cached from **org.zfsbootmenu:keysource** (*/zfsbootmenu/.keys*), which are secrets.

**journal** [**--level** *level*]

  Print the runtime journal, one record per line: the process ID, the level, the part of the runtime that wrote the
  record, the message and its fields, and the operations it was part of, outermost first, in brackets. With
  **--level**, print only the records of *level* or a more severe one: **error**, **warn**, **info**, **debug** or
  **trace**. Fields that could hold a secret are written as *[redacted]* when the record is made, so they are never
  in the journal.

**commands**

  Print the programs the runtime ran, in order, each with its exit status (**-** when it could not start), how long
  it took and its command line, then how many there were, how many failed or did not start, and how long they took in
  all. The input given to a program is never shown.

**luks**

  Print each LUKS volume: its device, its mapping name, its state (**open**, **locked**, **skipped**, or **failed:**
  with the reason), the key methods its header and the configuration offer, and the pool on it when it is open. The
  volumes are those the configuration selects, as for **zluks unlock** in :doc:`zluks(8) </man/zluks.8>`.

**pools**

  Print each imported pool: its health, whether it is imported read/write or read-only, and whether it has a
  checkpoint. Then each boot environment, indented: whether it is its pool's default (**bootfs**), its encryption
  (**not encrypted**, **locked** or **unlocked**), and the kernel command line it boots with, with where that was set:
  **set here** for its own **org.zfsbootmenu:commandline** property, **from** *dataset* when it inherits the property,
  and **default** when no dataset sets it.

**crash** [**--list**]

  Print the latest crash report of the runtime, or with **--list** list them, oldest first. A developer turns a
  report into function names and lines with the debug file of the image's build.

**saved**

  List the boots whose logs were saved on the EFI system partition (**zbm.save_logs** in
  :doc:`zfsbootmenu(7) </man/zfsbootmenu.7>`), and their files.

**save**

  Save this boot's logs on the EFI system partition now.

FILES
=====

*/zfsbootmenu/zbm.cmdline*

  The kernel command line start-up read, one argument per line, which **config** prints.

*/zfsbootmenu/journal.jsonl*

  The runtime journal, one JSON object per line, which **journal** and **commands** read.

*/zfsbootmenu/crash-*\ *time*\ *-*\ *pid*\ *.txt*

  Crash reports.

*/etc/zfsbootmenu.conf*

  The configuration of the image, which **luks** and **pools** read for the LUKS settings.

EXIT STATUS
===========

**0**

  The report was printed.

**1**

  There was nothing to report from (no journal, no saved command line, an empty state directory, no crash report),
  or the machine could not be read (**zpool** or **zfs** failed); a message on standard error says which.

**2**

  The arguments were not understood; the usage is printed on standard error.

EXAMPLES
========

Example 1: Finding a failed command
-----------------------------------

The following commands show the programs that failed, then the warnings and errors around them::

  # /libexec/zbm debug commands | grep -v '^   0 '
     1      84 ms  zpool import -N -o readonly=on zroot
     -       0 ms  tpm2_unseal -c 0x81000001 -p pcr:sha256:7
  14 commands, 2 failed or did not start, 412 ms in all
  # /libexec/zbm debug journal --level warn

Example 2: Checking an option
-----------------------------

The following command shows whether **zbm.timeout** was given, under which name, and the value in force::

  # /libexec/zbm debug config | grep timeout
  zbm.timeout                5                        command line, as timeout

Example 3: Showing the volumes and pools
----------------------------------------

::

  # /libexec/zbm debug luks
  /dev/sda2  luks-0a1b2c3d-...  open    methods: TPM2, passphrase  pool: zroot
  /dev/sdb2  /dev/sdb2          locked  methods: FIDO2, passphrase
  # /libexec/zbm debug pools
  zroot  ONLINE  read/write
    zroot/ROOT/void (default)  unlocked  command line: quiet (from zroot/ROOT)

SEE ALSO
========

:doc:`zfsbootmenu(7) </man/zfsbootmenu.7>`, :doc:`zluks(8) </man/zluks.8>`,
:doc:`zfsbootmenu-luks(7) </man/zfsbootmenu-luks.7>`, **zpool**\ (8), **zfs**\ (8)
