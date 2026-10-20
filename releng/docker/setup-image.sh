#!/bin/sh
# vim: softtabstop=2 shiftwidth=2 expandtab
#
# Installs the kernel, ZFS and everything else ZFSBootMenu needs into the
# Ubuntu build container. The Dockerfile runs this script once while building
# the image; KERNEL_FLAVOUR selects the Ubuntu kernel flavour.

set -e

: "${KERNEL_FLAVOUR:=generic}"
export DEBIAN_FRONTEND=noninteractive

# Keep kernel postinst hooks from building an unused initramfs in /boot
cat > /etc/dpkg/dpkg.cfg.d/90-zbm-noinitramfs <<CFG
path-exclude=/etc/kernel/postinst.d/dracut
path-exclude=/etc/kernel/postinst.d/initramfs-tools
path-exclude=/etc/kernel/postinst.d/zz-update-grub
path-exclude=/usr/lib/kernel/install.d/*dracut*
CFG

apt-get update
apt-get -y dist-upgrade

# Resolve the kernel packages of the flavour: the kernel image and its module
# packages, which include the ZFS module (a separate linux-main-modules-zfs-*
# package since Ubuntu 26.04), but not the firmware and microcode the
# metapackage also pulls in
kpkgs="$(apt-cache depends "linux-image-${KERNEL_FLAVOUR}" \
  | sed -n 's/^ *Depends: \(linux-[^<>]*\)$/\1/p' | grep -v '^linux-firmware')"
# shellcheck disable=SC2086
kabi="$(printf '%s\n' ${kpkgs} | sed -n 's/^linux-image-\([0-9].*\)$/\1/p' | head -n 1)"
if [ -z "${kabi}" ]; then
  echo "ERROR: no kernel found for flavour '${KERNEL_FLAVOUR}'"
  exit 1
fi

# Install the kernel, ZFS and the ZFSBootMenu dependencies
# shellcheck disable=SC2086
apt-get -y install --no-install-recommends ${kpkgs} zfsutils-linux \
  dracut-core dracut-network systemd-boot-efi systemd-cryptsetup \
  libsort-versions-perl libboolean-perl libyaml-pp-perl \
  libconfig-inifiles-perl fzf mbuffer kexec-tools bsdextrautils \
  ncurses-bin procps kmod udev zstd binutils curl ca-certificates \
  yq-go bash kbd gdisk iproute2 iputils-ping parted dosfstools \
  e2fsprogs efibootmgr cryptsetup-bin openssh-client util-linux \
  util-linux-extra less kpartx gnupg scdaemon libfido2-1 fido2-tools \
  clevis clevis-luks clevis-tpm2 tpm2-tools yubikey-personalization \
  openssl

# The ZFS module must exist for the installed kernel
if ! modinfo -k "${kabi}" zfs >/dev/null; then
  echo "ERROR: kernel ${kabi} has no ZFS module"
  exit 1
fi

# Select this kernel by default in build-init.sh
kver="${kabi%%-*}"
echo "${kver%.*}.*" > /etc/zbm-kernel-version

# Hold the kernel and ZFS so that build-init.sh -u keeps them
# shellcheck disable=SC2046,SC2086
apt-mark hold ${kpkgs} $(dpkg-query -W -f '${Package}\n' \
  | grep -E '^(zfsutils-linux|libzfs|libzpool|libnvpair|libuutil)')

# Leave only versioned kernel files in /boot
rm -f /boot/initrd.img* /boot/initramfs* /boot/vmlinuz /boot/vmlinuz.old
apt-get clean
rm -rf /var/lib/apt/lists/*
