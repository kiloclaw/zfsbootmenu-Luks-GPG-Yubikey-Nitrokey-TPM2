# This Dockerfile creates the container that builds ZFSBootMenu EFI
# executables and separate kernel/initramfs components. It is based on
# Ubuntu: the kernel and the ZFS module are Ubuntu's own packages (Ubuntu
# builds the module for each kernel, so no DKMS build or kernel headers are
# needed), and the Ubuntu release decides the kernel series and OpenZFS
# version. Ubuntu 26.10 ships Linux 7.3 and OpenZFS 2.4.4.
#
# Build the image from the root of the repository with
#
#   podman build -t localhost/zbm-builder releng/docker
#
# (or "docker build" with the same arguments) and run it with zbm-builder.sh,
# whose default image is localhost/zbm-builder:latest.
#
# The container records its kernel series in /etc/zbm-kernel-version, and
# build-init.sh uses it when the configured Kernel.Version matches no
# installed kernel. Images are built with dracut; Ubuntu does not package
# mkinitcpio. systemd's cryptsetup token plugins are installed, so images
# unlock TPM2, FIDO2 and PKCS#11 LUKS2 tokens through systemd's own code.
#
# The image does not contain ZFSBootMenu; the entrypoint fetches it, or uses a
# copy bind-mounted on /zbm.
#
# The image builds with podman, buildah, or Docker 23 or later.

# Ubuntu 26.10 or later: earlier releases do not package yq-go, which
# build-init.sh needs
ARG UBUNTU_RELEASE=26.10
FROM ubuntu:${UBUNTU_RELEASE}
LABEL org.opencontainers.image.authors="ZFSBootMenu Team, https://zfsbootmenu.org"

# Ubuntu kernel flavour to install; the matching kernel image, modules and ZFS
# module packages are installed without the firmware and microcode packages,
# which an EFI boot loader does not need
ARG KERNEL_FLAVOUR=generic

ENV DEBIAN_FRONTEND=noninteractive

# Install the kernel, ZFS and the ZFSBootMenu dependencies
COPY --chmod=755 setup-image.sh /tmp/setup-image.sh
RUN /tmp/setup-image.sh && rm -f /tmp/setup-image.sh

# ZFSBootMenu commit hash, tag or branch name used by
# default to build ZFSBootMenu images (default: master)
ARG ZBM_COMMIT_HASH=
# Record a commit hash if one was provided
RUN [ -z "${ZBM_COMMIT_HASH}" ] || echo "${ZBM_COMMIT_HASH}" > /etc/zbm-commit-hash

# Include the specified Ubuntu packages in the image
# (multiple entries must be separated by spaces)
ARG PACKAGES=
# Install ${PACKAGES} in a separate layer so that the kernel layer is reused
RUN [ -z "${PACKAGES}" ] || { apt-get update && \
	apt-get -y install --no-install-recommends ${PACKAGES} && \
	apt-get clean && rm -rf /var/lib/apt/lists/*; }

ARG ZBM_BUILDER=build-init.sh
# Install the build script under a fixed name: variables are not expanded in
# the exec form of ENTRYPOINT
COPY --chmod=755 ${ZBM_BUILDER} /build-init.sh
# Run the build script with no arguments by default
ENTRYPOINT [ "/build-init.sh" ]
