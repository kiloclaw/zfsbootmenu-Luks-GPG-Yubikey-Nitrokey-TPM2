#!/bin/bash
# Test hook: report which runtime started ZFSBootMenu: process 1 and the
# runtime the image records
echo "ZBMTEST-RUNTIME pid1=[$(tr '\0' ' ' < /proc/1/cmdline)] conf=[$(grep -o 'ZBM_RUNTIME="[a-z]*"' /etc/zfsbootmenu.conf)] bootfs=[${BOOTFS}]"
