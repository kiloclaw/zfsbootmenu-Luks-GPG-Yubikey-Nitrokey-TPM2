#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

usage() {
  cat <<-EOF
	USAGE: $0 [OPTIONS] <tag> [zbm-commit-like]
	Create a container image for building ZFSBootMenu

	OPTIONS

	-h: Display this message and exit

	-u <release>:
	   Build on the specified Ubuntu release, which decides the kernel
	   series and the OpenZFS version (26.10 or later)

	   Default: 26.10 (Linux 7.3, OpenZFS 2.4.4)

	-f <flavour>:
	   Install the specified Ubuntu kernel flavour; the flavour must have an
	   Ubuntu ZFS module package

	   Default: generic

	-p <pkg>:
	   Include the specified Ubuntu package in the image
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
ubuntu_release=
kernel_flavour=

while getopts "hu:f:p:" opt; do
  case "${opt}" in
    u)
      ubuntu_release="${OPTARG}"
      ;;
    f)
      kernel_flavour="${OPTARG}"
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

if [ -z "${ZBM_BUILDER}" ]; then
  ZBM_BUILDER="./releng/docker/build-init.sh"
fi

if [ ! -r "${ZBM_BUILDER}" ]; then
  echo "ERROR: cannot find build script at ${ZBM_BUILDER}"
  echo "Run from ZFSBootMenu root or override \$ZBM_BUILDER"
  exit 1
fi

dockerfile="$( dirname "${BASH_SOURCE[0]}" )/Dockerfile"
if [ ! -r "${dockerfile}" ]; then
  echo "ERROR: cannot find ${dockerfile}"
  exit 1
fi

# Build from a private context that holds just the Dockerfile and scripts
context="$( mktemp -d )"
trap 'rm -rf "${context}"' EXIT
cp "${dockerfile}" "${context}/Dockerfile"
cp "${ZBM_BUILDER}" "${context}/build-init.sh"
cp "$( dirname "${dockerfile}" )/setup-image.sh" "${context}/setup-image.sh"

build_args=()
[ -n "${ubuntu_release}" ] && build_args+=( --build-arg "UBUNTU_RELEASE=${ubuntu_release}" )
[ -n "${kernel_flavour}" ] && build_args+=( --build-arg "KERNEL_FLAVOUR=${kernel_flavour}" )
[ -n "${zbm_commit_hash}" ] && build_args+=( --build-arg "ZBM_COMMIT_HASH=${zbm_commit_hash}" )
if [ "${#extra_pkgs[@]}" -gt 0 ]; then
  echo "INFO: adding extra Ubuntu packages: ${extra_pkgs[*]}"
  build_args+=( --build-arg "PACKAGES=${extra_pkgs[*]}" )
fi

buildah build --layers "${build_args[@]}" -t "${tag}" "${context}"
