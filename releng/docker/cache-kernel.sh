#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

# Download the kernel pinned in kernel-pin (package and headers) from a Void
# Linux mirror into kernel-cache/, verify both against the repository index
# and turn the directory into a local XBPS repository.

usage() {
  cat <<USAGE
Usage: $0 [options]

Options:
  -a <arch>    Void architecture (default: x86_64)
  -R <repo>    Void repository URL (default: https://repo-fastly.voidlinux.org/current,
               with /aarch64 appended for aarch64)
  -e <engine>  Container engine: podman or docker (default: podman if found)
  -o <dir>     Cache directory (default: kernel-cache next to this script)
  -h           Show this help

The kernel version comes from kernel-pin next to this script.
USAGE
}

set -o errexit -o nounset -o pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

arch="x86_64"
repo=""
engine=""
cache="${here}/kernel-cache"

while getopts "a:R:e:o:h" opt; do
  case "${opt}" in
    a) arch="${OPTARG}" ;;
    R) repo="${OPTARG}" ;;
    e) engine="${OPTARG}" ;;
    o) cache="${OPTARG}" ;;
    h) usage; exit 0 ;;
    *) usage >&2; exit 1 ;;
  esac
done

# shellcheck disable=SC1091
. "${here}/kernel-pin"

if [ -z "${KERNEL_SERIES:-}" ] || [ -z "${KERNEL_PKGVER:-}" ]; then
  echo "ERROR: ${here}/kernel-pin must define KERNEL_SERIES and KERNEL_PKGVER" >&2
  exit 1
fi

if [ -z "${repo}" ]; then
  repo="https://repo-fastly.voidlinux.org/current"
  case "${arch}" in
    aarch64*) repo="${repo}/aarch64" ;;
  esac
fi

if [ -z "${engine}" ]; then
  if command -v podman >/dev/null 2>&1; then
    engine=podman
  elif command -v docker >/dev/null 2>&1; then
    engine=docker
  else
    echo "ERROR: neither podman nor docker is available" >&2
    exit 1
  fi
fi

mkdir -p "${cache}"
cache="$(cd "${cache}" && pwd)"

pkgs="linux${KERNEL_SERIES}-${KERNEL_PKGVER} linux${KERNEL_SERIES}-headers-${KERNEL_PKGVER}"
echo "Caching ${pkgs} (${arch}) from ${repo} in ${cache}"

# The work happens in a Void container, which provides a trusted xbps that
# verifies the repository index signature when syncing.
"${engine}" run --rm \
  --env "XBPS_ARCH=${arch}" \
  --env "REPO=${repo}" \
  --env "PKGS=${pkgs}" \
  --volume "${cache}:/cache:z" \
  ghcr.io/void-linux/void-glibc-full:latest \
  sh -s < "${here}/cache-kernel.inner.sh"

echo "Done. Builds will now install ${pkgs% *} from ${cache}."
