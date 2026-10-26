#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

# shellcheck disable=SC2034
zfsbootmenu_essential_binaries=(
  "bash"
  "zfs"
  "zpool"
  "zdb"
  "lsblk"
  "hostid"
  "mount"
  "mount.zfs"
  "kexec"
  "mkdir"
  "tput"
  "head"
  "mktemp"
  "sort"
  "sed"
  "grep"
  "tail"
  "tac"
  "blkid"
  "awk"
  "ps"
  "env"
  "chmod"
  "chroot"
  "od"
  "stty"
  "insmod"
  "modinfo"
  "lsmod"
  "depmod"
  "dmesg"
  "fzf"
  "setsid"
  "cat"
)

# shellcheck disable=SC2034
zfsbootmenu_optional_binaries=(
  "mbuffer"
  "column"
  "less"
)

# shellcheck disable=SC2034
zfsbootmenu_udev_rules=(
  "90-zfs.rules"
  "69-vdev.rules"
  "60-zvol.rules"
)

# Device-mapper udev rules let cryptsetup synchronize with udev and create
# /dev/disk/by-* links for unlocked LUKS volumes; names vary by distribution
# shellcheck disable=SC2034
zfsbootmenu_luks_udev_rules=(
  "10-dm.rules"
  "13-dm-disk.rules"
  "55-dm.rules"
  "60-persistent-storage-dm.rules"
  "95-dm-notify.rules"
)

# Programs needed to unlock LUKS volumes that hold ZFS vdevs
# shellcheck disable=SC2034
zfsbootmenu_luks_binaries=(
  "cryptsetup"
  "dmsetup"
  "udevadm"
  "base64"
  "cut"
  "tr"
  "wc"
  "sha256sum"
)

# TPM and USB/HID drivers for TPM2 chips, FIDO2 security keys and OpenPGP
# smart cards; missing or built-in modules are skipped
# shellcheck disable=SC2034
zfsbootmenu_luks_token_modules=(
  "tpm"
  "tpm_tis_core"
  "tpm_tis"
  "tpm_crb"
  "xhci_pci"
  "xhci_hcd"
  "ehci_pci"
  "ehci_hcd"
  "ohci_pci"
  "ohci_hcd"
  "uhci_hcd"
  "usbhid"
  "hid_generic"
  "hid"
)

# Libraries that the systemd token plugins and tpm2-tools load with dlopen()
# shellcheck disable=SC2034
zfsbootmenu_luks_token_libs=(
  "libtss2-esys.so.0"
  "libtss2-rc.so.0"
  "libtss2-mu.so.0"
  "libtss2-tctildr.so.0"
  "libtss2-tcti-device.so.0"
  "libfido2.so.1"
)

# Native unlocking of systemd-tpm2 tokens
# shellcheck disable=SC2034
zfsbootmenu_luks_tpm2_binaries=(
  "tpm2_load"
  "tpm2_startauthsession"
  "tpm2_policypcr"
  "tpm2_policyauthvalue"
  "tpm2_unseal"
  "tpm2_flushcontext"
  "tpm2_getcap"
  "openssl"
)

# Native unlocking of systemd-fido2 tokens
# shellcheck disable=SC2034
zfsbootmenu_luks_fido2_binaries=(
  "fido2-token"
  "fido2-assert"
)

# clevis LUKS2 tokens (TPM2, tang and sss policies)
# shellcheck disable=SC2034
zfsbootmenu_luks_clevis_binaries=(
  "jose"
  "luksmeta"
  "curl"
  "tpm2_createprimary"
  "tpm2_load"
  "tpm2_unseal"
  "tpm2_flushcontext"
  "tpm2_pcrread"
  "tpm2_getcap"
  "tpm2_startauthsession"
  "tpm2_policypcr"
  "tpm2_createpolicy"
)

# YubiKey HMAC-SHA1 challenge-response
# shellcheck disable=SC2034
zfsbootmenu_luks_yubikey_binaries=(
  "ykchalresp"
  "ykinfo"
)

# dm-crypt and the kernel crypto modules commonly used by LUKS; any that are
# missing (or built into the kernel) are silently skipped
# shellcheck disable=SC2034
zfsbootmenu_luks_modules=(
  "dm_mod"
  "dm_crypt"
  "dm_integrity"
  "af_alg"
  "algif_skcipher"
  "algif_hash"
  "cryptd"
  "crypto_simd"
  "gf128mul"
  "aes_generic"
  "aesni_intel"
  "aes_ce_blk"
  "aes_ce_cipher"
  "aes_neon_blk"
  "aes_neon_bs"
  "xts"
  "cbc"
  "ecb"
  "ctr"
  "lrw"
  "essiv"
  "sha256_generic"
  "sha256_ssse3"
  "sha512_generic"
  "sha512_ssse3"
  "sha2_ce"
  "sha256_arm64"
  "sha512_arm64"
  "crc32c_generic"
  "crc32c_cryptoapi"
  "crc32c_intel"
  "serpent_generic"
  "twofish_generic"
)

# shellcheck disable=SC2034
zfsbootmenu_essential_modules=(
  "zfs"
  "zcommon"
  "znvpair"
  "zavl"
  "zunicode"
  "zlua"
  "icp"
  "spl"
)

# shellcheck disable=SC2034
zfsbootmenu_optional_modules=(
  "zlib_deflate"
  "zlib_inflate"
  "fat"
  "vfat"
  "nls_iso8859_1"
  "nls_cp437"
)

create_zbm_conf() {
  # Create core ZBM configuration file

  # Check if fuzzy finder supports the refresh-preview flag
  # Added in fzf 0.22.0
  local has_refresh
  if echo "abc" | fzf -f "abc" --bind "alt-l:refresh-preview" --exit-0 >/dev/null 2>&1; then
    has_refresh=1
  fi

  # Check if fuzzy finder supports the disabled flag
  # Added in fzf 0.25.0
  local has_disabled
  if echo "abc" | fzf -f "abc" --disabled --exit-0 >/dev/null 2>&1; then
    has_disabled=1
  fi

  # Check if fuzzy finder supports border flags
  # Added in fzf 0.35.0
  local has_border
  if echo "abc" | fzf -f "abc" --border-label=test --exit-0 >/dev/null 2>&1; then
    has_border=1
  fi

  # Check if fuzzy finder supports optional --color arguments
  # Added in fzf 0.52.0
  local has_colors
  if echo "abc" | fzf -f "abc" --color='current-fg:red,selected-fg:magenta' --exit-0 >/dev/null 2>&1; then
    has_colors=1
  fi

  # Check if fuzzy finder supports --raw flag
  # Added in fzf 0.66.0
  local has_raw
  if echo "abc" | fzf -f "abc" --raw --exit-0 >/dev/null 2>&1; then
    has_raw=1
  fi

  local has_column
  if command -v column >/dev/null 2>&1 ; then
    has_column=1
  fi

  case "${zfsbootmenu_release_build,,}" in
    yes|y|on|1)
      zfsbootmenu_release_build=1
      ;;
    *)
      zfsbootmenu_release_build=
      ;;
  esac

  # Normalize ZBM_BUILDSTYLE, if set
  case "${ZBM_BUILDSTYLE,,}" in
    mkinitcpio) ZBM_BUILDSTYLE="mkinitcpio" ;;
    dracut) ZBM_BUILDSTYLE="dracut" ;;
    *) ZBM_BUILDSTYLE="" ;;
  esac

  cat > "${BUILDROOT}/etc/zfsbootmenu.conf" <<-'EOF'
	# Include guard
	[ -n "${_ETC_ZFSBOOTMENU_CONF}" ] && return
	readonly _ETC_ZFSBOOTMENU_CONF=1
	EOF

  # shellcheck disable=SC2154
  cat >> "${BUILDROOT}/etc/zfsbootmenu.conf" <<-EOF
	export HAS_REFRESH="${has_refresh}"
	export HAS_DISABLED="${has_disabled}"
	export HAS_BORDER="${has_border}"
	export HAS_COLUMN="${has_column}"
	export HAS_COLORS="${has_colors}"
	export HAS_RAW="${has_raw}"
	export ZBM_BUILDSTYLE="${ZBM_BUILDSTYLE}"
	export ZBM_RELEASE_BUILD="${zfsbootmenu_release_build}"
	export ZBM_RUNTIME="${ZBM_RUNTIME:-legacy}"
	EOF
}

create_zbm_profiles() {
  # Create shell profiles for ZBM

  cat > "${BUILDROOT}/etc/profile" <<-EOF
	export PATH=/zbm/bin:/usr/sbin:/usr/bin:/sbin:/bin
	export TERM=linux
	export HOME=/root
	EOF

  mkdir -p "${BUILDROOT}/root"
  cat > "${BUILDROOT}/root/.bashrc" <<-EOF
	source /etc/zfsbootmenu.conf >/dev/null 2>&1
	source /lib/kmsg-log-lib.sh >/dev/null 2>&1
	source /lib/zfsbootmenu-core.sh >/dev/null 2>&1
	source /lib/zfsbootmenu-kcl.sh >/dev/null 2>&1
	[ -f /etc/profile ] && source /etc/profile
	[ -f /lib/zfsbootmenu-completions.sh ] && source /lib/zfsbootmenu-completions.sh

	export PROMPT_COMMAND="/libexec/recovery-error-printer"
	export PS1='\[\033[0;33m\]zfsbootmenu\[\033[0m\] \w > '

	alias clear="tput clear"
	alias reset="tput reset"
	alias trace="ztrace"
	alias help="/libexec/zfsbootmenu-help -L recovery-shell"

	zdebug "sourced /root/.bashrc" || true
	EOF

  # symlink to .profile for /bin/sh - launched by dropbear
  ln -s "/root/.bashrc" "${BUILDROOT}/root/.profile"
}

zbm_install_symlink() {
  [ -L "${1}" ] || return 1

  # shellcheck disable=SC2154
  case "${1}" in
    "${zfsbootmenu_module_root}"/*) ;;
    *) return 1 ;;
  esac

  local dest src

  dest="$(readlink "${1}")" || dest=
  [ -n "${dest}" ] || return 1

  case "${dest}" in
    /*) return 1 ;;
  esac

  src="${2:-"${1}"}"

  # Attempt to install the target of the symlink
  local rpath
  if rpath="$(readlink -f "${1}")"; then
    if [ -e "${rpath}" ] && [ ! -L "${rpath}" ]; then
      zbm_install_file "${rpath}" "$(dirname "${src}")/${dest}" || return 1
    fi
  fi

  ln -Ts "${dest}" "${BUILDROOT}/${src}" || return 1
  return 0
}

zbm_install_file() {
  # All symlinks in the ZBM module root should be relative
  zbm_install_symlink "$@" && return 0

  case "${ZBM_BUILDSTYLE,,}" in
    mkinitcpio)
      if ! add_file "${1}" "${2}"; then
        error "failed to install file '${1}'"
        return 1
      fi
      ;;
    dracut)
      if ! inst_simple "${1}" "${2}"; then
        dfatal "failed to install file '${1}'"
        return 1
      fi
      ;;
    *)
      echo "ERROR: unrecognized build style; unable to install files" >&2
      return 1
      ;;
  esac
}

create_zbm_traceconf() {
  local zbm_prof_lib

  # Enable performance profiling if configured and available
  # shellcheck disable=SC2154
  case "${zfsbootmenu_trace_enable,,}" in
    yes|y|on|1)
      zbm_prof_lib="${zfsbootmenu_module_root}/profiling/profiling-lib.sh"
      ;;
    *)
      ;;
  esac

  if ! [ -r "${zbm_prof_lib}" ]; then
    echo "return 0" > "${BUILDROOT}/lib/profiling-lib.sh"
    return
  fi

  zbm_install_file "${zbm_prof_lib}" "/lib/profiling-lib.sh"

  # shellcheck disable=SC2154
  cat > "${BUILDROOT}/etc/profiling.conf" <<-EOF
	export zfsbootmenu_trace_term=${zfsbootmenu_trace_term}
	export zfsbootmenu_trace_baud=${zfsbootmenu_trace_baud}
	EOF
}


install_zbm_core() {
  local cdir cfile ret

  ret=0
  for cdir in lib libexec bin; do
    for cfile in "${zfsbootmenu_module_root}/${cdir}"/*; do
      # The Rust runtime starts with zbm, and zbm is zluks and the menu; the
      # shell libraries and the other tools stay, for hooks and the recovery
      # shell
      if [ "${ZBM_RUNTIME}" = "rust" ]; then
        case "${cdir}/${cfile##*/}" in
          libexec/zfsbootmenu-init|bin/zluks|bin/zfsbootmenu|bin/zbm) continue ;;
        esac
      fi
      zbm_install_file "${cfile}" "/${cdir}/${cfile##*/}" || ret=$?
    done
  done

  if [ "${ZBM_RUNTIME}" = "rust" ]; then
    ln -sf /libexec/zbm "${BUILDROOT}/bin/zluks" || ret=$?
    ln -sf /libexec/zbm "${BUILDROOT}/bin/zfsbootmenu" || ret=$?
    # zbm is the menu's other name; with a command (zbm debug crash, zbm
    # report) it runs that command of /libexec/zbm, which lists them
    cat > "${BUILDROOT}/bin/zbm" <<'EOF' || ret=$?
#!/bin/sh
case "${1:-}" in
  ""|-*) exec /libexec/zbm zfsbootmenu "$@" ;;
  *) exec /libexec/zbm "$@" ;;
esac
EOF
    chmod 755 "${BUILDROOT}/bin/zbm" || ret=$?
    return $ret
  fi

  for cfile in "${zfsbootmenu_module_root}/init.d"/*; do
    zbm_install_file "${cfile}" "/libexec/init.d/${cfile##*/}" || ret=$?
  done

  return $ret
}


install_zbm_docs() {
  local doc relative ret

  ret=0
  while read -r doc; do
    relative="${doc#"${zfsbootmenu_module_root}"}"
    [ "${relative}" = "${doc}" ] && continue
    relative="${relative#/}"
    zbm_install_file "${doc}" "/usr/share/docs/${relative}" || ret=$?
  done <<< "$( find "${zfsbootmenu_module_root}/help-files" -type f )"

  return $ret
}

install_zbm_fonts() {
  local font fonts subsets family

  fonts="${zfsbootmenu_module_root}/fonts"
  [ -d "${fonts}" ] || return 1

  # zfsbootmenu_console_font picks the family: terminus, the default, or
  # spleen, which only the Rust runtime loads (ROADMAP.md #20)
  # shellcheck disable=SC2154
  family="${zfsbootmenu_console_font:-terminus}"
  case "${family,,}" in
    terminus) ;;
    spleen)
      if [ "${ZBM_RUNTIME}" != "rust" ]; then
        zbm_warn "Spleen console fonts need zfsbootmenu_runtime=rust; installing Terminus"
      elif [ -d "${fonts}/spleen" ]; then
        fonts="${fonts}/spleen"
        [ -f "${fonts}/LICENSE" ] \
          && zbm_install_file "${fonts}/LICENSE" "/usr/share/zfsbootmenu/fonts/LICENSE.spleen"
      else
        zbm_warn "no Spleen console fonts in ${fonts}; installing Terminus"
      fi
      ;;
    *)
      zbm_warn "unknown zfsbootmenu_console_font '${family}'; installing Terminus"
      ;;
  esac

  # Images with the Rust runtime get each font as a 256-glyph subset, which
  # leaves the console all 16 colours (ROADMAP.md #20); the whole fonts when
  # zbm cannot make them
  # shellcheck disable=SC2154
  if [ "${ZBM_RUNTIME}" = "rust" ] && subsets="$( mktemp -d )"; then
    if "${zfsbootmenu_zbm:-/usr/libexec/zfsbootmenu/zbm}" console-fonts "${fonts}" "${subsets}" >&2 \
      && compgen -G "${subsets}/*.psf" >/dev/null; then
      fonts="${subsets}"
    else
      zbm_warn "unable to make 256-glyph console fonts; installing the whole fonts"
    fi
  fi

  # Install into a non-standard path so that the Dracut i18n module doesn't stomp over our custom fonts

  for font in "${fonts}"/*.psf; do
    [ -f "${font}" ] && zbm_install_file "${font}" "/usr/share/zfsbootmenu/fonts/${font##*/}"
  done

  # The console on the display of the Rust runtime draws with a TrueType
  # font (zbm.console=drm, ROADMAP.md #19)
  if [ "${ZBM_RUNTIME}" = "rust" ] && [ -f "${zfsbootmenu_module_root}/fonts/dejavu/DejaVuSansMono.ttf" ]; then
    zbm_install_file "${zfsbootmenu_module_root}/fonts/dejavu/DejaVuSansMono.ttf" \
      "/usr/share/zfsbootmenu/fonts/DejaVuSansMono.ttf"
    zbm_install_file "${zfsbootmenu_module_root}/fonts/dejavu/LICENSE" \
      "/usr/share/zfsbootmenu/fonts/LICENSE.dejavu"
  fi

  [ -n "${subsets}" ] && rm -rf "${subsets}"
  return 0
}

# Images with the Rust runtime get the keyboard layouts of
# zfsbootmenu_keyboard_layouts for the console on the display, compiled by
# zbm keyboard-layouts from the build host's XKB data (ROADMAP.md #19). The
# first is the console's own; without the option, the layout of the build
# host's /etc/vconsole.conf (XKBLAYOUT) comes before us, de, fr and gb.
#
# prints: nothing
# returns: 0

install_zbm_keyboard_layouts() {
  local layouts host compiled file

  [ "${ZBM_RUNTIME}" = "rust" ] || return 0

  # shellcheck disable=SC2154
  layouts="${zfsbootmenu_keyboard_layouts}"
  if [ -z "${layouts}" ]; then
    host="$( sed -n 's/^XKBLAYOUT="\{0,1\}\([^",]*\).*/\1/p' /etc/vconsole.conf 2>/dev/null )"
    layouts="${host} us de fr gb"
  fi

  compiled="$( mktemp -d )" || return 0
  # shellcheck disable=SC2086
  if ! "${zfsbootmenu_zbm:-/usr/libexec/zfsbootmenu/zbm}" keyboard-layouts \
    "${compiled}" ${layouts//,/ } >&2; then
    zbm_warn "unable to compile every keyboard layout of '${layouts}'"
  fi

  for file in "${compiled}"/*; do
    [ -f "${file}" ] && zbm_install_file "${file}" "/usr/share/zfsbootmenu/layouts/${file##*/}"
  done

  rm -rf "${compiled}"
  return 0
}

# Images with the Rust runtime let the recovery shell's line editor take
# characters beyond ASCII, which the console on the display types with its
# keyboard layouts (ROADMAP.md #19): readline keeps their bytes (/etc/inputrc),
# and the C.UTF-8 character set of the build host's C library, when it has
# one, makes them characters. Runs after create_zbm_profiles.
#
# prints: nothing
# returns: 0

install_zbm_shell_input() {
  local ctype

  [ "${ZBM_RUNTIME}" = "rust" ] || return 0

  cat > "${BUILDROOT}/etc/inputrc" <<-EOF
	set input-meta on
	set output-meta on
	set convert-meta off
	EOF

  for ctype in /usr/lib/locale/C.utf8/LC_CTYPE /usr/lib/locale/C.UTF-8/LC_CTYPE; do
    [ -f "${ctype}" ] || continue
    if zbm_install_file "${ctype}" "/usr/lib/locale/C.utf8/LC_CTYPE"; then
      echo "export LC_CTYPE=C.UTF-8" >> "${BUILDROOT}/etc/profile"
    fi
    break
  done

  return 0
}

# The omit_drivers regular expression of dracut without the drivers named:
# dracut leaves out the modules it matches, and the dependencies of others too
# (drm, for every graphics driver). Newer dracut anchors each name (^drm$),
# older dracut does not.
#
# arg1: the regular expression
# argN: the drivers
# prints: the regular expression without them

zbm_unomit_drivers() {
  local regex="|${1}|" name
  shift
  for name in "$@"; do
    regex="${regex//"|^${name}\$|"/|}"
    regex="${regex//"|${name}|"/|}"
  done
  regex="${regex#|}"
  printf '%s' "${regex%|}"
}

# Graphics drivers and the firmware of the GPUs the image is for (ROADMAP.md
# #17), from zfsbootmenu_graphics_drivers and zfsbootmenu_graphics_firmware,
# which generate-zbm sets from Kernel.GraphicsDrivers. The drivers are
# installed with every firmware file they list; then the files of their
# firmware directories that zfsbootmenu_graphics_firmware does not name are
# removed, keeping the targets of the links it names. ZFSBootMenu's dracut
# configuration omits the graphics drivers and drm (omit_drivers), so they
# are installed with that list cut for this call only. dracut runs this from
# installkernel, where nothing installs the drivers later and puts the
# removed files back. Only the Rust runtime takes graphics drivers.
#
# prints: nothing
# returns: 0, or 1 if the drivers cannot be installed or none of the listed
#          firmware is in the image

install_zbm_graphics() {
  local driver dir root file rel name target kept=0 removed=0 found
  local -a dirs=() roots=() missing=()
  # shellcheck disable=SC2154
  local omit_drivers="${omit_drivers}"
  local -A keep=() listed=()

  # shellcheck disable=SC2154
  [ -n "${zfsbootmenu_graphics_drivers}" ] || return 0

  if [ "${ZBM_RUNTIME}" != "rust" ]; then
    zbm_error "graphics drivers (zfsbootmenu_graphics_drivers) need zfsbootmenu_runtime=rust"
    return 1
  fi

  for driver in ${zfsbootmenu_graphics_drivers}; do
    case "${driver}" in
      amdgpu) dirs+=( amdgpu ) ;;
      i915) dirs+=( i915 ) ;;
      xe) dirs+=( xe i915 ) ;;
      *)
        zbm_error "unknown graphics driver '${driver}'"
        return 1
        ;;
    esac
  done

  # shellcheck disable=SC2086
  omit_drivers="$( zbm_unomit_drivers "${omit_drivers}" ${zfsbootmenu_graphics_drivers} drm )"
  for driver in ${zfsbootmenu_graphics_drivers}; do
    case "${ZBM_BUILDSTYLE,,}" in
      mkinitcpio) add_module "${driver}" ;;
      dracut) hostonly='' instmods -c "${driver}" ;;
      *) false ;;
    esac || {
      zbm_error "unable to install the graphics driver '${driver}'"
      return 1
    }
  done

  # shellcheck disable=SC2154
  [ -n "${zfsbootmenu_graphics_firmware}" ] || return 0
  for name in ${zfsbootmenu_graphics_firmware}; do
    listed["${name}"]=1
  done

  # The firmware trees of the image; /lib may be a link to /usr/lib
  for root in "${BUILDROOT}/usr/lib/firmware" "${BUILDROOT}/lib/firmware"; do
    [ -d "${root}" ] || continue
    [ "${#roots[@]}" -gt 0 ] && [ "${root}" -ef "${roots[0]}" ] && continue
    roots+=( "${root}" )
  done

  # Keep the listed files, compressed or not, and what their links point to
  for root in "${roots[@]}"; do
    for dir in "${dirs[@]}"; do
      [ -d "${root}/${dir}" ] || continue
      while IFS= read -r -d '' file; do
        rel="${file#"${root}"/}"
        name="${rel%.zst}"
        name="${name%.xz}"
        [ -n "${listed["${name}"]}" ] || continue
        keep["${file}"]=1
        listed["${name}"]=found
        [ -L "${file}" ] || continue
        target="$( readlink -f "${file}" )" || continue
        case "${target}" in
          "${BUILDROOT}"/*) ;;
          *) target="${BUILDROOT}${target}" ;;
        esac
        keep["${target}"]=1
      done < <( find "${root}/${dir}" \( -type f -o -type l \) -print0 )
    done
  done

  for root in "${roots[@]}"; do
    for dir in "${dirs[@]}"; do
      [ -d "${root}/${dir}" ] || continue
      while IFS= read -r -d '' file; do
        if [ -n "${keep["${file}"]}" ]; then
          kept=$(( kept + 1 ))
        elif rm -f "${file}"; then
          removed=$(( removed + 1 ))
        fi
      done < <( find "${root}/${dir}" \( -type f -o -type l \) -print0 )
    done
  done

  found=0
  for name in "${!listed[@]}"; do
    if [ "${listed["${name}"]}" = found ]; then
      found=$(( found + 1 ))
    else
      missing+=( "${name}" )
    fi
  done

  if [ "${found}" -eq 0 ]; then
    zbm_error "none of the graphics firmware is in the image; is linux-firmware installed?"
    return 1
  fi
  if [ "${#missing[@]}" -gt 0 ]; then
    zbm_warn "graphics firmware not found, left out: $( printf '%s\n' "${missing[@]}" | sort | tr '\n' ' ' )"
  fi
  zbm_info "graphics drivers ${zfsbootmenu_graphics_drivers}: kept ${kept} firmware files, removed ${removed}"
  return 0
}

install_zbm_osver() {
  local build_date
  [ -r "${zfsbootmenu_module_root}/zbm-release" ] || return 0
  zbm_install_file "${zfsbootmenu_module_root}/zbm-release" "/etc/zbm-release"
  # The day of SOURCE_DATE_EPOCH, when set, so that reproducible builds give
  # the same file (ROADMAP.md #5)
  if [ -n "${SOURCE_DATE_EPOCH}" ]; then
    build_date="$(date -u -d "@${SOURCE_DATE_EPOCH}" '+%Y-%m-%d')" || build_date=""
  else
    build_date="$(date '+%Y-%m-%d')" || build_date=""
  fi
  if [ -n "${build_date}" ]; then
    cat >> "${BUILDROOT}/etc/zbm-release" <<-EOF
	BUILD_ID="${build_date}"
	EOF
  fi

  ln -Tsf zbm-release "${BUILDROOT}/etc/os-release"
}

populate_hook_dir() {
  local hfile ret hlev

  hlev="${1}"
  if [ -z "${hlev}" ]; then
    echo "ERROR: a hook level is required" >&2
    return 1
  fi

  shift
  [ "$#" -gt 0 ] || return 0

  mkdir -p "${BUILDROOT}/libexec/hooks/${hlev}" || return 1

  ret=0
  for hfile in "$@"; do
    [ -x "${hfile}" ] || continue
    zbm_install_file "${hfile}" "/libexec/hooks/${hlev}/${hfile##*/}" || ret=$?
  done

  return $ret
}


install_zbm_hooks() {
  local hdir hsrc hfile ret stages

  ret=0

  stages=( early-setup.d setup.d load-key.d boot-sel.d teardown.d )

  # Install system hooks first so user options can override them
  for hdir in "${stages[@]}"; do
    hsrc="${zfsbootmenu_module_root}/hooks/${hdir}"
    [ -d "${hsrc}" ] || continue
    populate_hook_dir "${hdir}" "${hsrc}"/* || ret=$?
  done

  # The Rust runtime closes LUKS mappings with its own zluks
  if [ "${ZBM_RUNTIME}" = "rust" ] && [ -e "${BUILDROOT}/libexec/hooks/teardown.d/80-luks-close.sh" ]; then
    cat > "${BUILDROOT}/libexec/hooks/teardown.d/80-luks-close.sh" <<-'EOF'
	#!/bin/bash
	## Close the LUKS mappings that ZFSBootMenu opened to import ZFS pools, with
	## zluks: the pools on them are exported first, and closing them makes the
	## kernel wipe the dm-crypt volume keys before the new kernel starts.
	##
	## This hook may be disabled with zbm.skip_hooks=80-luks-close.sh

	command -v cryptsetup >/dev/null 2>&1 || exit 0
	/bin/zluks close
	exit 0
	EOF
    chmod 755 "${BUILDROOT}/libexec/hooks/teardown.d/80-luks-close.sh" || ret=$?
  fi

  # The Rust runtime sizes the console font with zbm, which knows the display's
  # pixel density (HiDPI)
  if [ "${ZBM_RUNTIME}" = "rust" ] && [ -e "${BUILDROOT}/libexec/hooks/early-setup.d/30-console-autosize.sh" ]; then
    cat > "${BUILDROOT}/libexec/hooks/early-setup.d/30-console-autosize.sh" <<-'EOF'
	#!/bin/bash
	## Size the console font to the display (zbm.autosize), with zbm: from the
	## display's physical size where its EDID gives it, so that text is readable
	## on high-density (HiDPI) and ordinary screens alike.
	##
	## This hook may be disabled with zbm.skip_hooks=30-console-autosize.sh

	exec /libexec/zbm console-autosize
	EOF
    chmod 755 "${BUILDROOT}/libexec/hooks/early-setup.d/30-console-autosize.sh" || ret=$?
  fi

  # Shadow mode: zbm compares the legacy runtime's decisions with the Rust
  # code's just before each boot (docs/dev/shadow-mode.md)
  # shellcheck disable=SC2154
  if zbm_opt_enabled "${zfsbootmenu_shadow}" no; then
    if [ "${ZBM_RUNTIME}" != "legacy" ]; then
      zbm_warn "zfsbootmenu_shadow: only legacy runtime images have a shadow mode"
    elif [ ! -x "${BUILDROOT}/libexec/zbm" ]; then
      zbm_error "zfsbootmenu_shadow: shadow mode needs zbm, set with zfsbootmenu_zbm"
      ret=1
    else
      mkdir -p "${BUILDROOT}/libexec/hooks/boot-sel.d" || ret=$?
      cat > "${BUILDROOT}/libexec/hooks/boot-sel.d/99-zbm-shadow.sh" <<-'EOF'
	#!/bin/bash
	## Shadow mode: compare the legacy runtime's decisions for this boot with
	## the Rust code's and print the differences and a summary line. It only
	## reads and never stops the boot.
	##
	## This hook may be disabled with zbm.skip_hooks=99-zbm-shadow.sh

	/libexec/zbm shadow
	exit 0
	EOF
      chmod 755 "${BUILDROOT}/libexec/hooks/boot-sel.d/99-zbm-shadow.sh" || ret=$?
    fi
  fi

  # Install user hooks via deprecated syntax first

  # shellcheck disable=SC2154
  if [[ "${zfsbootmenu_early_setup@a}" != *a* ]]; then
    # shellcheck disable=SC2086
    populate_hook_dir "early-setup.d" ${zfsbootmenu_early_setup} || ret=$?
  else
    populate_hook_dir "early-setup.d" "${zfsbootmenu_early_setup[@]}" || ret=$?
  fi

  # shellcheck disable=SC2154
  if [[ "${zfsbootmenu_setup@a}" != *a* ]]; then
    # shellcheck disable=SC2086
    populate_hook_dir "setup.d" ${zfsbootmenu_setup} || ret=$?
  else
    populate_hook_dir "setup.d" "${zfsbootmenu_setup[@]}" || ret=$?
  fi

  # shellcheck disable=SC2154
  if [[ "${zfsbootmenu_teardown@a}" != *a* ]]; then
    # shellcheck disable=SC2086
    populate_hook_dir "teardown.d" ${zfsbootmenu_teardown} || ret=$?
  else
    populate_hook_dir "teardown.d" "${zfsbootmenu_teardown[@]}" || ret=$?
  fi

  # Install user hooks using the preferred zfsbootmenu_hook_root mechanism
  for hdir in "${stages[@]}"; do
    # shellcheck disable=SC2154
    hsrc="${zfsbootmenu_hook_root}/${hdir}"
    [ -d "${hsrc}" ] || continue
    populate_hook_dir "${hdir}" "${hsrc}"/* || ret=$?
  done

  return $ret
}


# prints: nothing
# returns: 0 unless LUKS support has been disabled with zfsbootmenu_luks=no

zbm_luks_enabled() {
  # shellcheck disable=SC2154
  case "${zfsbootmenu_luks,,}" in
    no|n|off|0|false) return 1 ;;
  esac
  return 0
}

# arg1: name of a udev rule file
# prints: nothing
# returns: 0 if the rule exists on the build host

zbm_have_udev_rule() {
  local rdir
  for rdir in /etc/udev/rules.d /usr/lib/udev/rules.d /lib/udev/rules.d; do
    [ -e "${rdir}/${1}" ] && return 0
  done
  return 1
}

# arg1: path to an executable
# prints: nothing
# returns: 0 if the executable (and its dependencies) were installed

zbm_install_executable() {
  case "${ZBM_BUILDSTYLE,,}" in
    mkinitcpio) add_binary "${1}" ;;
    dracut) inst "${1}" ;;
    *) return 1 ;;
  esac
}

zbm_info() {
  case "${ZBM_BUILDSTYLE,,}" in
    mkinitcpio) msg2 "$@" ;;
    dracut) dinfo "$@" ;;
    *) echo "INFO: $*" >&2 ;;
  esac
}

zbm_warn() {
  case "${ZBM_BUILDSTYLE,,}" in
    mkinitcpio) warning "$@" ;;
    dracut) dwarning "$@" ;;
    *) echo "WARNING: $*" >&2 ;;
  esac
}

zbm_error() {
  case "${ZBM_BUILDSTYLE,,}" in
    mkinitcpio) error "$@" ;;
    dracut) derror "$@" ;;
    *) echo "ERROR: $*" >&2 ;;
  esac
}

# arg1: path to an executable
# arg2: path in the image
# prints: nothing
# returns: 0 if the executable (and its dependencies) were installed

zbm_install_executable_as() {
  case "${ZBM_BUILDSTYLE,,}" in
    mkinitcpio) add_binary "${1}" "${2}" ;;
    dracut) inst_binary "${1}" "${2}" ;;
    *) return 1 ;;
  esac
}

# Check the runtime selected with zfsbootmenu_runtime and install zbm, the
# Rust runtime program, from zfsbootmenu_zbm. The legacy runtime is the
# default; its images include zbm at /libexec/zbm when it is found, but do not
# use it. The Rust runtime (zfsbootmenu_runtime=rust) needs zbm. Sets
# ZBM_RUNTIME for create_zbm_conf and the start-up hooks.
#
# prints: nothing
# returns: 0 unless the setting is wrong or zbm cannot be installed

install_zbm_runtime() {
  local runtime zbm

  # shellcheck disable=SC2154
  runtime="${zfsbootmenu_runtime:-legacy}"
  case "${runtime,,}" in
    legacy)
      ZBM_RUNTIME="legacy"
      ;;
    rust)
      ZBM_RUNTIME="rust"
      ;;
    *)
      zbm_error "zfsbootmenu_runtime=${runtime}: unknown runtime; use zfsbootmenu_runtime=legacy or rust"
      return 1
      ;;
  esac

  # shellcheck disable=SC2154
  zbm="${zfsbootmenu_zbm:-/usr/libexec/zfsbootmenu/zbm}"
  if [ ! -x "${zbm}" ]; then
    if [ "${ZBM_RUNTIME}" = "rust" ]; then
      zbm_error "zfsbootmenu_runtime=rust: zbm not found at ${zbm}"
      return 1
    fi
    zbm_info "zbm not found at ${zbm}; the legacy runtime does not need it"
    return 0
  fi

  if ! zbm_install_executable_as "${zbm}" "/libexec/zbm"; then
    zbm_error "failed to install ${zbm}"
    return 1
  fi
  return 0
}

# arg1: option value
# arg2: default (yes or no)
# prints: nothing
# returns: 0 if the option is enabled

zbm_opt_enabled() {
  local val="${1:-${2}}"
  case "${val,,}" in
    no|n|off|0|false) return 1 ;;
  esac
  return 0
}

# arg1: library soname, e.g. libfido2.so.1
# prints: path of the library on the build host
# returns: 0 if found

zbm_find_library() {
  local lib
  lib="$( ldconfig -p 2>/dev/null | awk -v so="${1}" '$1 == so { print $NF; exit }' )"
  if [ -z "${lib}" ]; then
    for lib in /usr/lib/*/"${1}" /usr/lib64/"${1}" /usr/lib/"${1}" /lib/*/"${1}" /lib64/"${1}" /lib/"${1}"; do
      [ -e "${lib}" ] && break
      lib=""
    done
  fi
  [ -n "${lib}" ] && echo "${lib}"
}

# arg1: path to a shared library
# prints: nothing
# returns: 0 if the library and its dependencies were installed

zbm_install_library() {
  case "${ZBM_BUILDSTYLE,,}" in
    mkinitcpio) add_binary "${1}" ;;
    dracut) inst_library "${1}" ;;
    *) return 1 ;;
  esac
}

# args: names of executables
# prints: nothing
# returns: 0 if every executable was found and installed

zbm_install_executables() {
  local exe path ret=0
  for exe in "$@"; do
    if ! path="$( command -v "${exe}" 2>/dev/null )"; then
      ret=1
      continue
    fi
    zbm_install_executable "${path}" || ret=1
  done
  return "${ret}"
}

# Install support for LUKS2 tokens: systemd-tpm2 and systemd-fido2 (plugins
# and native tools) and clevis
#
# prints: nothing
# returns: 0 on success

install_zbm_luks_tokens() {
  local plugin dir found lib exe cdir

  # shellcheck disable=SC2154
  zbm_opt_enabled "${zfsbootmenu_luks_tokens}" yes || return 0

  # systemd's cryptsetup token plugins (systemd-cryptenroll)
  # shellcheck disable=SC2154
  if zbm_opt_enabled "${zfsbootmenu_luks_token_plugins}" yes; then
    for plugin in systemd-tpm2 systemd-fido2; do
      found=""
      for dir in /usr/lib/*/cryptsetup /usr/lib64/cryptsetup /usr/lib/cryptsetup \
          /lib/*/cryptsetup /lib64/cryptsetup /lib/cryptsetup; do
        [ -e "${dir}/libcryptsetup-token-${plugin}.so" ] || continue
        zbm_install_library "${dir}/libcryptsetup-token-${plugin}.so" && found=1
        break
      done
      [ -n "${found}" ] || zbm_info "cryptsetup ${plugin} token plugin not found; using native support"
    done
  fi

  # Libraries loaded at run time by the plugins and tpm2-tools
  # shellcheck disable=SC2154
  for lib in "${zfsbootmenu_luks_token_libs[@]}"; do
    lib="$( zbm_find_library "${lib}" )" || continue
    zbm_install_library "${lib}"
  done

  # Native systemd-tpm2 and systemd-fido2 support
  # shellcheck disable=SC2154
  zbm_install_executables "${zfsbootmenu_luks_tpm2_binaries[@]}" \
    || zbm_info "tpm2-tools/openssl incomplete; native TPM2 unlocking may be unavailable"
  # shellcheck disable=SC2154
  zbm_install_executables "${zfsbootmenu_luks_fido2_binaries[@]}" \
    || zbm_info "fido2-tools not found; native FIDO2 unlocking unavailable"

  # clevis
  # shellcheck disable=SC2154
  if zbm_opt_enabled "${zfsbootmenu_luks_clevis}" yes && exe="$( command -v clevis 2>/dev/null )"; then
    cdir="${exe%/*}"
    for exe in "${cdir}"/clevis "${cdir}"/clevis-*; do
      [ -e "${exe}" ] || continue
      if [ -x "${exe}" ]; then
        zbm_install_executable "${exe}"
      else
        zbm_install_file "${exe}" "${exe}"
      fi
    done
    # shellcheck disable=SC2154
    zbm_install_executables "${zfsbootmenu_luks_clevis_binaries[@]}"
  fi

  return 0
}

# Install GnuPG with the public keys used to encrypt LUKS key files
#
# prints: nothing
# returns: 0 on success

install_zbm_luks_gpg() {
  local comp path tmphome key ret=0
  local -a keyids keyfiles

  # shellcheck disable=SC2154
  zbm_opt_enabled "${zfsbootmenu_luks_gpg}" yes || return 0
  command -v gpg >/dev/null 2>&1 || return 0

  zbm_install_executables gpg gpgconf gpg-connect-agent || ret=1

  # gpg-agent, scdaemon (OpenPGP smart cards) and keyboxd live in
  # distribution-specific locations; gpgconf knows where
  while IFS=: read -r comp _ path; do
    case "${comp}" in
      gpg-agent|scdaemon|keyboxd)
        [ -x "${path}" ] && zbm_install_executable "${path}"
        ;;
    esac
  done <<< "$( gpgconf --list-components 2>/dev/null )"

  # Optional PC/SC daemon for readers not handled by GnuPG's own CCID driver
  # shellcheck disable=SC2154
  if zbm_opt_enabled "${zfsbootmenu_luks_pcscd}" no && path="$( command -v pcscd 2>/dev/null )"; then
    zbm_install_executable "${path}"
    for path in /usr/lib/pcsc /usr/lib64/pcsc /etc/reader.conf.d /etc/libccid_Info.plist; do
      [ -e "${path}" ] || continue
      ( cd / && tar -cf - "${path#/}" ) | ( cd "${BUILDROOT}" && tar -xf - )
    done
  fi

  # Public keys never belong in release images
  [ -n "${zfsbootmenu_release_build}" ] && return "${ret}"

  # shellcheck disable=SC2154
  read -r -a keyids <<< "${zfsbootmenu_luks_gpg_keys}"
  # shellcheck disable=SC2154
  read -r -a keyfiles <<< "${zfsbootmenu_luks_gpg_pubkeys}"
  [ "$(( ${#keyids[@]} + ${#keyfiles[@]} ))" -gt 0 ] || return "${ret}"

  # Build the keyring in a short path (agent sockets must fit a sockaddr_un);
  # an empty common.conf keeps gpg from switching to keyboxd
  tmphome="$( mktemp -d /tmp/zbmgpg.XXXXXX )" || return 1
  chmod 700 "${tmphome}"
  : > "${tmphome}/common.conf"

  for key in "${keyids[@]}"; do
    if ! gpg --batch --export "${key}" 2>/dev/null | GNUPGHOME="${tmphome}" gpg --batch --import 2>/dev/null; then
      zbm_warn "unable to export GnuPG public key ${key} from the build host"
      ret=1
    fi
  done

  for key in "${keyfiles[@]}"; do
    if ! GNUPGHOME="${tmphome}" gpg --batch --import "${key}" 2>/dev/null; then
      zbm_warn "unable to import GnuPG public key file ${key}"
      ret=1
    fi
  done

  mkdir -p "${BUILDROOT}/etc/zfsbootmenu/gnupg"
  for key in pubring.kbx pubring.gpg trustdb.gpg common.conf; do
    [ -e "${tmphome}/${key}" ] && cp "${tmphome}/${key}" "${BUILDROOT}/etc/zfsbootmenu/gnupg/"
  done
  chmod -R go-rwx "${BUILDROOT}/etc/zfsbootmenu/gnupg"

  GNUPGHOME="${tmphome}" gpgconf --kill all >/dev/null 2>&1
  rm -rf "${tmphome}"

  return "${ret}"
}

# arg1: path of a GnuPG-encrypted key file on the build host
# prints: nothing
# returns: 0 if the file was embedded in the image

install_zbm_luks_gpg_keyfile() {
  local file="${1}"

  if [ ! -r "${file}" ]; then
    zbm_warn "GnuPG key file ${file} not found"
    return 1
  fi

  # Only encrypted files may be embedded; refuse anything else
  if ! gpg --batch --list-only --list-packets "${file}" 2>/dev/null | grep -q '^:\(pubkey\|symkey\) enc packet:'; then
    zbm_warn "${file} is not GnuPG-encrypted; refusing to embed it in the image"
    return 1
  fi

  if ! gpg --batch --list-only --list-packets "${file}" 2>/dev/null | grep -q '^:pubkey enc packet:'; then
    zbm_warn "${file} is only protected by a passphrase; anyone with the image can attack it offline"
  fi

  mkdir -p "${BUILDROOT}${file%/*}"
  cp "${file}" "${BUILDROOT}${file}" && chmod 0600 "${BUILDROOT}${file}"
}

# Install LUKS configuration: the ZFSBootMenu crypttab, any key scripts and
# GnuPG-encrypted key files it references, and support for tokens, GnuPG and
# YubiKeys. Binaries, modules and udev rules for dm-crypt are installed by the
# builder.
#
# prints: nothing
# returns: 0 on success

install_zbm_luks() {
  local crypttab line name device keyfile options opt ks ret
  local -a optlist

  zbm_luks_enabled || return 0

  if ! command -v cryptsetup >/dev/null 2>&1; then
    zbm_warn "cryptsetup not found; ZFS pools on LUKS volumes will not be unlocked"
    return 0
  fi

  ret=0

  install_zbm_luks_tokens || ret=1
  install_zbm_luks_gpg || ret=1

  # shellcheck disable=SC2154
  if zbm_opt_enabled "${zfsbootmenu_luks_yubikey}" yes; then
    # shellcheck disable=SC2154
    zbm_install_executables "${zfsbootmenu_luks_yubikey_binaries[@]}"
  fi

  : "${zfsbootmenu_luks_crypttab:=/etc/zfsbootmenu/crypttab}"
  crypttab="${zfsbootmenu_luks_crypttab}"

  # Like hostid, a crypttab is host-specific and never belongs in releases
  # shellcheck disable=SC2154
  [ -n "${zfsbootmenu_release_build}" ] && return 0
  [ -r "${crypttab}" ] || return 0

  mkdir -p "${BUILDROOT}/etc/zfsbootmenu"
  if ! cp "${crypttab}" "${BUILDROOT}/etc/zfsbootmenu/crypttab"; then
    zbm_warn "failed to install LUKS crypttab ${crypttab}"
    return 1
  fi
  chmod 0600 "${BUILDROOT}/etc/zfsbootmenu/crypttab"

  # The last line may lack a newline; files edited on Windows end in CRLF
  while IFS= read -r line || [ -n "${line}" ]; do
    line="${line%$'\r'}"
    read -r name device keyfile options _ <<< "${line}"
    case "${name}" in
      ""|"#"*) continue ;;
    esac

    # Plain key files are deliberately not embedded: the image is not
    # encrypted. GnuPG-encrypted key files are, as they are useless without
    # their passphrase or smart card.
    case "${keyfile}" in
      ""|-|none) ;;
      /*:*) ;;
      *.gpg|*.asc|*.pgp)
        install_zbm_luks_gpg_keyfile "${keyfile}" || ret=1
        ;;
      *)
        if [[ ",${options}," == *,gpg,* ]]; then
          install_zbm_luks_gpg_keyfile "${keyfile}" || ret=1
        else
          zbm_warn "crypttab entry '${name}' uses key file '${keyfile}', which is NOT embedded in the image; provide it at boot time (e.g., from an early-setup hook) or use path:device"
        fi
        ;;
    esac

    IFS=',' read -r -a optlist <<< "${options}"
    for opt in "${optlist[@]}"; do
      case "${opt}" in
        keyscript=*)
          ks="${opt#keyscript=}"
          [[ "${ks}" == /* ]] || ks="/lib/cryptsetup/scripts/${ks}"
          if [ -x "${ks}" ]; then
            zbm_install_executable "${ks}" || ret=1
          else
            zbm_warn "key script '${ks}' for crypttab entry '${name}' not found"
            ret=1
          fi
          ;;
      esac
    done
  done < "${crypttab}"

  return "${ret}"
}

find_libgcc_s() {
  local f libdirs libbase ldir zlibs matched

  # Skip detection if desired
  # shellcheck disable=SC2154
  case "${zfsbootmenu_skip_gcc_s,,}" in
    yes|on|1) return 0 ;;
  esac

  # This is only required on glibc systems due to a dlopen in pthread_cancel
  # https://github.com/openzfs/zfs/commit/24554082bd93cb90400c4cb751275debda229009
  ldconfig -p 2>/dev/null | grep -qF 'libc.so.6' || return 0

  # Build a list of libraries linked by zpool
  zlibs="$( ldd "$( command -v zpool 2>/dev/null )" )" || zlibs=

  # If zpool links libgcc_s overtly, there is no need for further action
  if grep -qF 'libgcc_s.so' <<< "${zlibs}"; then
    return 0
  fi

  # Query gcc-config for a current runtime profile if possible
  if command -v gcc-config >/dev/null 2>&1; then
    local gver
    if gver="$( gcc-config -c )"; then
      for f in "/usr/lib/gcc/${gver%-*}/${gver##*-}"/libgcc_s.so*; do
        [ -e "${f}" ] || continue
        echo "${f}"
        matched="yes"
      done
      [ -n "${matched}" ] && return 0
    fi
  fi

  # Try walking library paths to find libgcc_s

  # Search the system cache (adapted from dracut)
  libdirs="$( ldconfig -pN 2>/dev/null \
              | grep -E -v '/(lib|lib64|usr/lib|usr/lib64)/[^/]*$' \
              | sed -n 's,.* => \(.*\)/.*,\1,p' | sort | uniq )" || libdirs=""

  # Search zpool dependencies to figure out system libdirs
  if [[ "${zlibs}" == */lib64/* ]]; then
    libbase="lib64"
  else
    libbase="lib"
  fi

  # Look in all possible system library directories
  libdirs="/${libbase} /usr/${libbase} ${libdirs}"
  for ldir in ${libdirs}; do
    for f in "${ldir}"/libgcc_s.so*; do
      [ -e "${f}" ] || continue
      echo "${f}"
      matched="yes"
    done
  done
  [ -n "${matched}" ] && return 0

  # As a final fallback, just try to grab *any* libgcc_s from GCC
  for f in /usr/lib/gcc/*/*/libgcc_s.so*; do
    [ -f "${f}" ] || continue
    echo "${f}"
    matched="yes"
  done

  [ -n "${matched}" ] && return 0
  return 1
}
