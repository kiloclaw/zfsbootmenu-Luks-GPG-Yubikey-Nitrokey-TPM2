#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

# The dracut pre-mount hook of the Rust runtime: zbm sets up the console and
# replaces this shell, process 1, with the supervisor, as
# zfsbootmenu-preinit.sh does with zfsbootmenu-init for the legacy runtime.

exec /libexec/zbm zfsbootmenu-preinit
