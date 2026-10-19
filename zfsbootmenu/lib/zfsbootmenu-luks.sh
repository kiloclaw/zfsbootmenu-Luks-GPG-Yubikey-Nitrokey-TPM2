#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

# LUKS (dm-crypt) support for ZFSBootMenu
#
# These functions unlock LUKS volumes that hold ZFS pool vdevs so that the
# pools can be imported by ZFSBootMenu, and close those volumes again right
# before ZFSBootMenu hands control to a boot environment with kexec.
#
# Which volumes get unlocked is controlled, in order of precedence, by
#
#   1. the kernel command-line option zbm.luks=<off|auto|device,device,...>
#   2. a crypttab(5)-style file at ${ZBM_CRYPTTAB} inside the image
#   3. automatic discovery of every LUKS volume on the system
#
# Mappings opened by ZFSBootMenu are recorded in ${LUKS_STATE} so that the
# teardown hook and the zluks helper know which ones to close.
#
# For each volume, unlock methods are tried in this order: key script, key
# file (plain or GnuPG-encrypted), LUKS2 tokens (TPM2, FIDO2, clevis), YubiKey
# challenge-response, passphrases entered for other volumes, and finally a
# passphrase prompt. See zfsbootmenu-luks-keys.sh for the hardware methods.

# Include guard
[ -n "${_ZFSBOOTMENU_LUKS}" ] && return
readonly _ZFSBOOTMENU_LUKS=1

# timed_prompt, colorize and the KCL helpers live in the core library
if [ -z "${_ZFSBOOTMENU_CORE}" ]; then
  # shellcheck disable=SC1091
  source /lib/zfsbootmenu-core.sh >/dev/null 2>&1 || return 1
fi

# TPM2, FIDO2, clevis, GnuPG and YubiKey support
: "${LUKS_KEYS_LIB:=/lib/zfsbootmenu-luks-keys.sh}"
# shellcheck disable=SC1090
source "${LUKS_KEYS_LIB}" >/dev/null 2>&1 || return 1

: "${BASE:=/zfsbootmenu}"
: "${ZBM_CRYPTTAB:=/etc/zfsbootmenu/crypttab}"
: "${LUKS_STATE:=${BASE}/luks-mappings}"

# Volumes the user chose to skip; later retries leave them alone
: "${LUKS_SKIPPED:=${BASE}/luks-skipped}"

# Default number of passphrase attempts per round for each volume
: "${LUKS_DEFAULT_TRIES:=3}"

# Secrets reused across the volumes of one unlock run, so that every member of
# a mirror or RAIDZ vdev does not ask again: typed passphrases (and YubiKey
# responses), TPM2 PINs, and decrypted GnuPG key files (key file spec -> file
# in RAM). luks_forget_secrets() wipes all of them.
_luks_passphrases=()
_luks_tpm2_pins=()
declare -gA _luks_gpg_plain=()
declare -gA _luks_keydev_absent=()

# prints: nothing
# returns: nothing

luks_forget_secrets() {
  local f
  for f in "${_luks_gpg_plain[@]}"; do
    rm -f "${f}"
  done
  _luks_gpg_plain=()
  _luks_passphrases=()
  _luks_tpm2_pins=()
  _luks_keydev_absent=()
}

# prints: nothing
# returns: 0 if cryptsetup is usable, 1 otherwise

luks_available() {
  command -v cryptsetup >/dev/null 2>&1 || return 1

  # Without the dm udev rules nothing acknowledges libdevmapper's udev cookie
  # and cryptsetup waits forever; let libdevmapper manage nodes itself instead
  if [ -z "${DM_DISABLE_UDEV+x}" ] \
      && [ ! -e /usr/lib/udev/rules.d/95-dm-notify.rules ] \
      && [ ! -e /lib/udev/rules.d/95-dm-notify.rules ] \
      && [ ! -e /etc/udev/rules.d/95-dm-notify.rules ]; then
    zdebug "95-dm-notify.rules missing, disabling udev synchronization for device-mapper"
    export DM_DISABLE_UDEV=1
  fi

  return 0
}

# arg1: device specification
#       /dev/..., UUID=, PARTUUID=, LABEL=, PARTLABEL=, ID=, PATH=,
#       a bare LUKS UUID or luks-<UUID>
# prints: canonical device node
# returns: 0 if the device exists, 1 otherwise

luks_resolve_device() {
  local spec key val dev

  spec="${1}"
  [ -n "${spec}" ] || return 1

  case "${spec}" in
    /dev/*)
      dev="${spec}"
      ;;
    *=*)
      key="${spec%%=*}"
      key="${key^^}"
      val="${spec#*=}"
      case "${key}" in
        UUID) dev="/dev/disk/by-uuid/${val}" ;;
        PARTUUID) dev="/dev/disk/by-partuuid/${val}" ;;
        LABEL) dev="/dev/disk/by-label/${val}" ;;
        PARTLABEL) dev="/dev/disk/by-partlabel/${val}" ;;
        ID) dev="/dev/disk/by-id/${val}" ;;
        PATH) dev="/dev/disk/by-path/${val}" ;;
        *)
          zerror "unrecognized LUKS device specification '${spec}'"
          return 1
          ;;
      esac
      ;;
    *)
      # Accept a bare UUID or the systemd-style luks-<UUID> mapping name
      val="${spec#luks-}"
      if [[ "${val}" =~ ^[[:xdigit:]]{8}-([[:xdigit:]]{4}-){3}[[:xdigit:]]{12}$ ]]; then
        key="UUID"
        dev="/dev/disk/by-uuid/${val}"
      else
        zerror "unrecognized LUKS device specification '${spec}'"
        return 1
      fi
      ;;
  esac

  # LUKS and GPT UUIDs are lower case, FAT volume IDs upper case; tools such
  # as sgdisk print GPT UUIDs in upper case, so accept either
  if [ ! -b "${dev}" ]; then
    case "${key}" in
      UUID) [ -b "/dev/disk/by-uuid/${val,,}" ] && dev="/dev/disk/by-uuid/${val,,}" ;;
      PARTUUID) [ -b "/dev/disk/by-partuuid/${val,,}" ] && dev="/dev/disk/by-partuuid/${val,,}" ;;
    esac
  fi

  # udev symlinks may be missing or escaped differently; ask blkid directly
  if [ ! -b "${dev}" ] && [ -n "${key}" ]; then
    case "${key}" in
      UUID|PARTUUID|LABEL|PARTLABEL)
        dev="$( blkid -c /dev/null -l -o device -t "${key}=${val}" 2>/dev/null )"
        if [ -z "${dev}" ] && [ "${val}" != "${val,,}" ]; then
          case "${key}" in
            UUID|PARTUUID) dev="$( blkid -c /dev/null -l -o device -t "${key}=${val,,}" 2>/dev/null )" ;;
          esac
        fi
        ;;
    esac
  fi

  [ -n "${dev}" ] && [ -b "${dev}" ] || return 1
  readlink -f "${dev}"
}

# arg1: device specification, as for luks_resolve_device
# prints: nothing
# returns: 0 if the device exists

luks_device_present() {
  luks_resolve_device "${1}" >/dev/null 2>&1
}

# arg1: block device
# prints: nothing
# returns: 0 if something (e.g., an active dm-crypt mapping) holds the device

luks_is_open() {
  local dev holder

  dev="$( readlink -f "${1}" 2>/dev/null )" || return 1
  [ -n "${dev}" ] || return 1

  for holder in "/sys/class/block/${dev##*/}/holders"/*; do
    [ -e "${holder}" ] && return 0
  done

  return 1
}

# arg1: LUKS block device
# prints: a default mapping name for the device, luks-<UUID>
# returns: nothing

luks_default_name() {
  local uuid
  uuid="$( cryptsetup luksUUID "${1}" 2>/dev/null )" || uuid=""
  if [ -n "${uuid}" ]; then
    echo "luks-${uuid}"
  else
    echo "luks-${1##*/}"
  fi
}

# arg1: LUKS block device
# prints: a short human-readable description of the device
# returns: nothing

luks_describe_device() {
  local dev desc size label partlabel

  dev="${1}"
  desc="${dev}"

  size="$( lsblk -dno SIZE "${dev}" 2>/dev/null )" || size=""
  size="${size// /}"
  label="$( blkid -c /dev/null -s LABEL -o value "${dev}" 2>/dev/null )" || label=""
  partlabel="$( blkid -c /dev/null -s PARTLABEL -o value "${dev}" 2>/dev/null )" || partlabel=""

  if [ -n "${label}" ]; then
    desc+=" \"${label}\""
  elif [ -n "${partlabel}" ]; then
    desc+=" \"${partlabel}\""
  fi

  [ -n "${size}" ] && desc+=" (${size})"
  echo "${desc}"
}

# arg1: crypttab-style option list (comma separated)
# prints: nothing
# returns: nothing
#
# sets: _luks_args (array of cryptsetup arguments), _luks_tries,
#       _luks_noauto, _luks_nofail, _luks_keyscript, _luks_header, _luks_gpg,
#       _luks_notoken, _luks_yk, _luks_yk_slot, _luks_yk_hash, _luks_yk_concat

luks_parse_options() {
  local opts opt
  local -a optlist

  opts="${1}"

  _luks_args=()
  _luks_tries="${LUKS_DEFAULT_TRIES}"
  _luks_noauto=""
  _luks_nofail=""
  _luks_keyscript=""
  _luks_header=""
  _luks_gpg=""
  _luks_notoken=""
  _luks_yk=""
  _luks_yk_slot=2
  _luks_yk_hash=""
  _luks_yk_concat=""

  [ -n "${opts}" ] && [ "${opts}" != "-" ] || return 0

  IFS=',' read -r -a optlist <<< "${opts}"
  for opt in "${optlist[@]}"; do
    case "${opt}" in
      "") ;;
      discard|allow-discards)
        _luks_args+=( "--allow-discards" )
        ;;
      readonly|read-only)
        _luks_args+=( "--readonly" )
        ;;
      tries=*)
        if [ "${opt#tries=}" -ge 0 ] 2>/dev/null; then
          _luks_tries="${opt#tries=}"
        else
          zwarn "ignoring invalid LUKS option '${opt}'"
        fi
        ;;
      noauto)
        _luks_noauto=1
        ;;
      nofail)
        _luks_nofail=1
        ;;
      keyscript=*)
        _luks_keyscript="${opt#keyscript=}"
        # Like Debian, bare names refer to the stock cryptsetup scripts
        [[ "${_luks_keyscript}" == /* ]] \
          || _luks_keyscript="/lib/cryptsetup/scripts/${_luks_keyscript}"
        ;;
      header=*)
        _luks_header="${opt#header=}"
        _luks_args+=( "--header" "${_luks_header}" )
        ;;
      keyfile-offset=*|keyfile-size=*|key-slot=*|keyslot=*)
        # A bad value would make every attempt fail, the passphrase included
        if [[ "${opt#*=}" =~ ^[0-9]+$ ]]; then
          case "${opt%%=*}" in
            keyslot) _luks_args+=( "--key-slot" "${opt#*=}" ) ;;
            *) _luks_args+=( "--${opt%%=*}" "${opt#*=}" ) ;;
          esac
        else
          zwarn "ignoring invalid LUKS option '${opt}'"
        fi
        ;;
      gpg)
        _luks_gpg=1
        ;;
      no-tokens|notoken|no-token)
        _luks_notoken=1
        ;;
      ykchalresp)
        _luks_yk=1
        ;;
      ykchalresp=*)
        _luks_yk=1
        case "${opt#ykchalresp=}" in
          1|2) _luks_yk_slot="${opt#ykchalresp=}" ;;
          *) zwarn "ignoring invalid YubiKey slot in '${opt}'" ;;
        esac
        ;;
      ykchalresp-hash)
        _luks_yk=1
        _luks_yk_hash=1
        ;;
      ykchalresp-concat)
        _luks_yk=1
        _luks_yk_concat=1
        ;;
      no-read-workqueue)
        _luks_args+=( "--perf-no_read_workqueue" )
        ;;
      no-write-workqueue)
        _luks_args+=( "--perf-no_write_workqueue" )
        ;;
      same-cpu-crypt)
        _luks_args+=( "--perf-same_cpu_crypt" )
        ;;
      submit-from-crypt-cpus)
        _luks_args+=( "--perf-submit_from_crypt_cpus" )
        ;;
      luks|luks1|luks2|initramfs|x-initrd.attach|_netdev|noearly|force)
        # Meaningful to other crypttab consumers, not to ZFSBootMenu
        ;;
      *)
        zdebug "ignoring unsupported LUKS option '${opt}'"
        ;;
    esac
  done
}

# Populate the list of volumes that should be unlocked
#
# prints: nothing
# returns: 0 if LUKS support is enabled, 1 if disabled
#
# sets: _luks_mode (auto, crypttab or cmdline) and the parallel arrays
#       _luks_cfg_name, _luks_cfg_spec, _luks_cfg_key, _luks_cfg_opts

luks_load_config() {
  local arg spec name device keyfile options
  local -a specs

  _luks_mode=""
  _luks_cfg_name=()
  _luks_cfg_spec=()
  _luks_cfg_key=()
  _luks_cfg_opts=()

  if arg="$( get_zbm_arg zbm.luks )"; then
    case "${arg,,}" in
      off|no|none|0|false|disable|disabled)
        zinfo "LUKS unlocking disabled on the command line"
        return 1
        ;;
      ""|auto|on|yes|1|true)
        _luks_mode="auto"
        ;;
      *)
        _luks_mode="cmdline"
        IFS=',' read -r -a specs <<< "${arg}"
        for spec in "${specs[@]}"; do
          [ -n "${spec}" ] || continue
          _luks_cfg_name+=( "" )
          _luks_cfg_spec+=( "${spec}" )
          _luks_cfg_key+=( "" )
          _luks_cfg_opts+=( "" )
        done
        ;;
    esac
  elif [ -r "${ZBM_CRYPTTAB}" ]; then
    # shellcheck disable=SC2034
    local line
    # The last line may lack a newline; files edited on Windows end in CRLF
    while IFS= read -r line || [ -n "${line}" ]; do
      line="${line%$'\r'}"
      read -r name device keyfile options _ <<< "${line}"
      case "${name}" in
        ""|"#"*) continue ;;
      esac

      if [ -z "${device}" ]; then
        zwarn "ignoring malformed entry '${name}' in ${ZBM_CRYPTTAB}"
        continue
      fi

      case "${keyfile}" in
        ""|-|none) keyfile="" ;;
      esac

      _luks_cfg_name+=( "${name}" )
      _luks_cfg_spec+=( "${device}" )
      _luks_cfg_key+=( "${keyfile}" )
      _luks_cfg_opts+=( "${options}" )
    done < "${ZBM_CRYPTTAB}"

    if [ "${#_luks_cfg_spec[@]}" -gt 0 ]; then
      _luks_mode="crypttab"
    else
      _luks_mode="auto"
    fi
  else
    _luks_mode="auto"
  fi

  zdebug "LUKS configuration mode: ${_luks_mode}"
  return 0
}

# arg1: keyfile specification, path[:keydevice]
# arg2: name of a variable that receives the path of a usable key file
# arg3: name of a variable that receives a mount point to clean up, if any
# prints: nothing
# returns: 0 if a readable key file was found, 1 otherwise

luks_find_keyfile() {
  local _kf_spec _kf_file _kf_devspec _kf_dev _kf_dir
  local -n _kf_path="${2}"
  local -n _kf_mnt="${3}"

  _kf_spec="${1}"
  _kf_path=""
  _kf_mnt=""

  [ -n "${_kf_spec}" ] || return 1

  # path:device uses a key file stored on another (e.g., removable) device
  if [[ "${_kf_spec}" == /*:* ]]; then
    _kf_file="${_kf_spec%%:*}"
    _kf_devspec="${_kf_spec#*:}"
  else
    _kf_file="${_kf_spec}"
    _kf_devspec=""
  fi

  if [ -z "${_kf_devspec}" ]; then
    if [ -r "${_kf_file}" ]; then
      _kf_path="${_kf_file}"
      return 0
    fi
    zwarn "LUKS key file ${_kf_file} not found"
    return 1
  fi

  if ! _kf_dev="$( luks_resolve_device "${_kf_devspec}" )"; then
    # USB storage often appears a few seconds after boot. Wait once per run
    # (not during import retries), as for security keys and smart cards.
    if [ -z "${LUKS_RETRY}" ] && [ -z "${_luks_keydev_absent[${_kf_devspec}]}" ] \
      && luks_wait_for "$( luks_token_wait )" \
        "$( colorize cyan "Insert the key device $( colorize yellow "${_kf_devspec}" ) holding ${_kf_file}" )" \
        luks_device_present "${_kf_devspec}"; then
      _kf_dev="$( luks_resolve_device "${_kf_devspec}" )"
    fi
    if [ -z "${_kf_dev}" ]; then
      _luks_keydev_absent[${_kf_devspec}]=1
      zwarn "LUKS key device ${_kf_devspec} not found"
      return 1
    fi
  fi

  mkdir -p /tmp
  if ! _kf_dir="$( mktemp -d /tmp/luks-key.XXXXXX )"; then
    zerror "unable to create a mount point for key device ${_kf_dev}"
    return 1
  fi

  if ! mount -o ro "${_kf_dev}" "${_kf_dir}" >/dev/null 2>&1; then
    zwarn "unable to mount LUKS key device ${_kf_dev}"
    rmdir "${_kf_dir}" 2>/dev/null
    return 1
  fi

  _kf_mnt="${_kf_dir}"

  if [ -r "${_kf_dir}${_kf_file}" ]; then
    _kf_path="${_kf_dir}${_kf_file}"
    return 0
  fi

  zwarn "LUKS key file ${_kf_file} not found on ${_kf_dev}"
  return 1
}

# arg1: mount point created by luks_find_keyfile, possibly empty
# prints: nothing
# returns: nothing

luks_release_keyfile() {
  [ -n "${1}" ] || return 0
  umount "${1}" >/dev/null 2>&1
  rmdir "${1}" >/dev/null 2>&1
}

# arg1: mapping name
# arg2: LUKS device
# arg3: 1 if the volume was discovered automatically, empty otherwise
# prints: nothing
# returns: nothing

luks_record_mapping() {
  mkdir -p "${LUKS_STATE%/*}"

  # Concurrent sessions (console and SSH) may both report the same mapping
  if [ -r "${LUKS_STATE}" ] && grep -q "^${1}"$'\t' "${LUKS_STATE}"; then
    return 0
  fi

  printf '%s\t%s\t%s\n' "${1}" "${2}" "${3:-0}" >> "${LUKS_STATE}"
}

# arg1: LUKS device
# prints: nothing
# returns: nothing

luks_mark_skipped() {
  mkdir -p "${LUKS_SKIPPED%/*}"
  grep -qxF "${1}" "${LUKS_SKIPPED}" 2>/dev/null || echo "${1}" >> "${LUKS_SKIPPED}"
}

# arg1: LUKS device
# prints: nothing
# returns: 0 if a retry should leave the device alone (open or skipped)

luks_retry_ignores() {
  luks_is_open "${1}" && return 0
  grep -qxF "${1}" "${LUKS_SKIPPED}" 2>/dev/null
}

# arg1: mapping name
# arg2: LUKS device
# arg3: key file, or - to read key material from stdin
# args: additional arguments for cryptsetup open
# prints: nothing
# returns: exit status of cryptsetup

luks_cryptsetup_open() {
  local name dev keyfile output ret
  name="${1}"
  dev="${2}"
  keyfile="${3}"
  shift 3

  output="$( cryptsetup open --type luks --key-file="${keyfile}" "$@" "${dev}" "${name}" 2>&1 )"
  ret=$?

  if [ "${ret}" -ne 0 ]; then
    zdebug "cryptsetup open ${dev} ${name} failed (${ret}): ${output}"
  fi

  return "${ret}"
}

# arg1: mapping name
# arg2: LUKS device
# arg3: key file specification (path or path:keydevice), may be empty
# arg4: crypttab-style options, may be empty
# arg5: 1 if the volume was discovered automatically, empty otherwise
# prints: prompts on the console
# returns: 0 if the volume is open on return, 1 otherwise
#
# Successful passphrases are remembered in _luks_passphrases for the duration
# of the current shell so that volumes sharing a passphrase prompt only once.
# Callers should run this in a subshell so the passphrases die with it.

luks_open_device() {
  local name dev keyspec opts discovered
  local keypath keymnt desc pass ret attempt cached plain

  name="${1}"
  dev="${2}"
  keyspec="${3}"
  opts="${4}"
  discovered="${5}"

  luks_parse_options "${opts}"

  if luks_is_open "${dev}"; then
    zinfo "LUKS device ${dev} is already in use, skipping"
    return 0
  fi

  if ! cryptsetup isLuks "${_luks_header:-${dev}}" >/dev/null 2>&1; then
    zwarn "${_luks_header:-${dev}} is not a LUKS volume (header), skipping ${dev}"
    return 1
  fi

  [ -n "${name}" ] || name="$( luks_default_name "${_luks_header:-${dev}}" )"

  if [ -e "/dev/mapper/${name}" ]; then
    zerror "cannot unlock ${dev}: mapping ${name} already exists"
    return 1
  fi

  # 1. A key script prints the key on stdout (Debian crypttab semantics)
  if [ -n "${_luks_keyscript}" ]; then
    if [ -x "${_luks_keyscript}" ]; then
      zinfo "unlocking ${dev} with key script ${_luks_keyscript}"
      CRYPTTAB_NAME="${name}" CRYPTTAB_SOURCE="${dev}" \
        CRYPTTAB_KEY="${keyspec:-none}" CRYPTTAB_OPTIONS="${opts}" \
        CRYPTTAB_TRIED=0 "${_luks_keyscript}" "${keyspec:-none}" \
        | luks_cryptsetup_open "${name}" "${dev}" - "${_luks_args[@]}"

      if [ "${PIPESTATUS[1]}" -eq 0 ]; then
        luks_record_mapping "${name}" "${dev}" "${discovered}"
        zinfo "unlocked ${dev} as ${name} with key script"
        return 0
      fi
      zwarn "key script ${_luks_keyscript} failed to unlock ${dev}"
    else
      zwarn "key script ${_luks_keyscript} is not executable"
    fi
  fi

  # 2. A key file, possibly on a separate key device (with a key script, the
  #    key file field is the script's argument rather than a file). GnuPG-
  #    encrypted key files are decrypted to RAM first.
  plain="${_luks_gpg_plain[${keyspec:-none}]}"
  if [ -n "${keyspec}" ] && [ -z "${_luks_keyscript}" ] && [ -n "${plain}" ] && [ -s "${plain}" ]; then
    # Decrypted earlier in this run for another volume
    zinfo "unlocking ${dev} with GnuPG key file ${keyspec} (already decrypted)"
    if luks_cryptsetup_open "${name}" "${dev}" "${plain}" "${_luks_args[@]}" < /dev/null; then
      luks_record_mapping "${name}" "${dev}" "${discovered}"
      zinfo "unlocked ${dev} as ${name} with key file"
      return 0
    fi
    zwarn "key file ${keyspec} failed to unlock ${dev}"
  elif [ -n "${keyspec}" ] && [ -z "${_luks_keyscript}" ]; then
    if luks_find_keyfile "${keyspec}" keypath keymnt; then
      if [ -n "${_luks_gpg}" ] || luks_is_gpg_keyfile "${keypath}" ""; then
        ret=1
        if plain="$( mktemp /run/zbm-key.XXXXXX 2>/dev/null || mktemp /tmp/zbm-key.XXXXXX )"; then
          if luks_gpg_decrypt "${keypath}" "${plain}" "${dev}"; then
            # Kept in RAM for other volumes until luks_forget_secrets
            _luks_gpg_plain[${keyspec}]="${plain}"
            zinfo "unlocking ${dev} with GnuPG key file ${keyspec}"
            luks_cryptsetup_open "${name}" "${dev}" "${plain}" "${_luks_args[@]}" < /dev/null
            ret=$?
          else
            rm -f "${plain}"
          fi
        fi
      else
        zinfo "unlocking ${dev} with key file ${keyspec}"
        luks_cryptsetup_open "${name}" "${dev}" "${keypath}" "${_luks_args[@]}" < /dev/null
        ret=$?
      fi
      luks_release_keyfile "${keymnt}"

      if [ "${ret}" -eq 0 ]; then
        luks_record_mapping "${name}" "${dev}" "${discovered}"
        zinfo "unlocked ${dev} as ${name} with key file"
        return 0
      fi
      zwarn "key file ${keyspec} failed to unlock ${dev}"
    else
      luks_release_keyfile "${keymnt}"
    fi
  fi

  # 3. LUKS2 tokens: TPM2, FIDO2 security keys and clevis
  if [ -z "${_luks_notoken}" ] && luks_tokens_enabled; then
    if luks_try_tokens "${name}" "${dev}" "${_luks_header:-${dev}}" "${_luks_args[@]}"; then
      luks_record_mapping "${name}" "${dev}" "${discovered}"
      return 0
    fi
    # Another tool may have unlocked the device while tokens were tried
    luks_is_open "${dev}" && return 0
  fi

  # 4. Passphrases (and YubiKey responses) that unlocked another volume
  for cached in "${_luks_passphrases[@]}"; do
    if printf '%s' "${cached}" | luks_cryptsetup_open "${name}" "${dev}" - "${_luks_args[@]}"; then
      luks_record_mapping "${name}" "${dev}" "${discovered}"
      zinfo "unlocked ${dev} as ${name} with a previously entered passphrase"
      return 0
    fi
  done

  # 5. YubiKey HMAC-SHA1 challenge-response
  if [ -n "${_luks_yk}" ]; then
    if luks_try_ykchalresp "${name}" "${dev}" "${_luks_args[@]}"; then
      luks_record_mapping "${name}" "${dev}" "${discovered}"
      return 0
    fi
  fi

  # 6. Ask the user
  desc="$( luks_describe_device "${dev}" )"

  while true; do
    attempt=0
    while [ "${_luks_tries}" -eq 0 ] || [ "${attempt}" -lt "${_luks_tries}" ]; do
      attempt=$(( attempt + 1 ))

      # Echo is disabled before the prompt appears and stays off for the whole
      # prompt (rather than per read), so no keystroke can ever be echoed
      stty -echo 2>/dev/null
      tput cnorm 2>/dev/null
      echo
      colorize cyan "Unlock LUKS volume $( colorize yellow "${desc}" )\n"
      [ "${attempt}" -gt 1 ] && colorize red "No key available with this passphrase.\n"
      echo -n "Passphrase (leave empty to skip): "

      # Poll so that an unlock performed elsewhere (e.g., zluks over SSH) is
      # noticed while this prompt is waiting
      pass=""
      while true; do
        IFS= read -r -t 2 pass
        ret=$?
        [ "${ret}" -eq 0 ] && break

        if [ "${ret}" -gt 128 ]; then
          if luks_is_open "${dev}"; then
            stty echo 2>/dev/null
            echo
            zinfo "${dev} was unlocked elsewhere"
            return 0
          fi
          continue
        fi

        # stdin closed; nothing more can be done interactively
        stty echo 2>/dev/null
        echo
        zwarn "no console available to unlock ${dev}"
        return 1
      done
      stty echo 2>/dev/null
      echo

      if [ -z "${pass}" ]; then
        zinfo "skipping LUKS volume ${dev} at user request"
        luks_mark_skipped "${dev}"
        return 1
      fi

      echo "Unlocking ${dev} ..."
      if printf '%s' "${pass}" | luks_cryptsetup_open "${name}" "${dev}" - "${_luks_args[@]}"; then
        _luks_passphrases+=( "${pass}" )
        pass=""
        luks_record_mapping "${name}" "${dev}" "${discovered}"
        zinfo "unlocked ${dev} as ${name}"
        return 0
      fi
      pass=""

      if luks_is_open "${dev}"; then
        zinfo "${dev} was unlocked elsewhere"
        return 0
      fi

      zwarn "failed to unlock ${dev} (attempt ${attempt})"
    done

    if ! timed_prompt \
      -m "$( colorize red "Unable to unlock LUKS volume ${desc}" )" \
      -r "try again" \
      -e "skip this volume" \
      -p "Retrying in $( colorize yellow "%0.2d" ) seconds"; then
      tput cnorm 2>/dev/null
      tput clear 2>/dev/null
      zwarn "giving up on LUKS volume ${dev}"
      luks_mark_skipped "${dev}"
      return 1
    fi

    tput cnorm 2>/dev/null
    tput clear 2>/dev/null
  done
}

# arg1: device specification
# arg2: 1 if a missing device should be skipped without asking
# prints: canonical device path
# returns: 0 if the device was found, 1 otherwise

luks_wait_device() {
  local spec nofail dev

  spec="${1}"
  nofail="${2}"

  while true; do
    if dev="$( luks_resolve_device "${spec}" )"; then
      echo "${dev}"
      return 0
    fi

    udevadm settle >/dev/null 2>&1
    if dev="$( luks_resolve_device "${spec}" )"; then
      echo "${dev}"
      return 0
    fi

    if [ -n "${nofail}" ]; then
      zinfo "optional LUKS device ${spec} not found, skipping"
      return 1
    fi

    if ! timed_prompt -d "${zbm_retry_delay:-5}" \
      -m "$( colorize red "LUKS device ${spec} is missing" )" \
      -r "retry now" \
      -e "skip this device" \
      -p "Retrying in $( colorize yellow "%0.2d" ) seconds" >&2; then
      tput cnorm >&2 2>/dev/null
      tput clear >&2 2>/dev/null
      zerror "LUKS device ${spec} not found"
      return 1
    fi
  done
}

# prints: every LUKS block device on the system, one per line
# returns: nothing

luks_discover_devices() {
  local dev

  while read -r dev; do
    [ -b "${dev}" ] || continue
    readlink -f "${dev}"
  done <<< "$( blkid -c /dev/null -o device -t TYPE=crypto_LUKS 2>/dev/null )" | sort -u
}

# arg1: mapping name
# prints: nothing
# returns: 0 if the opened mapping might hold a ZFS vdev, 1 otherwise

luks_may_hold_zfs() {
  local fstype
  fstype="$( blkid -c /dev/null -p -s TYPE -o value "/dev/mapper/${1}" 2>/dev/null )" || fstype=""

  # Unknown contents (or partitioned devices) are given the benefit of the doubt
  [ -z "${fstype}" ] || [ "${fstype}" = "zfs_member" ]
}

# Unlock all configured or discovered LUKS volumes
#
# prints: prompts on the console
# returns: 0 if at least one volume was unlocked (or none needed unlocking)

luks_unlock_all() {
  local i dev spec name key opts unlocked failed disc
  local -a devs
  local -A seen

  # A retry from the import loop succeeds only if it unlocked something new;
  # reporting success when LUKS is unavailable or disabled would make the loop
  # retry the import forever without ever offering the recovery shell
  if ! luks_available; then
    zdebug "cryptsetup is not available; LUKS support disabled"
    [ -z "${LUKS_RETRY}" ]
    return
  fi

  if ! luks_load_config; then
    [ -z "${LUKS_RETRY}" ]
    return
  fi

  # Default key for volumes without one of their own
  local global_key
  global_key="$( get_zbm_arg zbm.luks_key )" || global_key=""

  udevadm settle >/dev/null 2>&1

  unlocked=0
  failed=0
  luks_forget_secrets

  if [ "${_luks_mode}" = "auto" ]; then
    mapfile -t devs < <( luks_discover_devices )

    # On retries, only volumes that appeared since the last pass matter
    if [ -n "${LUKS_RETRY}" ]; then
      local -a fresh=()
      for dev in "${devs[@]}"; do
        luks_retry_ignores "${dev}" || fresh+=( "${dev}" )
      done
      devs=( "${fresh[@]}" )
    fi

    if [ "${#devs[@]}" -eq 0 ]; then
      zdebug "no LUKS volumes found"
      [ -n "${LUKS_RETRY}" ] && return 1
      return 0
    fi

    zinfo "discovered LUKS volumes: ${devs[*]}"
    for dev in "${devs[@]}"; do
      if luks_open_device "" "${dev}" "${global_key}" "" 1; then
        unlocked=$(( unlocked + 1 ))
      else
        failed=$(( failed + 1 ))
      fi
    done
  else
    for i in "${!_luks_cfg_spec[@]}"; do
      spec="${_luks_cfg_spec[$i]}"
      name="${_luks_cfg_name[$i]}"
      key="${_luks_cfg_key[$i]:-${global_key}}"
      opts="${_luks_cfg_opts[$i]}"

      luks_parse_options "${opts}"
      if [ -n "${_luks_noauto}" ]; then
        zdebug "not unlocking ${spec} (noauto)"
        continue
      fi

      # Retries never wait: a device still missing is checked again next time
      if [ -n "${LUKS_RETRY}" ]; then
        dev="$( luks_resolve_device "${spec}" )" || continue
        luks_retry_ignores "${dev}" && continue
      elif ! dev="$( luks_wait_device "${spec}" "${_luks_nofail}" )"; then
        [ -n "${_luks_nofail}" ] || failed=$(( failed + 1 ))
        continue
      fi

      if [ -n "${seen[${dev}]}" ]; then
        zwarn "LUKS device ${dev} is configured more than once"
        continue
      fi
      seen[${dev}]=1

      if luks_open_device "${name}" "${dev}" "${key}" "${opts}" ""; then
        unlocked=$(( unlocked + 1 ))
      else
        failed=$(( failed + 1 ))
      fi
    done
  fi

  luks_forget_secrets

  # Let udev create /dev/disk/by-* links for the new mappings before import
  udevadm settle >/dev/null 2>&1

  # Automatically discovered volumes that clearly do not hold ZFS are closed
  if [ -r "${LUKS_STATE}" ]; then
    local -a keep=()
    while IFS=$'\t' read -r name dev disc; do
      [ -n "${name}" ] || continue
      if [ "${disc}" = "1" ] && [ -b "/dev/mapper/${name}" ] && ! luks_may_hold_zfs "${name}"; then
        zinfo "closing ${name}: ${dev} does not contain a ZFS vdev"
        if cryptsetup close "${name}" >/dev/null 2>&1; then
          unlocked=$(( unlocked - 1 ))
          continue
        fi
        zwarn "unable to close ${name}"
      fi
      keep+=( "${name}"$'\t'"${dev}"$'\t'"${disc}" )
    done < "${LUKS_STATE}"

    if [ "${#keep[@]}" -gt 0 ]; then
      printf '%s\n' "${keep[@]}" > "${LUKS_STATE}"
    else
      : > "${LUKS_STATE}"
    fi
  fi

  zinfo "LUKS: ${unlocked} volume(s) available, ${failed} not unlocked"

  # A retry succeeds only if it unlocked something new
  if [ -n "${LUKS_RETRY}" ]; then
    [ "${unlocked}" -gt 0 ]
    return
  fi

  [ "${failed}" -eq 0 ] || [ "${unlocked}" -gt 0 ]
}

# arg1: block device (e.g., /dev/mapper/name)
# prints: names of imported pools with a vdev (data, log, cache, special or
#         spare) on the device or on one of its partitions
# returns: nothing

luks_pools_on_device() {
  local target pool

  target="$( readlink -f "${1}" 2>/dev/null )" || return 0
  [ -n "${target}" ] || return 0

  # zpool status -L -P prints resolved paths such as /dev/dm-3; a vdev on a
  # partition of the mapping shows up as /dev/dm-3p1
  while read -r pool; do
    [ -n "${pool}" ] || continue
    if zpool status -L -P "${pool}" 2>/dev/null \
        | awk -v t="${target}" '
            { for (i = 1; i <= NF; i++) if ($i == t || ($i ~ "^" t "p[0-9]+$")) f = 1 }
            END { exit !f }'; then
      echo "${pool}"
    fi
  done <<< "$( zpool list -H -o name 2>/dev/null )"
}

# arg1: optional mapping name; when omitted, close every recorded mapping
# prints: nothing
# returns: 0 if all requested mappings were closed

luks_close_all() {
  local only name dev disc pool ret
  local -a keep=()

  only="${1}"
  ret=0

  [ -r "${LUKS_STATE}" ] || return 0
  luks_available || return 0

  while IFS=$'\t' read -r name dev disc; do
    [ -n "${name}" ] || continue

    if [ -n "${only}" ] && [ "${only}" != "${name}" ]; then
      keep+=( "${name}"$'\t'"${dev}"$'\t'"${disc}" )
      continue
    fi

    [ -b "/dev/mapper/${name}" ] || continue

    # Pools must be exported before their backing devices disappear
    while read -r pool; do
      [ -n "${pool}" ] || continue
      zdebug "exporting ${pool} before closing ${name}"
      if ! zpool export "${pool}" >/dev/null 2>&1; then
        zwarn "unable to export ${pool}, which uses ${name}"
      fi
    done <<< "$( luks_pools_on_device "/dev/mapper/${name}" )"

    if cryptsetup close "${name}" >/dev/null 2>&1; then
      zinfo "closed LUKS mapping ${name}"
    else
      zwarn "unable to close LUKS mapping ${name}"
      keep+=( "${name}"$'\t'"${dev}"$'\t'"${disc}" )
      ret=1
    fi
  done < "${LUKS_STATE}"

  if [ "${#keep[@]}" -gt 0 ]; then
    printf '%s\n' "${keep[@]}" > "${LUKS_STATE}"
  else
    : > "${LUKS_STATE}"
  fi

  return "${ret}"
}
