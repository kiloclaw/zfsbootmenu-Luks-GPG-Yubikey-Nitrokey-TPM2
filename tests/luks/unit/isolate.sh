# shellcheck shell=bash
# Sourced first by every unit suite.
#
# The suites run ZFSBootMenu's LUKS code against real LUKS2 headers on loop
# devices, but replace device-mapper with stubs that create plain nodes in
# /dev/mapper. To keep them away from the host's own mappings, each suite
# re-executes itself in a private mount namespace with an empty tmpfs on
# /dev/mapper. Loop devices are detached only if they are backed by files of
# the suite (see detach_own_loops). The host's HID devices (keyboards, security
# keys) are hidden behind empty files, since the code under test takes any
# /dev/hidraw* for a FIDO2 key; the suites make their own, /dev/hidraw99.

if [ -z "${ZBMTEST_UNIT_ISOLATED:-}" ]; then
  if [ "$(id -u)" -ne 0 ]; then
    echo "SKIP: the LUKS unit tests need root (loop devices, mknod, mount namespaces)"
    exit 77
  fi
  # /dev/mapper is created (and removed afterwards) only on hosts without
  # device-mapper; the tmpfs mount itself is private to the namespace
  # Suites share /dev/hidraw99, swtpm ports and work directories: one runs at
  # a time, and another waits for the lock (held by flock, not by daemons the
  # suite starts, which -o keeps from inheriting it)
  exec flock -o /run/lock/zbm-luks-unit.lock unshare --mount --propagation private \
    env ZBMTEST_UNIT_ISOLATED=1 \
    bash -c 'made=; [ -d /dev/mapper ] || { mkdir /dev/mapper && made=1; }
             mount -t tmpfs -o mode=0755 zbmtest-mapper /dev/mapper || exit 1
             blank=$(mktemp)
             for d in /dev/hidraw*; do
               [ -c "$d" ] && [ "$d" != /dev/hidraw99 ] && mount --bind "$blank" "$d"
             done
             rm -f "$blank"
             "$@"; rc=$?
             umount /dev/mapper; [ -n "${made}" ] && rmdir /dev/mapper
             exit "${rc}"' bash "$0" "$@"
fi

# detach_own_loops DIR : detach loop devices whose backing file is under DIR
detach_own_loops() {
  losetup -l -n -O NAME,BACK-FILE 2>/dev/null \
    | awk -v dir="${1%/}/" 'index($2, dir) == 1 { print $1 }' \
    | xargs -r -n1 losetup -d 2>/dev/null
  return 0
}
