#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab
#
# Compares the ZFSBootMenu images in two output directories, for example the
# images that the Perl and the Rust generate-zbm build from one configuration.
#
# Usage: compare-images.sh <directory> <directory>
#
# Both directories must hold the same files. Initramfs images, and the .initrd
# section of EFI bundles, are unpacked and compared by their file lists,
# contents, permissions and symlink targets, which do not change between
# builds; their archives and compressed sizes do, through timestamps. Other
# EFI sections and other files are compared byte for byte. The script needs
# objcopy, objdump and dracut's lsinitrd; the build container has them.
#
# Exits with 0 when the images match and 1 when they differ.

set -u

if [ "$#" -ne 2 ] || [ ! -d "$1" ] || [ ! -d "$2" ]; then
  echo "Usage: $0 <directory> <directory>" >&2
  exit 2
fi

left="$( cd "$1" && pwd )"
right="$( cd "$2" && pwd )"
work="$( mktemp -d )" || exit 2
trap 'rm -rf "${work}"' EXIT

status=0
differ() {
  echo "DIFFERS: $*"
  status=1
}

# Lists an unpacked tree: type, permissions, path and link target, and a
# checksum of each regular file
describe_tree() {
  ( cd "$1" && find . -mindepth 1 -printf '%y %#m %p -> %l\n' | LC_ALL=C sort ) || return 1
  ( cd "$1" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 -r sha256sum ) || return 1
}

# arg1: an initramfs image, arg2: where to unpack it
unpack_initramfs() {
  mkdir -p "$2" && ( cd "$2" && lsinitrd --unpack "$1" >/dev/null 2>&1 )
}

compare_initramfs() {
  local name="$1" a="$2" b="$3"
  local dir="${work}/${name//\//_}"
  if ! unpack_initramfs "${a}" "${dir}/a" || ! unpack_initramfs "${b}" "${dir}/b"; then
    differ "${name}: unable to unpack"
    return
  fi
  describe_tree "${dir}/a" > "${dir}/a.list"
  describe_tree "${dir}/b" > "${dir}/b.list"
  if ! diff -u "${dir}/a.list" "${dir}/b.list" > "${dir}/diff"; then
    differ "${name}: contents"
    head -n 40 "${dir}/diff"
  else
    echo "same: ${name} ($( grep -c . "${dir}/a.list" ) entries)"
  fi
}

# The section names of an EFI bundle, in order
efi_sections() {
  objdump -w -h "$1" | awk '$1 ~ /^[0-9]+$/ { print $2 }'
}

compare_efi() {
  local name="$1" a="$2" b="$3" section
  local dir="${work}/${name}"
  mkdir -p "${dir}"
  if [ "$( efi_sections "${a}" )" != "$( efi_sections "${b}" )" ]; then
    differ "${name}: section lists"
    diff <( efi_sections "${a}" ) <( efi_sections "${b}" )
    return
  fi
  for section in $( efi_sections "${a}" ); do
    objcopy --dump-section "${section}=${dir}/a${section}" "${a}" "${dir}/a.tmp" 2>/dev/null
    objcopy --dump-section "${section}=${dir}/b${section}" "${b}" "${dir}/b.tmp" 2>/dev/null
    if [ "${section}" = ".initrd" ]; then
      compare_initramfs "${name}:${section}" "${dir}/a${section}" "${dir}/b${section}"
    elif cmp -s "${dir}/a${section}" "${dir}/b${section}"; then
      echo "same: ${name}:${section}"
    else
      differ "${name}:${section}"
    fi
  done
}

names="$( { ls -A "${left}"; ls -A "${right}"; } | LC_ALL=C sort -u )"
while read -r name; do
  [ -n "${name}" ] || continue
  a="${left}/${name}" b="${right}/${name}"
  if [ ! -e "${a}" ] || [ ! -e "${b}" ]; then
    differ "${name}: only in $( [ -e "${a}" ] && echo "${left}" || echo "${right}" )"
    continue
  fi
  case "${name}" in
    *.EFI|*.efi) compare_efi "${name}" "${a}" "${b}" ;;
    initramfs-*) compare_initramfs "${name}" "${a}" "${b}" ;;
    *)
      if cmp -s "${a}" "${b}"; then
        echo "same: ${name}"
      else
        differ "${name}"
      fi
      ;;
  esac
done <<< "${names}"

if [ "${status}" -eq 0 ]; then
  echo "The images match."
fi
exit "${status}"
