#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

## Close the LUKS mappings that ZFSBootMenu opened to import ZFS pools.
##
## The boot environment's own initramfs must unlock its LUKS volumes again
## after kexec, so nothing is lost here. Exporting the affected pools and
## closing the mappings guarantees a clean hand-off and makes the kernel wipe
## the dm-crypt volume keys from memory before the new kernel starts.
##
## This hook may be disabled with zbm.skip_hooks=80-luks-close.sh

sources=(
  /lib/profiling-lib.sh
  /etc/zfsbootmenu.conf
  /lib/kmsg-log-lib.sh
  /lib/zfsbootmenu-core.sh
  /lib/zfsbootmenu-luks.sh
)

for src in "${sources[@]}"; do
  # shellcheck disable=SC1090
  source "${src}" >/dev/null 2>&1 || exit 0
done

unset src sources

luks_close_all
exit 0
