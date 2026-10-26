#!/bin/bash
# Builds the test boot environment inside the ZFSBootMenu build container:
#
#   <out>/payload/boot/vmlinuz-<kver>          the container's Ubuntu kernel
#   <out>/payload/boot/initramfs-<kver>.img    dracut, systemd, ZFS 2.4.4, crypt,
#                                              QEMU's GPU drivers
#   <out>/payload/rootfs/                      busybox root with zfs, zpool, dmsetup
#
# The initramfs contains /etc/zbmtest/be.key and be-other.key (the test LUKS
# passphrases "pw" and "other" as key files) and /etc/hostid, so the boot environment unlocks its LUKS volumes
# with rd.luks.uuid= and rd.luks.key= parameters and no crypttab.
#
# Usage (from the host):
#   docker run --rm -v <work>:/out -v <repo>/tests/luks/vm/be:/be:ro \
#     --entrypoint /be/build-be.sh localhost/zbm-builder /out

set -e
out="${1:?output directory}"
export DEBIAN_FRONTEND=noninteractive

apt-get update -qq
apt-get install -y -qq --no-install-recommends zfs-dracut busybox-static >/dev/null

kver="$( find /usr/lib/modules -mindepth 1 -maxdepth 1 -printf '%f\n' | sort -V | tail -n 1 )"

# Test key file and hostid shared with the ZFSBootMenu test images
mkdir -p /etc/zbmtest
printf 'pw' > /etc/zbmtest/be.key
printf 'other' > /etc/zbmtest/be-other.key
chmod 0400 /etc/zbmtest/be.key /etc/zbmtest/be-other.key
cp "${out}/hostid" /etc/hostid

mkdir -p "${out}/payload/boot"
cp "/boot/vmlinuz-${kver}" "${out}/payload/boot/vmlinuz-${kver}"
chmod 0644 "${out}/payload/boot/vmlinuz-${kver}"

dracut --force --no-hostonly --no-early-microcode --kver "${kver}" \
  --add "systemd zfs crypt" --omit "plymouth network" \
  --add-drivers "virtio_gpu bochs" \
  --install "/etc/zbmtest/be.key /etc/zbmtest/be-other.key" \
  "${out}/payload/boot/initramfs-${kver}.img"
chmod 0644 "${out}/payload/boot/initramfs-${kver}.img"

# Root file system: busybox for the shell, plus zfs, zpool and dmsetup with
# their libraries
root="${out}/payload/rootfs"
rm -rf "${root}"
mkdir -p "${root}"/{bin,sbin,usr/bin,usr/sbin,etc,proc,sys,dev,run,tmp,root}
cp /usr/bin/busybox-static "${root}/bin/busybox"
cp /be/init.sh "${root}/sbin/init"
chmod 0755 "${root}/sbin/init"
cp /etc/hostid "${root}/etc/hostid"
printf 'NAME="ZBM test boot environment"\nID=zbmtest\n' > "${root}/etc/os-release"

for exe in /usr/sbin/zfs /usr/sbin/zpool /usr/sbin/dmsetup; do
  cp -L "${exe}" "${root}${exe}"
  for lib in $( ldd "${exe}" | grep -o '/[^ ]*' ); do
    mkdir -p "${root}$( dirname "${lib}" )"
    cp -L "${lib}" "${root}${lib}"
  done
done
for applet in sh mount umount cat ls sed grep tr uname sleep sync; do
  ln -sf /bin/busybox "${root}/bin/${applet}"
done

echo "${kver}" > "${out}/payload/kver"
echo "built boot environment for ${kver}"
