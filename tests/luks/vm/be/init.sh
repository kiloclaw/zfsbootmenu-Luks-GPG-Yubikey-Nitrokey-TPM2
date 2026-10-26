#!/bin/busybox sh
# shellcheck shell=sh
# /sbin/init of the test boot environment. It runs after ZFSBootMenu has
# kexec'd the boot environment kernel and the boot environment's own dracut
# initramfs (ZFS 2.4.4, systemd-cryptsetup) has unlocked its LUKS volumes and
# mounted the root dataset. It reports what the test harness checks, then
# powers off.

bb=/bin/busybox
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH

$bb mount -t proc proc /proc 2>/dev/null
$bb mount -t sysfs sysfs /sys 2>/dev/null
$bb mount -t devtmpfs devtmpfs /dev 2>/dev/null

echo "BE-INIT-START"
echo "BE-CMDLINE: $($bb cat /proc/cmdline)"
echo "BE-KERNEL: $($bb uname -r)"
echo "BE-ZFS-VERSION: $(/usr/sbin/zfs version 2>&1 | $bb tr '\n' ' ')"

while read -r dev mnt fs opts rest; do
  [ "${mnt}" = "/" ] && echo "BE-ROOT: ${dev} ${fs} ${opts%%,*}"
done < /proc/mounts

# The displays the kernel brought up, after ZFSBootMenu handed its GPUs back
drm=""
for card in /sys/class/drm/card*; do
  case "${card##*/}" in *-*) continue ;; esac
  [ -e "${card}/device/driver" ] || continue
  driver="$($bb readlink -f "${card}/device/driver")"
  # virtio_gpu makes its card on the PCI device, which virtio-pci drives
  for sub in "${card}"/device/virtio*/driver; do
    [ -e "${sub}" ] && driver="$($bb readlink -f "${sub}")"
  done
  drm="${drm}${card##*/}=${driver##*/} "
done
echo "BE-DRM: ${drm}"

echo "BE-MAPPER: $($bb ls /dev/mapper | $bb tr '\n' ' ')"
echo "BE-DMTABLE-BEGIN"
/usr/sbin/dmsetup table 2>&1 | $bb sed 's/^/BE-DM| /'
echo "BE-DMTABLE-END"

# Import every other pool whose devices the initramfs unlocked
/usr/sbin/zpool import -a -N 2>&1 | $bb sed 's/^/BE-IMPORT| /'

echo "BE-STATUS-BEGIN"
/usr/sbin/zpool status -P 2>&1 | $bb sed 's/^/BE| /'
echo "BE-STATUS-END"

echo "BE-FEATURES-BEGIN"
/usr/sbin/zpool get -H -o name,property,value all 2>/dev/null \
  | $bb grep 'feature@' | $bb sed 's/^/BE-F| /'
echo "BE-FEATURES-END"

# The root dataset must be writable with this ZFS version
if $bb mount -o remount,rw / 2>/dev/null && echo "written by $(uname -r)" > /be-write-test \
    && $bb sync && [ -s /be-write-test ]; then
  echo "BE-WRITE: ok"
else
  echo "BE-WRITE: failed"
fi

# Scrub every imported pool and report errors
for pool in $(/usr/sbin/zpool list -H -o name); do
  /usr/sbin/zpool scrub -w "${pool}" >/dev/null 2>&1
  echo "BE-SCRUB: ${pool} $(/usr/sbin/zpool status -x "${pool}" 2>&1 | $bb tr '\n' ' ')"
done

echo "BE-INIT-DONE"
$bb sync
for pool in $(/usr/sbin/zpool list -H -o name); do
  /usr/sbin/zpool export "${pool}" >/dev/null 2>&1
done
echo o > /proc/sysrq-trigger
$bb sleep 30
