#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab
#
# Builds the ZFSBootMenu image with an older ZFS that the oldzfs VM tests
# boot (ZBMTEST_OLD_KERNEL, ZBMTEST_OLD_INITRD): the legacy runtime of this
# checkout on Ubuntu 24.04, with its Linux 6.8 and OpenZFS 2.2, configured
# as the tests' own "auto" image (no crypttab, the test hooks). The legacy
# runtime needs neither Rust nor generate-zbm, so dracut builds it directly.
#
# Usage: tests/luks/vm/old-image.sh [output directory]
#
# The build runs in an ubuntu:24.04 container (docker when installed, else
# podman; ZBMTEST_RUNTIME chooses), and writes vmlinuz-bootmenu,
# initramfs-bootmenu.img and versions.txt to the output directory, by
# default /var/tmp/zbm-luks-test/old. Then:
#
#   ZBMTEST_OLD_KERNEL=/var/tmp/zbm-luks-test/old/vmlinuz-bootmenu \
#   ZBMTEST_OLD_INITRD=/var/tmp/zbm-luks-test/old/initramfs-bootmenu.img \
#   cargo xtask test vm complex-setup complex-oldzfs layouts-parity-oldzfs

set -euo pipefail

RELEASE=24.04

if [ "${1:-}" != "--inside" ]; then
  out="${1:-/var/tmp/zbm-luks-test/old}"
  repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
  runtime="${ZBMTEST_RUNTIME:-}"
  if [ -z "${runtime}" ]; then
    if command -v docker >/dev/null; then runtime=docker; else runtime=podman; fi
  fi
  mkdir -p "${out}"
  out="$(cd "${out}" && pwd)"
  checkout="$(git -C "${repo}" rev-parse --short HEAD 2>/dev/null || echo unknown)"
  exec "${runtime}" run --rm -v "${repo}:/zbm:ro" -v "${out}:/out" \
    -e "ZBM_CHECKOUT=${checkout}" --entrypoint /bin/bash "ubuntu:${RELEASE}" /zbm/tests/luks/vm/old-image.sh --inside
fi

# In the container: the kernel, ZFS and what the legacy runtime installs,
# as packaging/container/setup-image.sh does, without Rust and yq-go
export DEBIAN_FRONTEND=noninteractive
cat > /etc/dpkg/dpkg.cfg.d/90-zbm-noinitramfs <<CFG
path-exclude=/etc/kernel/postinst.d/dracut
path-exclude=/etc/kernel/postinst.d/initramfs-tools
path-exclude=/etc/kernel/postinst.d/zz-update-grub
path-exclude=/usr/lib/kernel/install.d/*dracut*
CFG
apt-get update
kpkgs="$(apt-cache depends linux-image-generic \
  | sed -n 's/^ *Depends: \(linux-[^<>]*\)$/\1/p' | grep -v '^linux-firmware')"
# shellcheck disable=SC2086
apt-get -y install --no-install-recommends ${kpkgs} zfsutils-linux \
  dracut-core dracut-network kexec-tools bsdextrautils ncurses-bin procps \
  kmod udev systemd-sysv zstd binutils fzf mbuffer bash kbd util-linux \
  util-linux-extra less \
  cryptsetup-bin gnupg scdaemon libfido2-1 fido2-tools clevis clevis-luks \
  clevis-tpm2 tpm2-tools yubikey-personalization openssl

kver="$(find /lib/modules -mindepth 1 -maxdepth 1 -printf '%f\n' | sort -V | tail -n 1)"
modinfo -k "${kver}" zfs >/dev/null

# The checkout's legacy runtime and dracut module, as build-init.sh links them
ln -Tsf /zbm/legacy/zfsbootmenu /usr/share/zfsbootmenu
mkdir -p /usr/lib/dracut/modules.d
ln -Tsf /zbm/packaging/dracut /usr/lib/dracut/modules.d/90zfsbootmenu

# The tests pass the host ID on the command line (spl_hostid=)
rm -f /etc/hostid

conf=/tmp/dracut.conf.d
mkdir -p "${conf}"
cp /zbm/config/zfsbootmenu/dracut.conf.d/* "${conf}/"
cat > "${conf}/zz-oldzfs.conf" <<CONF
hostonly="no"
compress="zstd"
zfsbootmenu_runtime="legacy"
zfsbootmenu_hook_root="/zbm/tests/luks/vm/hooks"
zfsbootmenu_luks_crypttab="/zbm/tests/luks/vm/no-crypttab"
CONF

dracut -f --conf /dev/null --confdir "${conf}" --no-early-microcode \
  --kver "${kver}" /out/initramfs-bootmenu.img
cp "/boot/vmlinuz-${kver}" /out/vmlinuz-bootmenu
{
  echo "release: Ubuntu ${RELEASE}"
  echo "kernel: ${kver}"
  echo "zfs: $(dpkg-query -W -f '${Version}' zfsutils-linux)"
  echo "checkout: ${ZBM_CHECKOUT:-unknown}"
} > /out/versions.txt
chmod 0644 /out/vmlinuz-bootmenu /out/initramfs-bootmenu.img /out/versions.txt
cat /out/versions.txt
