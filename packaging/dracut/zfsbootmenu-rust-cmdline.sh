#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

# The dracut cmdline hook of the Rust runtime: zbm parses the command line
# into /etc/zfsbootmenu.conf, as zfsbootmenu-parse-commandline.sh does for the
# legacy runtime. dracut reads the variables below from its own shell, which a
# child process cannot set, so they stay here.

if ! /libexec/zbm zfsbootmenu-parse-commandline; then
  echo "ERROR: failed to parse the ZFSBootMenu command line"
fi

# Make sure Dracut is happy that we have a root

# shellcheck disable=SC2034
rootok=1

# Dracut requires root to be defined
# shellcheck disable=SC2034
root=zfsbootmenu
ln -s /dev/null /dev/root 2>/dev/null
