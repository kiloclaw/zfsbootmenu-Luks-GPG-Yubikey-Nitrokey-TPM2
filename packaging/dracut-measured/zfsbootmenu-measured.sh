#!/bin/sh
# vim: softtabstop=2 shiftwidth=2 expandtab
#
# Keeps ZFSBootMenu's TPM event log of this boot (ROADMAP.md SE10): from the
# initramfs, which switch_root frees, to /run, which the booted system keeps.

log=/.extra/zfsbootmenu/tpm-measure.json
if [ -f "${log}" ]; then
  mkdir -p /run/log/zfsbootmenu && cp "${log}" /run/log/zfsbootmenu/tpm-measure.json
fi
