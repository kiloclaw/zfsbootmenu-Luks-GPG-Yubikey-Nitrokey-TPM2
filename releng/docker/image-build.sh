#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

usage() {
  cat <<-EOF
	USAGE: $0 [OPTIONS] <tag> [zbm-commit-like]
	Create a container image for building ZFSBootMenu
	
	OPTIONS
	
	-h: Display this message and exit
	
	-r <path>:
	   Mount the specified path into the build container when installing
	   packages and configure XBPS to use the path as a package repository
	
	   (One path per argument; may be repeated as needed)
	
	-R <repo>:
	   Configure XBPS to use the specified Void Linux package repository
	
	   Default: https://repo-fastly.voidlinux.org/current/
	
	   (One repository per argument; may be repeated as needed)
	
	-k <kver>:
	   Include the specified Void Linux kernel in the image; <kver> takes
	   the form <major>.<minor> for the newest build of a series (Void
	   packages linux<kver> and linux<kver>-headers) or
	   <major>.<minor>-<version>_<revision> for an exact package version
	
	   Default: the exact version pinned in releng/docker/kernel-pin
	   (currently 7.2-7.2.11_1). If releng/docker/kernel-cache holds a
	   repository made by cache-kernel.sh, it is preferred over all others.
	
	   (One version per argument; may be repeated as needed)
	
	-p <pkg>:
	   Include the specified Void Linux package in the image
	   (One package per argument; may be repeated as needed)
	
	
	ARGUMENTS
	
	<tag>:
	   Tag assigned to container image
	
	[zbm-commit-like]:
	   ZFSBootMenu commit hash, tag or branch name used by
	   default to build ZFSBootMenu images (default: master)
	EOF
}

set -o errexit

extra_pkgs=()

kern_series=()

# By default, mount tmpfs on /var/cache/xbps
host_mounts=( --mount "type=tmpfs,dst=/var/cache/xbps" )

host_repos=()

web_repos=()

while getopts "hr:R:k:p:" opt; do
  case "${opt}" in
    r)
      target="/pkgs/${#host_repos[@]}"
      host_repos+=( "${target}" )
      host_mounts+=( --mount "type=bind,src=${OPTARG},dst=${target},ro" )
      ;;
    R)
      # Sanitize repo arguments
      case "${OPTARG}" in
        /*|http://*|https://*)
          ;;
        *)
          echo "ERROR: '${OPTARG}' does not appear to be a valid repository"
          exit 1
          ;;
      esac

      web_repos+=( "${OPTARG}" )
      ;;
    k)
      kern_series+=( "linux${OPTARG}" )
      ;;
    p)
      extra_pkgs+=( "${OPTARG}" )
      ;;
    h)
      usage
      exit
      ;;
    *)
      usage
      exit 1
      ;;
  esac
done

shift $((OPTIND-1))

# A tag for the image is required
tag="${1}"
if [ -z "${tag}" ]; then
  usage
  exit 1
fi

# If a commit hash is unspecified, try to pull HEAD from git
zbm_commit_hash="$2"
if [ -z "${zbm_commit_hash}" ]; then
  if ! zbm_commit_hash="$(git rev-parse HEAD 2>/dev/null)"; then
    unset zbm_commit_hash
  fi
fi

# Use default Void repo when nothing was specified
if [ "${#web_repos[@]}" -lt 1 ]; then
  web_repos=( "https://repo-fastly.voidlinux.org/current" )
fi

docker_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Use the pinned kernel when nothing was specified
if [ "${#kern_series[@]}" -lt 1 ]; then
  # shellcheck disable=SC1091
  . "${docker_dir}/kernel-pin"
  kern_series=( "linux${KERNEL_SERIES}-${KERNEL_PKGVER}" )
fi

# Populate the corresponding package-name and headers lists; for an exact
# version such as linux7.2-7.2.11_1, the headers are linux7.2-headers-7.2.11_1
kern_names=()
kern_headers=()
kern_header_names=()
for _kern in "${kern_series[@]}"; do
  _name="${_kern%%-[0-9]*}"
  kern_names+=( "${_name}" )
  kern_headers+=( "${_name}-headers${_kern#"${_name}"}" )
  kern_header_names+=( "${_name}-headers" )
done

# A local repository holding the pinned kernel, made by cache-kernel.sh
kern_cache="${docker_dir}/kernel-cache"
kern_cache_mounts=()
if compgen -G "${kern_cache}/*-repodata" >/dev/null; then
  if [ -e "${kern_cache}/SHA256SUMS" ]; then
    if ! ( cd "${kern_cache}" && sha256sum --quiet -c SHA256SUMS ); then
      echo "ERROR: kernel cache in ${kern_cache} failed verification"
      exit 1
    fi
  fi
  echo "INFO: using kernel cache in ${kern_cache}"
  kern_cache_mounts=( --mount "type=bind,src=${kern_cache},dst=/kernel-cache,ro" )
fi

if [ -z "${ZBM_BUILDER}" ]; then
  ZBM_BUILDER="./releng/docker/build-init.sh"
fi

if [ ! -r "${ZBM_BUILDER}" ]; then
  echo "ERROR: cannot find build script at ${ZBM_BUILDER}"
  echo "Run from ZFSBootMenu root or override \$ZBM_BUILDER"
  exit 1
fi

maintainer="ZFSBootMenu Team, https://zfsbootmenu.org"
container="$(buildah from ghcr.io/void-linux/void-glibc-full:latest)"

buildah config --label author="${maintainer}" "${container}"

# Favor custom repositories
for repo in "${host_repos[@]}" "${web_repos[@]}"; do
  buildah run "${container}" sh -c "cat >> /etc/xbps.d/00-custom-repos.conf" <<-EOF
	repository=${repo}
	EOF
done

# Make sure image is up to date
buildah run "${host_mounts[@]}" "${container}" xbps-install -Syu xbps
buildah run "${host_mounts[@]}" "${container}" xbps-install -Syu

# Add extra packages, as desired
if [ "${#extra_pkgs[@]}" -gt 0 ]; then
  echo "INFO: adding extra Void packages: ${extra_pkgs[*]}"
  # shellcheck disable=SC2086
  buildah run "${host_mounts[@]}" "${container}" xbps-install -y "${extra_pkgs[@]}"
fi

# Prefer an LTS version over whatever Void thinks is current
buildah run "${container}" sh -c "cat > /etc/xbps.d/10-nolinux.conf" <<-EOF
	ignorepkg=linux
	ignorepkg=linux-headers
EOF

# Prevent initramfs kernel hooks from being installed
buildah run "${container}" sh -c "cat > /etc/xbps.d/15-noinitramfs.conf" <<-EOF
	noextract=/usr/libexec/mkinitcpio/*
	noextract=/usr/libexec/dracut/*
EOF

# Install ZFSBootMenu dependencies and components necessary to build images
buildah run "${host_mounts[@]}" "${container}" \
  sh -c 'xbps-query -Rp run_depends zfsbootmenu | xargs xbps-install -y'

# The kernel cache is preferred over every other repository, but only while
# it is mounted for the kernel installation
if [ "${#kern_cache_mounts[@]}" -gt 0 ]; then
  buildah run "${container}" sh -c \
    'echo "repository=/kernel-cache" > /etc/xbps.d/00-0-kernel-cache.conf'
fi

# Exact versions vanish from Void mirrors once a newer build is published
for _kern in "${kern_series[@]}"; do
  [[ "${_kern}" == *-[0-9]* ]] || continue
  found="$( buildah run "${host_mounts[@]}" "${kern_cache_mounts[@]}" "${container}" \
    xbps-query -R -p pkgver "${_kern}" 2>/dev/null )" || found=
  if [ "${found}" != "${_kern}" ]; then
    echo "ERROR: ${_kern} is not available from any repository."
    echo "Populate ${kern_cache} with cache-kernel.sh while it is current,"
    echo "or update kernel-pin to a version Void still provides."
    buildah rm "${container}" >/dev/null
    exit 1
  fi
done

buildah run "${host_mounts[@]}" "${kern_cache_mounts[@]}" "${container}" \
  xbps-install -y "${kern_series[@]}" "${kern_headers[@]}" \
  git zstd systemd-boot-efistub curl yq-go bash kbd \
  dracut mkinitcpio dracut-network gptfdisk iproute2 iputils parted \
  curl dosfstools e2fsprogs efibootmgr cryptsetup openssh util-linux kpartx

buildah run "${container}" rm -f /etc/xbps.d/00-0-kernel-cache.conf

# Keep the installed kernels at the requested versions
buildah run "${container}" xbps-pkgdb -m hold "${kern_names[@]}"

# Remove headers and development toolchain, but keep binutils for objcopy
buildah run "${container}" sh -c 'echo "ignorepkg=dkms" > /etc/xbps.d/10-nodkms.conf'
buildah run "${container}" xbps-pkgdb -m manual binutils
buildah run "${container}" xbps-remove -Roy dkms "${kern_header_names[@]}"

# Record a commit hash if one is available
if [ -n "${zbm_commit_hash}" ]; then
  echo "${zbm_commit_hash}" | \
    buildah run "${container}" sh -c 'cat > /etc/zbm-commit-hash'
fi

buildah copy "${container}" "${ZBM_BUILDER}" /build-init.sh
buildah run "${container}" chmod 755 /build-init.sh

buildah config \
  --workingdir / \
  --entrypoint '[ "/build-init.sh" ]' \
  --cmd '[ ]' \
  "${container}"

buildah commit --rm "${container}" "${tag}"
