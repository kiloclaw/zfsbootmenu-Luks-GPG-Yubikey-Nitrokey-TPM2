# syntax=docker/dockerfile:1.4
#
# This Dockerfile creates a container that will create an EFI executable and
# separate kernel/initramfs components from a ZFSBootMenu repository. The
# container will pre-populate its /zbm directory with a clone of the master
# branch of the upstream ZFSBootMenu branch and build the images from that.
#
# To use a different ZFSBootMenu repository or version, bind-mound the
# repository you want on /zbm inside the container.
#
# Note - this Dockerfile depends on Docker >= 23 and the docker buildx plugin.

# Use the official Void Linux container
FROM ghcr.io/void-linux/void-glibc-full
LABEL org.opencontainers.image.authors="ZFSBootMenu Team, https://zfsbootmenu.org"

ARG XBPS_REPOS="https://repo-fastly.voidlinux.org/current"

# Include the specified Void Linux kernels in the image. Entries take the form
# linux<major>.<minor> (newest build of a series) or
# linux<major>.<minor>-<version>_<revision> (an exact package version).
#
# (multiple entries must be seperated by spaces)
#
# When empty, the exact version pinned in the file kernel-pin is installed
# (Linux 7.2.11, which requires OpenZFS >= 2.4.4). Installed kernels are held
# so that later package updates do not replace them.
ARG KERNELS=
# Update repos and install kernels and base dependencies.

# Run the following within an external cache (/var/cache/xbps) for the
# package manager; so when this layer is rebuilt, at least you save
# some bandwidth. The build context is mounted at /ctx to read kernel-pin
# and, if it has been populated with cache-kernel.sh, the local repository in
# kernel-cache that keeps the pinned kernel available.
RUN --mount=type=cache,target=/var/cache/xbps \
	--mount=type=bind,source=.,target=/ctx <<-EOF
	set -e

	if [ -z "${KERNELS}" ]; then
		. /ctx/kernel-pin
		KERNELS="linux${KERNEL_SERIES}-${KERNEL_PKGVER}"
	fi

	mkdir -p /etc/xbps.d
	for _repo in ${XBPS_REPOS}; do
		echo "repository=${_repo}" >> /etc/xbps.d/00-custom-repos.conf
	done

	# Prefer the local kernel cache over every other repository
	if [ -e "/ctx/kernel-cache/$(xbps-uhelper arch)-repodata" ]; then
		if [ -e /ctx/kernel-cache/SHA256SUMS ]; then
			( cd /ctx/kernel-cache && sha256sum -c SHA256SUMS )
		fi
		echo "repository=/ctx/kernel-cache" > /etc/xbps.d/00-0-kernel-cache.conf
	fi

	# Ensure everything is up-to-date
	xbps-install -Suy xbps && xbps-install -Suy

	# Prefer an LTS version over whatever Void thinks is current
	echo "ignorepkg=linux" > /etc/xbps.d/10-nolinux.conf
	echo "ignorepkg=linux-headers" >> /etc/xbps.d/10-nolinux.conf

	# Prevent initramfs kernel hooks from being installed
	echo "noextract=/usr/libexec/mkinitcpio/*" > /etc/xbps.d/15-noinitramfs.conf
	echo "noextract=/usr/libexec/dracut/*" >> /etc/xbps.d/15-noinitramfs.conf

	# linux7.2-7.2.11_1 -> name linux7.2, headers linux7.2-headers-7.2.11_1
	for _kern in ${KERNELS}; do
		_name="${_kern%%-[0-9]*}"
		kern_names="${kern_names} ${_name}"
		kern_headers="${kern_headers} ${_name}-headers${_kern#"${_name}"}"
		kern_header_names="${kern_header_names} ${_name}-headers"
	done

	# Exact versions vanish from Void mirrors once a newer build is published
	for _kern in ${KERNELS}; do
		case "${_kern}" in
			*-[0-9]*) ;;
			*) continue ;;
		esac
		if [ "$(xbps-query -R -p pkgver "${_kern}" 2>/dev/null)" != "${_kern}" ]; then
			echo "ERROR: ${_kern} is not available from any repository."
			echo "Populate kernel-cache with cache-kernel.sh while it is current,"
			echo "or update kernel-pin to a version Void still provides."
			exit 1
		fi
	done

	# Install ZFSBootMenu dependencies and components necessary to build images
	zbm_deps="$(xbps-query -Rp run_depends zfsbootmenu)"
	xbps-install -y ${KERNELS} ${kern_headers} ${zbm_deps} \
		zstd systemd-boot-efistub curl yq-go bash kbd \
		dracut mkinitcpio dracut-network gptfdisk iproute2 iputils parted \
		dosfstools e2fsprogs efibootmgr cryptsetup openssh util-linux kpartx

	# Keep the installed kernels at the requested versions
	xbps-pkgdb -m hold ${kern_names}

	# Remove headers and development toolchain, but keep binutils for objcopy
	echo "ignorepkg=dkms" > /etc/xbps.d/10-nodkms.conf
	xbps-pkgdb -m manual binutils
	xbps-remove -Roy dkms ${kern_header_names}

	# The kernel cache is only mounted during this step
	rm -f /etc/xbps.d/00-0-kernel-cache.conf
	EOF

# ZFSBootMenu commit hash, tag or branch name used by
# default to build ZFSBootMenu images (default: master)
ARG ZBM_COMMIT_HASH=
# Record a commit hash if one was provided
RUN [ -z "${ZBM_COMMIT_HASH}" ] || echo "${ZBM_COMMIT_HASH}" > /etc/zbm-commit-hash

# Include the specified Void Linux package in the image
# (multiple entries must be seperated by spaces)
ARG PACKAGES=
# Run ${PACKAGES} install in seperate layer so that the zfs dkms packages
# are not rebuilt when ${PACKAGES} change. reuse. Additionally: use xbps cache.
# Install ZFSBootMenu dependencies and components necessary to build images
RUN --mount=type=cache,target=/var/cache/xbps \
	[ -z "${PACKAGES}" ] || xbps-install -y ${PACKAGES}

# Free space in image
RUN rm -f /var/cache/xbps/*

ARG ZBM_BUILDER=build-init.sh
COPY --chmod=755 ${ZBM_BUILDER} /
# Run the build script with no arguments by default
ENTRYPOINT [ "/${ZBM_BUILDER}" ]
