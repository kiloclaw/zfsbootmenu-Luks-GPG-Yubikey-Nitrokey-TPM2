#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

# The ZFSBootMenu dracut module: zbm decides what goes into the image (`zbm
# image plan`, ROADMAP.md SB8, docs/dev/adr/0006-image-plan.md), and this
# module installs it with dracut's functions. The plan is a manifest of
# tab-separated lines: installkernel takes the kernel modules and the
# firmware, install the rest.

check() {
  # Do not include this module by default; it must be requested
  return 255
}

depends() {
  echo bash udev-rules
  return 0
}

# Prints the build's settings for zbm, a line each: NAME<TAB>VALUE for a
# variable, NAME[]<TAB>VALUE for each element of an array.
zbm_settings() {
  local name value attributes
  for name in $( compgen -v zfsbootmenu_ ) moddir embedded_kcl dracut_trace_enable \
    SOURCE_DATE_EPOCH PATH; do
    attributes="$( declare -p "${name}" 2>/dev/null )" || continue
    attributes="${attributes#declare -}"
    if [[ "${attributes%% *}" == *a* ]]; then
      local -n zbm_array="${name}"
      for value in "${zbm_array[@]}"; do
        printf '%s[]\t%s\n' "${name}" "${value//$'\n'/ }"
      done
      unset -n zbm_array
    else
      value="${!name}"
      printf '%s\t%s\n' "${name}" "${value//$'\n'/ }"
    fi
  done
}

# Writes the plan of the image into zbm_plan_dir, once a build.
zbm_plan() {
  [ -n "${zbm_plan_dir}" ] && [ -r "${zbm_plan_dir}/manifest" ] && return 0

  : "${zfsbootmenu_zbm:=/usr/libexec/zfsbootmenu/zbm}"
  if [ ! -x "${zfsbootmenu_zbm}" ]; then
    dfatal "zbm, which plans every ZFSBootMenu image, is not at ${zfsbootmenu_zbm} (zfsbootmenu_zbm)"
    return 1
  fi

  # dracut removes its temporary directory when it ends
  zbm_plan_dir="$( mktemp -d -p "${DRACUT_TMPDIR:-${TMPDIR:-/var/tmp}}" zbm-plan.XXXXXX )" || return 1
  zbm_settings > "${zbm_plan_dir}/settings" || return 1
  if ! "${zfsbootmenu_zbm}" image plan --builder dracut --config "${zbm_plan_dir}/settings" \
    --stage "${zbm_plan_dir}/stage" > "${zbm_plan_dir}/manifest.new"; then
    dfatal "Unable to plan the ZFSBootMenu image"
    return 1
  fi
  mv "${zbm_plan_dir}/manifest.new" "${zbm_plan_dir}/manifest"
}

# The omit_drivers regular expression of dracut without the drivers named:
# dracut leaves out the modules it matches, and the dependencies of others too
# (drm, for every graphics driver). Newer dracut anchors each name (^drm$),
# older dracut does not.
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

# Installs the kernel module of a manifest line: arg1 the module, arg2
# required or optional, arg3 host or any (also in a host-only image).
zbm_install_module() {
  local -a flags=()
  [ "${2}" = required ] && flags=( -c )
  if [ "${3}" = any ]; then
    hostonly='' instmods "${flags[@]}" "${1}"
  else
    instmods "${flags[@]}" "${1}"
  fi
}

installkernel() {
  local kind one two three
  # shellcheck disable=SC2154
  local omit_drivers="${omit_drivers}"

  zbm_plan || exit 1
  while IFS=$'\t' read -r kind one two three; do
    case "${kind}" in
      unomit)
        omit_drivers="$( zbm_unomit_drivers "${omit_drivers}" "${one}" )"
        ;;
      module)
        if ! zbm_install_module "${one}" "${two}" "${three}" && [ "${two}" = required ]; then
          dfatal "Required kernel module '${one}' is missing, aborting image creation!"
          exit 1
        fi
        ;;
      firmware)
        # shellcheck disable=SC2154
        if ! "${zfsbootmenu_zbm}" image prune-firmware "${initdir}" "${one}" "${two}"; then
          dfatal "Unable to install the graphics firmware"
          exit 1
        fi
        ;;
    esac
  done < "${zbm_plan_dir}/manifest"
}

# Installs a line of the manifest but the kernel's; returns 1 when the build
# cannot go on.
zbm_install_line() {
  local kind="${1}" one="${2}" two="${3}" three="${4}"
  case "${kind}" in
    dir)
      mkdir -p "${initdir}${one}" && chmod "${two}" "${initdir}${one}"
      ;;
    file)
      inst_simple "${one}" "${two}" || { dfatal "failed to install file '${one}'"; return 1; }
      [ "${three}" = - ] || chmod "${three}" "${initdir}${two}"
      ;;
    link)
      mkdir -p "${initdir}${two%/*}" && ln -sfn "${one}" "${initdir}${two}" \
        || { dfatal "failed to link '${two}' to '${one}'"; return 1; }
      ;;
    hardlink)
      mkdir -p "${initdir}${two%/*}" && ln -f "${initdir}${one}" "${initdir}${two}" \
        || { dfatal "failed to link '${two}' to '${one}'"; return 1; }
      ;;
    binary)
      inst_binary "${one}" "${two}" || { dfatal "failed to install '${one}'"; return 1; }
      ;;
    library)
      inst_library "${one}" || { dfatal "failed to install '${one}'"; return 1; }
      ;;
    rule)
      if ! inst_rules "${one}"; then
        [ "${two}" = required ] && { dfatal "failed to install udev rule '${one}'"; return 1; }
        dwarning "failed to install udev rule '${one}'"
      fi
      ;;
    hook)
      inst_hook "${one}" "${two}" "${three}" || { dfatal "failed to install hook '${three}'"; return 1; }
      ;;
    hostonly)
      if [ -e "${initdir}${one}" ] && type mark_hostonly >/dev/null 2>&1; then
        mark_hostonly "${one}"
      fi
      ;;
    info) dinfo "${one}" ;;
    warn) dwarning "${one}" ;;
  esac
  return 0
}

install() {
  local kind one two three

  zbm_plan || exit 1
  while IFS=$'\t' read -r kind one two three; do
    zbm_install_line "${kind}" "${one}" "${two}" "${three}" || exit 1
  done < "${zbm_plan_dir}/manifest"
}
