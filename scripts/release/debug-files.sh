#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab
#
# Checks that every binary of ours in the ZFSBootMenu images of a build
# directory has its debug file, and prints the build IDs of those files.
#
# Usage: debug-files.sh <build directory>
#
# The images are initramfs-bootmenu.img and the .initrd section of
# vmlinuz.EFI; at least one must be there. Each image must carry
# /libexec/zbm, and Rust-runtime images carry /usr/bin/brush too. The build ID
# of each binary must name <build directory>/debug/<build ID>.debug, as
# packaging/container/build-init.sh leaves it. A debug file must name no path
# under /tmp/, /home/ or /root/, which would be the build host's: the sources
# are at /zbm and the build's own directories are remapped.
#
# The build IDs are printed one per line, each once, when every binary has its
# debug file. The script needs objcopy, readelf and dracut's lsinitrd; the build
# container has them.
#
# Exits with 0 when every binary has its debug file, with 1 when one has none
# or a debug file fails its check, and with 2 on wrong use or when it cannot
# make its working directory.

set -u

if [ "$#" -ne 1 ] || [ ! -d "$1" ]; then
  echo "Usage: $0 <build directory>" >&2
  exit 2
fi

build="$( cd "$1" && pwd )"
work="$( mktemp -d )" || exit 2
trap 'rm -rf "${work}"' EXIT

status=0
fail() {
  echo "ERROR: $*" >&2
  status=1
}

# The GNU build ID of an ELF file, in lowercase hexadecimal
build_id() {
  readelf -n "$1" 2>/dev/null \
    | sed -n 's/^[[:space:]]*Build ID:[[:space:]]*\([0-9a-f][0-9a-f]*\)[[:space:]]*$/\1/p' \
    | tail -n 1
}

# Paths of the build host that a debug file names, at most five. Fails when
# readelf finds no compilation directory in the file or exits with an error,
# so that a file it cannot read never passes the check with no paths found
host_paths() {
  readelf --debug-dump=info "$1" 2>/dev/null | grep -q 'DW_AT_comp_dir' \
    || return 1
  readelf --debug-dump=info,line "$1" 2>/dev/null \
    | grep -oE '(^|[^[:alnum:]_.~/-])/(tmp|home|root)/[^[:space:]]*' \
    | sed 's|^[^/]||' | LC_ALL=C sort -u | head -n 5
  [ "${PIPESTATUS[0]}" -eq 0 ]
}

images=()
names=()
if [ -f "${build}/initramfs-bootmenu.img" ]; then
  images+=( "${build}/initramfs-bootmenu.img" )
  names+=( "initramfs-bootmenu.img" )
fi
if [ -f "${build}/vmlinuz.EFI" ]; then
  if objcopy --dump-section ".initrd=${work}/efi.initrd" \
      "${build}/vmlinuz.EFI" "${work}/efi.tmp" 2>/dev/null \
      && [ -s "${work}/efi.initrd" ]; then
    images+=( "${work}/efi.initrd" )
    names+=( "vmlinuz.EFI" )
  else
    fail "vmlinuz.EFI: unable to read its .initrd section"
  fi
fi
if [ "${#images[@]}" -eq 0 ] && [ "${status}" -eq 0 ]; then
  fail "${build} holds no initramfs-bootmenu.img and no vmlinuz.EFI"
fi

# The build IDs found, each once
declare -A ids=()

for i in "${!images[@]}"; do
  for binary in libexec/zbm usr/bin/brush; do
    what="${names[i]}:/${binary}"
    out="${work}/${i}-${binary##*/}"
    # lsinitrd prints nothing for a file that the image does not carry
    lsinitrd -f "${binary}" "${images[i]}" > "${out}" 2>/dev/null
    if [ ! -s "${out}" ]; then
      [ "${binary}" = libexec/zbm ] && fail "${what}: missing from the image"
      continue
    fi

    id="$( build_id "${out}" )"
    if [ -z "${id}" ]; then
      fail "${what}: no GNU build ID"
      continue
    fi
    if [ ! -f "${build}/debug/${id}.debug" ]; then
      fail "${what}: no debug file debug/${id}.debug"
      continue
    fi
    ids["${id}"]=1
  done
done

for id in "${!ids[@]}"; do
  if ! leaks="$( host_paths "${build}/debug/${id}.debug" )"; then
    fail "debug/${id}.debug: readelf cannot read its debug information"
  elif [ -n "${leaks}" ]; then
    fail "debug/${id}.debug names paths of the build host: ${leaks//$'\n'/ }"
  fi
done

if [ "${status}" -eq 0 ]; then
  printf '%s\n' "${!ids[@]}" | LC_ALL=C sort
fi
exit "${status}"
