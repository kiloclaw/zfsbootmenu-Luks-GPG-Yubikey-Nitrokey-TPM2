#!/bin/bash
# Test hook: report what is still mapped, imported and recorded at teardown
dm=""
for m in /dev/mapper/*; do
  [ -e "${m}" ] && [ "${m##*/}" != "control" ] && dm="${dm}${m##*/} "
done
echo "ZBMTEST-TEARDOWN dm=[${dm}] pools=[$(zpool list -H -o name 2>/dev/null | tr '\n' ' ')] state=[$(tr '\n' ' ' < /zfsbootmenu/luks-mappings 2>/dev/null)]"
