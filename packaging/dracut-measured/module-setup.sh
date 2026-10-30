#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab
#
# The boot environment's side of measured boot (ROADMAP.md SE10, ADR 0008):
# ZFSBootMenu appends its TPM event log to the initramfs it boots, at
# /.extra/zfsbootmenu/tpm-measure.json, which the initramfs frees at
# switch_root; this module's hook copies it to /run/log/zfsbootmenu/,
# which the booted system keeps. A boot that ZFSBootMenu did not measure has
# no log, and the hook does nothing.

check() {
  # Included by default: it costs one small script
  return 0
}

depends() {
  return 0
}

install() {
  inst_multiple mkdir cp
  # shellcheck disable=SC2154
  inst_hook pre-pivot 90 "${moddir}/zfsbootmenu-measured.sh"
}
