#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

cleanup() {
  if [ -n "${ZBMWORKDIR}" ]; then
    rm -rf "${ZBMWORKDIR}"
  fi

  unset ZBMWORKDIR
}

error() {
  echo ERROR: "$@"
  exit 1
}

# The build container is based on Ubuntu
if ! command -v apt-get >/dev/null 2>&1; then
  error "this build script runs in the Ubuntu-based ZFSBootMenu build container"
fi
export DEBIAN_FRONTEND=noninteractive

# Ubuntu kernel flavour used by -k
: "${ZBM_KERNEL_FLAVOUR:=generic}"

apt_install() {
  apt-get -y install --no-install-recommends "$@"
}

update_apt() {
  [ -n "${APT_UPDATE_COMPLETE}" ] && return 0
  readonly APT_UPDATE_COMPLETE="yes"
  apt-get update || error "failed to update package lists"
}

# Install the newest Ubuntu kernel of a series, with its ZFS module
install_apt_kernel() {
  local series="${1}" abi pkg
  local -a pkgs

  abi="$(
    apt-cache search --names-only \
      "^linux-image-${series//./\\.}\.[0-9]+-[0-9]+-${ZBM_KERNEL_FLAVOUR}\$" \
      | sed -n "s/^linux-image-\([^ ]*\) .*/\1/p" | sort -V | tail -n 1
  )"
  [ -n "${abi}" ] || return 1

  # The ZFS module is in linux-main-modules-zfs-* since Ubuntu 26.04 and in
  # linux-modules-* before
  pkgs=( "linux-image-${abi}" )
  for pkg in "linux-modules-${abi}" "linux-main-modules-zfs-${abi}"; do
    apt-cache show "${pkg}" >/dev/null 2>&1 && pkgs+=( "${pkg}" )
  done

  apt_install --allow-change-held-packages "${pkgs[@]}" || return 1
  modinfo -k "${abi}" zfs >/dev/null 2>&1 || {
    echo "ERROR: kernel ${abi} has no ZFS module"
    return 1
  }
  apt-mark hold "${pkgs[@]}"
}

# Package names of ZFS userspace
apt_zfs_packages() {
  dpkg-query -W -f '${Package}\n' 2>/dev/null \
    | grep -E '^(zfsutils-linux|libzfs|libzpool|libnvpair|libuutil)'
}

update_zfs() {
  local packages

  mapfile -t packages < <( apt_zfs_packages )
  [ "${#packages[@]}" -ge 1 ] || return 0
  apt_install --only-upgrade --allow-change-held-packages "${packages[@]}"
}

usage() {
  cat <<EOF
Usage: $0 [options]
  OPTIONS:
  -h Display help text

  -b <buildroot>
     Specify path for build root
     (Default: /build)

  -o <output-directory>
     Specify path to output directory
     (Default: \${BUILDROOT}/build)

  -p <package>
     Install the named Ubuntu package in the container
     before building. May be specified more than once to
     install more than one package.

  -t <tag>
     Specify specific tag or commit hash to fetch
     (Ignored if /zbm already contains a ZFSBootMenu tree)

  -u
    Update Ubuntu packages in the container
    (Kernel and ZFS packages will be ignored)

  -k <series>
    Install the newest Ubuntu kernel of the specified series
    for which Ubuntu publishes a ZFS module, and build with it
    (the flavour is taken from \$ZBM_KERNEL_FLAVOUR,
    default "generic")

    The <series> argument takes the form "X.Y" for a major
    version X and minor version Y.

  -z
    Update the ZFS packages when installing kernels with -k

  -V
     Do not attempt to update the version recorded in the source tree

  -e <statement>
     Specify a yq-go statement that will be evaluated as

       yq-go -e "<statement>" -i <config>

     to modify the configuration used for image building.
     May be specified more than once to chain modifications.

  -- <arguments>
      Additional arguments to the generate-zbm binary
EOF
}

PACKAGES=()
CONFIGEVALS=()
GENARGS=()

UPDATE=
UPDATE_ZFS=
KERN_SERIES=
SKIP_VERSIONING=

while getopts "hb:o:t:e:p:k:uzV" opt; do
  case "${opt}" in
    b)
      BUILDROOT="${OPTARG}"
      ;;
    o)
      ZBMOUTPUT="${OPTARG}"
      ;;
    t)
      ZBMTAG="${OPTARG}"
      ;;
    p)
      PACKAGES+=( "${OPTARG}" )
      ;;
    e)
      CONFIGEVALS+=( "${OPTARG}" )
      ;;
    u)
      UPDATE="yes"
      ;;
    k)
      KERN_SERIES="${OPTARG}"
      ;;
    z)
      UPDATE_ZFS="yes"
      ;;
    V)
      SKIP_VERSIONING="yes"
      ;;
    h)
      usage
      exit 0
      ;;
    *)
      usage
      exit 1
  esac
done

# Start processing everything after --
shift $((OPTIND-1))
GENARGS+=( "${@}" )

ZBMWORKDIR=""
trap cleanup INT QUIT TERM EXIT
if ! ZBMWORKDIR="$(mktemp -d)"; then
  unset ZBMWORKDIR
  error "unable to allocate working directory"
fi

# Default tag is master unless specified in /etc/zbm-commit-hash
if [ -z "${ZBMTAG}" ]; then
  if [ -r "/etc/zbm-commit-hash" ]; then
    read -r ZBMTAG < /etc/zbm-commit-hash
  else
    ZBMTAG="master"
  fi
fi

# Update packages, if desired; the image holds its kernel and ZFS packages, so
# apt keeps them back
if [ -n "${UPDATE}" ]; then
  update_apt
  apt-get -y upgrade || error "failed to update packages"
fi

if [ "${#PACKAGES[@]}" -gt 0 ]; then
  # Install all requested packages
  update_apt
  apt_install "${PACKAGES[@]}" || error "failed to install requested packages"
fi

if [ -n "${KERN_SERIES}" ]; then
  update_apt

  if [ -n "${UPDATE_ZFS}" ]; then
    update_zfs || error "failed to update ZFS packages"
  fi

  install_apt_kernel "${KERN_SERIES}" \
    || error "failed to install a ${ZBM_KERNEL_FLAVOUR} kernel of series ${KERN_SERIES} with ZFS"

  # Override kernel version to match the requested series by default
  CONFIGEVALS=(
    ".Kernel.Version = \"${KERN_SERIES}.*\""
    "${CONFIGEVALS[@]}"
  )
fi

# If a custom rc.pre.d exists, run every executable file therein
for rfile in "${BUILDROOT}"/rc.pre.d/*; do
  [ -x "${rfile}" ] || continue
  "${rfile}" || error "failed to run RC script ${rfile##*/}"
done

# shellcheck disable=SC2010
if [ ! -d /zbm ] || ! ls -Aq /zbm 2>/dev/null | grep -q . >/dev/null 2>&1; then
  # /zbm is empty or does not exist, attempt to fetch the desired tag
  if ! curl -L -o "${ZBMWORKDIR}/zbm.tar.gz" \
        "https://github.com/zbm-dev/zfsbootmenu/archive/${ZBMTAG}.tar.gz"; then
    error "unable to fetch ZFSBootMenu tag '${ZBMTAG}'"
  fi

  mkdir -p /zbm

  # Ubuntu's tar is GNU tar, so --strip-components is available
  tar -xv --strip-components=1 -f "${ZBMWORKDIR}/zbm.tar.gz" -C /zbm
fi

## Make sure major ZBM components exist at /zbm

[ -x /zbm/bin/generate-zbm ] || error "missing executable /zbm/bin/generate-zbm"

if [ -d /zbm/zfsbootmenu ]; then
  # With new structure, core library must be in /usr/share
  mkdir -p /usr/share
  ln -Tsf /zbm/zfsbootmenu /usr/share/zfsbootmenu \
    || error "unable to link core ZFSBootMenu libraries"

  # dracut module is in "dracut"
  dracutmod=/zbm/dracut
elif [ -d /zbm/90zfsbootmenu ]; then
  # dracut module is in "90zfsbootmenu" and contains everything
  dracutmod=/zbm/90zfsbootmenu
else
  error "/zbm does not appear to be a valid ZFSBootMenu tree"
fi

# Link to dracut module
mkdir -p /usr/lib/dracut/modules.d
ln -Tsf "${dracutmod}" /usr/lib/dracut/modules.d/90zfsbootmenu \
  || error "unable to link dracut module"

# Make sure the build root exists
: "${BUILDROOT:=/build}"
mkdir -p "${BUILDROOT}" || error "unable to create directory '${BUILDROOT}'"

# Make sure that the output directory exists
: "${ZBMOUTPUT:=${BUILDROOT}/build}"
mkdir -p "${ZBMOUTPUT}" || error "unable to create directory '${ZBMOUTPUT}'"

# Add forced overrides to the end of CONFIGEVALS
CONFIGEVALS+=(
  ".Global.ManageImages = true"
  ".Components.ImageDir = \"${ZBMOUTPUT}\""
  ".EFI.ImageDir = \"${ZBMOUTPUT}\""
  "del(.Global.BootMountPoint)"
)

# Use provided hostid
if [ -r "${BUILDROOT}/hostid" ]; then
  ln -Tsf "${BUILDROOT}/hostid" /etc/hostid \
    || error "failed to link hostid"
else
  rm -f /etc/hostid
fi

# Link all configuration files in standard location;
# go from most generic to most specificj
mkdir -p /etc/zfsbootmenu

# Images built in a container must not depend on the container host, whose
# loaded modules have nothing to do with the machines ZFSBootMenu will boot.
# Ubuntu's dracut is host-only by default, which would leave the ZFS and
# dm-crypt modules out. Files linked below may still
# override this.
mkdir -p /etc/zfsbootmenu/dracut.conf.d
echo 'hostonly="no"' > /etc/zfsbootmenu/dracut.conf.d/00-zbm-builder-generic.conf \
  || error "unable to write dracut configuration"
confroots=(
  "/zbm/etc/zfsbootmenu"
  "/zbm/etc/zbm-builder"
  "${BUILDROOT}"
)
for confroot in "${confroots[@]}"; do
  if [ -e "${confroot}/config.yaml" ]; then
    ln -Tsf "${confroot}/config.yaml" "/etc/zfsbootmenu/config.yaml" \
      || error "unable to link config.yaml"
  fi

  for cfile in "${confroot}/dracut.conf.d"/*; do
    [ -e "${cfile}" ] || continue
    ln -Tsf "${cfile}" "/etc/zfsbootmenu/dracut.conf.d/${cfile##*/}" \
      || error "unable to link ${cfile}"
  done
done

# If a custom rc.d exists, run every executable file therein
for rfile in "${BUILDROOT}"/rc.d/*; do
  [ -x "${rfile}" ] || continue
  "${rfile}" || error "failed to run RC script ${rfile##*/}"
done

# Copy default configuration to temporary directory for modifications
ZBMCONF="${ZBMWORKDIR}/config.yaml"
cp "/etc/zfsbootmenu/config.yaml" "${ZBMCONF}" \
  || error "failed to copy configuration to working directory"

GENARGS+=( "--config" "${ZBMCONF}" )

# Apply CONFIGEVALS to override configuration in working directory
for ceval in "${CONFIGEVALS[@]}"; do
  yq-go eval "${ceval}" -i "${ZBMCONF}" \
    || error "failed to apply '${ceval}' to config"
done

# Ubuntu does not package mkinitcpio, so the container builds with dracut only
if [ "$( yq-go eval '.Global.InitCPIO // false' "${ZBMCONF}" )" = "true" ]; then
  error "the build container has no mkinitcpio; remove Global.InitCPIO from the configuration to build with dracut"
fi

# When the configured kernel version matches no installed kernel, use the
# series the image recorded in /etc/zbm-kernel-version
if [ -z "${KERN_SERIES}" ] && [ -r /etc/zbm-kernel-version ]; then
  read -r default_kver < /etc/zbm-kernel-version
  conf_kver="$( yq-go eval '.Kernel.Version // ""' "${ZBMCONF}" )"
  kver_found=
  if [ -n "${conf_kver}" ]; then
    for kprefix in vmlinuz linux vmlinux kernel; do
      if compgen -G "/boot/${kprefix}-${conf_kver}" >/dev/null; then
        kver_found="yes"
        break
      fi
    done
  fi

  if [ -n "${default_kver}" ] && [ -n "${conf_kver}" ] && [ -z "${kver_found}" ]; then
    echo "INFO: no kernel matches Kernel.Version ${conf_kver}; using ${default_kver}"
    yq-go eval ".Kernel.Version = \"${default_kver}\"" -i "${ZBMCONF}" \
      || error "failed to set kernel version ${default_kver}"
  fi
fi

# If possible, update the version recorded in the ZFSBootMenu repository
if [ "${SKIP_VERSIONING,,}" != "yes" ] && [ -x /zbm/releng/version.sh ]; then
  ( cd /zbm && ./releng/version.sh -u ) || true
fi

exec /zbm/bin/generate-zbm "${GENARGS[@]}"
