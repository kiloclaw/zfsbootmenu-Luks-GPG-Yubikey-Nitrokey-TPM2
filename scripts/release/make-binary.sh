#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

error() {
  echo "ERROR:" "$@"
  exit 1
}

cleanup() {
  test -d "${buildtmp}" && rm -rf "${buildtmp}"
  unset buildtmp
  exit
}

# arg1: an output directory of the build container, arg2: the build IDs that
# scripts/release/debug-files.sh printed for its images, one per line
# Copies the debug file of each build ID to ${debugdir}; a build ID that an
# earlier build gave a debug file must have the same one
collect_debug_files() {
  local id src dest
  while read -r id; do
    [ -n "${id}" ] || continue
    src="${1}/debug/${id}.debug"
    dest="${debugdir}/${id}.debug"
    if [ ! -e "${dest}" ]; then
      cp "${src}" "${dest}" || error "unable to copy the debug file of build ID ${id}"
    elif ! cmp -s "${src}" "${dest}"; then
      error "two builds gave build ID ${id} different debug files"
    fi
  done <<< "${2}"
}

unset buildtmp
trap cleanup EXIT INT TERM

# Accept release with or without a leading "v"
release="${1#v}"

case "${release}" in
  "") error "usage: $0 <release> [buildtag]" ;;
  */*) error "release must NOT contain a forward slash" ;;
  *) ;;
esac

buildtag="${2:-ghcr.io/zbm-dev/zbm-builder:$(date '+%Y%m%d')}"
if ! podman inspect "${buildtag}" >/dev/null 2>&1; then
  build_args=( "${buildtag}" "${ZBM_COMMIT_HASH:-"v${release}"}" )

  if ! ./packaging/container/image-build.sh "${build_args[@]}"; then
    error "failed to create builder image"
  fi
fi

arch="$( uname -m )"
case "${arch}" in
  x86_64) BUILD_EFI="true" ;;
  *) BUILD_EFI="false" ;;
esac

buildtmp="$( mktemp -d )" || error "cannot create build directory"

# Common volume mounts for the container:
# - Current repo is the tree from which builds will be made
# - A read-only "build" directory (to be made) will contain configuration
volmounts=(
  "-v" ".:/zbm:ro"
  "-v" "${buildtmp}/build:/build:ro"
)

if ! assets="$( realpath -e scripts/release )/assets/${release}"; then
  error "unable to define path to built assets"
fi

if [ -d "${assets}" ]; then
  rm -f "${assets}"/*
else
  mkdir -p "${assets}"
fi

# The debug files of the binaries in every image, by build ID, published as
# one archive of this name
debugname="zfsbootmenu-debug-${arch}-v${release}"
debugdir="${buildtmp}/${debugname}"
mkdir -p "${debugdir}" || error "cannot create the debug file directory"

# Kernel series (X.Y) installed in the build container
kernels="$(podman run --rm --entrypoint /bin/sh "${buildtag}" -c \
  "ls /boot | sed -n 's/^vmlinuz-\([0-9]*\.[0-9]*\)\..*/\1/p' | sort -u")" || kernels=

for style in release recovery; do
  echo "Building style: ${style}"

  # Always start with a fresh configuration tree
  mkdir -p "${buildtmp}/build/dracut.conf.d" || error "cannot create config tree"

  # Copy style-specific configuration components in place;
  # build container sets up standard configuration elements
  cp "./config/zfsbootmenu/${style}.yaml" "${buildtmp}/build/config.yaml"
  cp "./config/zfsbootmenu/${style}.conf.d/"*.conf "${buildtmp}/build/dracut.conf.d"

  # Make sure there is an output directory for this asset style
  zbmtriplet="zfsbootmenu-${style}-${arch}-v${release}"

  for kern in ${kernels}; do
    kern_args=()
    if [ -n "${kern}" ]; then
      echo "Building style ${style} for kernel ${kern}"
      kern_args=( "--" "--kver" "${kern}.*" )
    fi

    outdir="${buildtmp}/${zbmtriplet}"
    mkdir -p "${outdir}" || error "cannot create output directory"

    # In addition to common mounts which expose source repo and build configs,
    # make sure a writable output directory is available for this style and
    # build the EFI bundle if it is supported for this arch
    if ! podman run --rm \
        "${volmounts[@]}" -v "${outdir}:/out" \
        "${buildtag}" -o /out -e ".EFI.Enabled = ${BUILD_EFI}" "${kern_args[@]}"; then
      error "failed to create image"
    fi

    # Every binary of ours in the images must have its debug file, which goes
    # to the debug archive rather than the component tarball
    if ! ids="$( podman run --rm -v ".:/zbm:ro" -v "${outdir}:/out:ro" \
        --entrypoint /bin/bash "${buildtag}" /zbm/scripts/release/debug-files.sh /out )"; then
      error "a binary of the ${style} image has no debug file"
    fi
    collect_debug_files "${outdir}" "${ids}"
    rm -rf "${outdir}/debug"

    if [ "${BUILD_EFI}" = "true" ]; then
      # If EFI file was produced, copy it
      if [ -n "${kern}" ]; then
        efifile="${zbmtriplet}-linux${kern}.EFI"
      else
        efifile="${zbmtriplet}-vmlinuz.EFI"
      fi

      if ! cp "${outdir}/vmlinuz.EFI" "${assets}/${efifile}"; then
        error "failed to copy UEFI bundle"
      fi

      # Remove it so it won't be included in component tarballs
      rm -f "${outdir}/vmlinuz.EFI"
    fi

    have_components=
    for f in "${outdir}"/*; do
      [ -e "${f}" ] || continue
      have_components="yes"
      break
    done

    if [ -n "${have_components}" ]; then
      # If components were produced, archive them
      if [ -n "${kern}" ]; then
        tarball="${zbmtriplet}-linux${kern}.tar.gz"
      else
        tarball="${zbmtriplet}.tar.gz"
      fi

      ( cd "${buildtmp}" && \
        tar -czvf "${assets}/${tarball}" "${zbmtriplet}"
      ) || error "failed to pack components"
    fi

    # Clean up the output directory for the next kernel, if needed
    rm -rf "${outdir}"
  done

  # Clean up the style-specific build components
  rm -rf "${buildtmp}/build"
done

# Pack the debug files once, reproducibly: fixed modes, owners, order, format
# and time, and xz on one thread
compgen -G "${debugdir}/*.debug" >/dev/null || error "no debug files to publish"
epoch="$( git log -1 --format=%ct )" || error "unable to read the time of the last commit"
if ! chmod 0755 "${debugdir}" || ! chmod 0644 "${debugdir}"/*.debug; then
  error "unable to set the modes of the debug files"
fi
( set -o pipefail
  cd "${buildtmp}" && \
  LC_ALL=C tar --format=gnu --sort=name --owner=0 --group=0 --numeric-owner \
    --mtime="@${epoch}" -cf - "${debugname}" | xz -9 -T1 > "${assets}/${debugname}.tar.xz"
) || error "failed to pack the debug files"
