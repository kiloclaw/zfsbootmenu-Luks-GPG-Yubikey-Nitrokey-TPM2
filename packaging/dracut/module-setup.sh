#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

check() {
  # Do not include this module by default; it must be requested
  return 255
}

depends() {
  echo bash udev-rules
  return 0
}

installkernel() {
  local mod

  # shellcheck disable=SC2154
  for mod in "${zfsbootmenu_essential_modules[@]}"; do
    if ! instmods -c "${mod}" ; then
      dfatal "Required kernel module '${mod}' is missing, aborting image creation!"
      exit 1
    fi
  done

  # shellcheck disable=SC2154
  for mod in "${zfsbootmenu_optional_modules[@]}"; do
    instmods "${mod}"
  done

  # dm-crypt and crypto modules for ZFS pools on LUKS volumes
  if zbm_luks_enabled; then
    # shellcheck disable=SC2154
    for mod in "${zfsbootmenu_luks_modules[@]}"; do
      instmods "${mod}"
    done

    # TPM and USB/HID drivers for TPM2 chips, security keys and smart cards
    # shellcheck disable=SC2154
    for mod in "${zfsbootmenu_luks_token_modules[@]}"; do
      instmods "${mod}"
    done
  fi

  # Graphics drivers, with only the firmware of the GPUs the image is for.
  # install, which dracut runs first, has sourced the helpers and chosen the
  # runtime; a build of the kernel modules alone cannot take the drivers.
  # shellcheck disable=SC2154
  if [ -n "${zfsbootmenu_graphics_drivers}" ]; then
    if ! type install_zbm_graphics >/dev/null 2>&1 || ! install_zbm_graphics; then
      dfatal "Unable to install the graphics drivers"
      exit 1
    fi
  fi
}

# The uutils multi-call binary, when the host has one: images then get their
# coreutils from it, one binary with a hard link per program, whatever the
# host's own coreutils are (REFACTOR-ROADMAP.md, "Using uutils coreutils").
# uutils refuses to run as a utility whose name is not the executable's, so
# the links are hard links, not symbolic ones. GNU coreutils' optional
# multi-call binary has no --list, so it is not taken.
zbm_uutils=""
if command -v coreutils >/dev/null 2>&1 && coreutils --list >/dev/null 2>&1; then
  zbm_uutils="$( command -v coreutils )"
fi

# Installs the program _exec: a hard link to the uutils multi-call binary if
# it provides _exec, otherwise the host's program
zbm_install_exec() {
  local _exec="${1}"
  if [ -n "${zbm_uutils}" ] && coreutils --list 2>/dev/null | grep -qxF "${_exec}"; then
    inst_simple "${zbm_uutils}" /usr/bin/coreutils || return 1
    # shellcheck disable=SC2154
    ln -f "${initdir}/usr/bin/coreutils" "${initdir}/usr/bin/${_exec}"
    return 0
  fi
  dracut_install "${_exec}"
}

install() {
  : "${zfsbootmenu_module_root:=/usr/share/zfsbootmenu}"
  : "${zfsbootmenu_hook_root:=/etc/zfsbootmenu/hooks}"

  # shellcheck disable=SC1091
  if ! source "${zfsbootmenu_module_root}/install-helpers.sh" ; then
    dfatal "Unable to source ${zfsbootmenu_module_root}/install-helpers.sh"
    exit 1
  fi

  # BUILDROOT is an initcpio-ism
  # shellcheck disable=SC2154,2034
  BUILDROOT="${initdir}"

  # shellcheck disable=SC2034
  ZBM_BUILDSTYLE="dracut"

  # Check the selected runtime and install zbm
  if ! install_zbm_runtime; then
    dfatal "Unable to install the ZFSBootMenu runtime"
    exit 1
  fi

  local _rule _exec _ret

  # shellcheck disable=SC2154
  for _rule in "${zfsbootmenu_udev_rules[@]}"; do
    if ! inst_rules "${_rule}"; then
      dfatal "failed to install udev rule '${_rule}'"
      exit 1
    fi
  done

  # shellcheck disable=SC2154
  for _exec in "${zfsbootmenu_essential_binaries[@]}"; do
    if ! zbm_install_exec "${_exec}"; then
      dfatal "failed to install essential executable '${_exec}'"
      exit 1
    fi
  done

  # shellcheck disable=SC2154
  for _exec in "${zfsbootmenu_optional_binaries[@]}"; do
    if ! zbm_install_exec "${_exec}"; then
      dwarning "optional component '${_exec}' not found, omitting from image"
    fi
  done

  # Support for ZFS pools on LUKS volumes
  if zbm_luks_enabled && command -v cryptsetup >/dev/null 2>&1; then
    # shellcheck disable=SC2154
    for _exec in "${zfsbootmenu_luks_binaries[@]}"; do
      if ! zbm_install_exec "${_exec}"; then
        dwarning "LUKS component '${_exec}' not found, omitting from image"
      fi
    done

    # shellcheck disable=SC2154
    for _rule in "${zfsbootmenu_luks_udev_rules[@]}"; do
      zbm_have_udev_rule "${_rule}" || continue
      inst_rules "${_rule}" || dwarning "failed to install udev rule '${_rule}'"
    done

    install_zbm_luks || dwarning "LUKS configuration was not fully installed"

    # If the dracut crypt module is included anyway, keep it from prompting
    # for LUKS volumes before ZFSBootMenu can handle them
    mkdir -p "${initdir}/etc/cmdline.d"
    echo "rd.luks=0" > "${initdir}/etc/cmdline.d/zfsbootmenu-luks.conf"
  fi

  # Add libgcc_s as appropriate
  local _libgcc_s
  if ! _libgcc_s="$( find_libgcc_s )"; then
    dfatal "Unable to locate libgcc_s.so"
    exit 1
  fi

  local _lib
  while read -r _lib ; do
    [ -n "${_lib}" ] || continue
    if ! dracut_install "${_lib}"; then
      dfatal "Failed to install '${_lib}'"
      exit 1
    fi
  done <<< "${_libgcc_s}"

  compat_dirs=( "/etc/zfs/compatibility.d" "/usr/share/zfs/compatibility.d/" )
  for compat_dir in "${compat_dirs[@]}"; do
    # shellcheck disable=2164
    [ -d "${compat_dir}" ] && tar -cf - "${compat_dir}" | ( cd "${initdir}" ; tar xfp - )
  done

  _ret=0

  # Install core ZFSBootMenu functionality
  install_zbm_core || _ret=$?

  # Install runtime hooks
  install_zbm_hooks || _ret=$?

  # Hooks necessary to initialize ZBM
  # shellcheck disable=SC2154
  if [ "${ZBM_RUNTIME}" = "rust" ]; then
    inst_hook cmdline 95 "${moddir}/zfsbootmenu-rust-cmdline.sh" || _ret=$?
    inst_hook pre-mount 90 "${moddir}/zfsbootmenu-rust-preinit.sh" || _ret=$?
  else
    inst_hook cmdline 95 "${zfsbootmenu_module_root}/pre-init/zfsbootmenu-parse-commandline.sh" || _ret=$?
    inst_hook pre-mount 90 "${zfsbootmenu_module_root}/pre-init/zfsbootmenu-preinit.sh" || _ret=$?
  fi

  # Hooks to force the dracut event loop to fire at least once
  # Things like console configuration are done in optional event-loop hooks
  # shellcheck disable=SC2154
  inst_hook initqueue/settled 99 "${moddir}/zfsbootmenu-ready-set.sh" || _ret=$?
  inst_hook initqueue/finished 99 "${moddir}/zfsbootmenu-ready-chk.sh" || _ret=$?

  if [ ${_ret} -ne 0 ]; then
    dfatal "Unable to install core ZFSBootMenu functions"
    exit 1
  fi

  # Install online documentation if possible
  install_zbm_docs

  # Install an os-release, if one is available
  install_zbm_osver

  # optionally enable early Dracut profiling
  if [ -n "${dracut_trace_enable}" ]; then
    inst_hook cmdline 00 "${zfsbootmenu_module_root}/profiling/profiling-lib.sh"
  fi

  # vdev_id.conf and hostid files are host-specific
  # and do not belong in public release images
  if [ -z "${zfsbootmenu_release_build}" ]; then
    if [ -e /etc/zfs/vdev_id.conf ]; then
      inst /etc/zfs/vdev_id.conf
      type mark_hostonly >/dev/null 2>&1 && mark_hostonly /etc/zfs/vdev_id.conf
    fi

    # Try to synchronize hostid between host and ZFSBootMenu
    #
    # DEPRECATION NOTICE: on musl systems, zfs < 2.0 produced a bad hostid in
    # dracut images. Unfortunately, this should be replicated for now to ensure
    # those images are bootable. After some time, remove this version check.
    ZVER="$( zfs version | head -n1 | sed 's/zfs-\(kmod-\)\?//' )"
    if [ -n "${ZVER}" ] && printf '%s\n' "${ZVER}" "2.0" | sort -VCr; then
      NEWZFS=yes
    else
      NEWZFS=""
    fi

    if [ -n "${NEWZFS}" ] && [ -e /etc/hostid ]; then
      # With zfs >= 2.0, prefer the hostid file if it exists
      inst /etc/hostid
    elif HOSTID="$( hostid 2>/dev/null )"; then
      # Fall back to `hostid` output when it is nonzero or with zfs < 2.0
      if [ -z "${NEWZFS}" ]; then
        # In zfs < 2.0, zgenhostid does not provide necessary behavior
        echo -ne "\\x${HOSTID:6:2}\\x${HOSTID:4:2}\\x${HOSTID:2:2}\\x${HOSTID:0:2}" > "${initdir}/etc/hostid"
      elif [ "${HOSTID}" != "00000000" ]; then
        # In zfs >= 2.0, zgenhostid writes the output, but only with nonzero hostid
        # shellcheck disable=SC2154
        zgenhostid -o "${initdir}/etc/hostid" "${HOSTID}"
      fi
    fi
  fi

  # shellcheck disable=SC2154
  if [ -e "${initdir}/etc/hostid" ] && type mark_hostonly >/dev/null 2>&1; then
    mark_hostonly /etc/hostid
  fi

  # Force rd.hostonly=0 in the KCL for releases, this will purge itself after 99base/init.sh runs
  # shellcheck disable=SC2154
  if [ -n "${zfsbootmenu_release_build}" ]; then
    echo "rd.hostonly=0" > "${initdir}/etc/cmdline.d/hostonly.conf"
  fi

  create_zbm_conf
  create_zbm_profiles
  create_zbm_traceconf

  # Embed a kernel command line in the initramfs; create_zbm_conf writes the
  # file anew, so this comes after it. Single quotes keep every character as
  # typed (a quote in it is written '\''), so that sourcing the file and the
  # Rust runtime's parser read the same value.
  # shellcheck disable=SC2154
  if [ -n "${embedded_kcl}" ]; then
    printf "export embedded_kcl='%s'\n" "${embedded_kcl//\'/\'\\\'\'}" \
      >> "${initdir}/etc/zfsbootmenu.conf"
  fi

  if command -v setfont >/dev/null 2>&1; then
    install_zbm_fonts && dracut_install setfont
  fi

  install_zbm_keyboard_layouts
  install_zbm_shell_input
}
