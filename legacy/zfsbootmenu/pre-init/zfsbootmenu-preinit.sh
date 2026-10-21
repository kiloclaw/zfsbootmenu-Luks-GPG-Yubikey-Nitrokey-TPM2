#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

# Disable all kernel messages to the console
echo 0 > /proc/sys/kernel/printk

# fzf needs stty cols/rows explicitly set, otherwise it can't
# determine the serial terminal size. Defaults should only be
# called when the controlling terminal is NOT /dev/tty[0-9]

tty_re='/dev/tty[0-9]'

#shellcheck disable=SC2154
if ! [[ ${control_term} =~ ${tty_re} ]]; then
  stty rows "${zbm_lines:-25}"
  stty cols "${zbm_columns:-80}"
fi

export BASE="/zfsbootmenu"

# arg1: a value
# prints: the value between single quotes, with each single quote in it
#         written as '\'', so that sourcing the line gives the value back
# returns: nothing
#
# Values come from the kernel command line and may contain quotes.

zbm_conf_quote() {
  local quote="'"
  printf "'%s'" "${1//${quote}/${quote}\\${quote}${quote}}"
}

# shellcheck disable=SC2154
cat >> "/etc/zfsbootmenu.conf" <<EOF
# BEGIN additions by zfsbootmenu-preinit.sh
export BASE=$( zbm_conf_quote "${BASE}" )
export spl_hostid=$( zbm_conf_quote "${spl_hostid}" )
export import_policy=$( zbm_conf_quote "${import_policy}" )
export menu_timeout=$( zbm_conf_quote "${menu_timeout}" )
export loglevel=$( zbm_conf_quote "${loglevel}" )
export zbm_prefer_pool=$( zbm_conf_quote "${zbm_prefer_pool}" )
export zbm_require_pool=$( zbm_conf_quote "${zbm_require_pool}" )
export zbm_prefer_bootfs=$( zbm_conf_quote "${zbm_prefer_bootfs}" )
export default_hostid=00bab10c
export zbm_sort=$( zbm_conf_quote "${zbm_sort}" )
export zbm_set_hostid=$( zbm_conf_quote "${zbm_set_hostid}" )
export zbm_retry_delay=$( zbm_conf_quote "${zbm_retry_delay}" )
export zbm_hook_root=$( zbm_conf_quote "${zbm_hook_root}" )
export zbm_wait_for_devices=$( zbm_conf_quote "${zbm_wait_for_devices}" )
export control_term=$( zbm_conf_quote "${control_term}" )
# END additions by zfsbootmenu-preinit.sh
EOF

# Set a non-empty hostname so we show up in zpool history correctly
echo "ZFSBootMenu" > /proc/sys/kernel/hostname

# https://busybox.net/FAQ.html#job_control
ZFSBOOTMENU_CONSOLE=yes exec setsid \
    bash -c "exec /libexec/zfsbootmenu-init <${control_term} >${control_term} 2>&1"
