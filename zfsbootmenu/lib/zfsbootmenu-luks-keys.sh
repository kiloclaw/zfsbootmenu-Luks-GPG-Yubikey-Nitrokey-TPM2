#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

# Hardware-backed and encrypted keys for LUKS volumes
#
# This library is sourced by zfsbootmenu-luks.sh and provides:
#
#   - LUKS2 tokens:
#       systemd-tpm2   (systemd-cryptenroll --tpm2-device)
#       systemd-fido2  (systemd-cryptenroll --fido2-device; YubiKey, Nitrokey, ...)
#       clevis         (clevis luks bind ... tpm2|tang|sss)
#     systemd tokens are opened with the systemd cryptsetup token plugins when
#     they are present in the image; otherwise ZFSBootMenu falls back to its
#     own implementation built on tpm2-tools and fido2-assert.
#   - GnuPG-encrypted key files, protected by a passphrase or by an OpenPGP
#     key on a smart card (YubiKey, Nitrokey, ...)
#   - YubiKey HMAC-SHA1 challenge-response (compatible with yubikey-luks)
#
# Every function that opens a volume takes the mapping name, the LUKS device
# and the additional cryptsetup arguments, and returns 0 only if the volume is
# open afterwards.

# Include guard
[ -n "${_ZFSBOOTMENU_LUKS_KEYS}" ] && return
readonly _ZFSBOOTMENU_LUKS_KEYS=1

# Where the TPM lives; tests may point this at a simulator
: "${LUKS_TPM2_TCTI:=device:/dev/tpmrm0}"

# Persistent handle of the storage root key used by systemd >= 254
LUKS_TPM2_SRK_HANDLE="0x81000001"

# Public keys, trust database and configuration for GnuPG in the image
: "${LUKS_GNUPG_DIR:=/etc/zfsbootmenu/gnupg}"

# Seconds to wait for a security key or smart card to be plugged in
luks_token_wait() {
  local wait
  if wait="$( get_zbm_arg zbm.luks_token_wait )" && [ "${wait}" -ge 0 ] 2>/dev/null; then
    echo "${wait}"
  else
    echo 10
  fi
}

# prints: nothing
# returns: 0 unless token unlocking was disabled with zbm.luks_tokens=0

luks_tokens_enabled() {
  get_zbm_bool 1 zbm.luks_tokens
}

# arg1: prompt
# arg2: name of a variable that receives the secret
# prints: the prompt
# returns: 0 if a line was read, 1 on end of input
#
# Echo is turned off before the prompt is printed so no keystroke can echo.

luks_read_secret() {
  local -n _rs_out="${2}"
  local _rs_line _rs_ret

  stty -echo 2>/dev/null
  tput cnorm 2>/dev/null
  echo -n "${1}"
  IFS= read -r _rs_line
  _rs_ret=$?
  stty echo 2>/dev/null
  echo

  _rs_out="${_rs_line}"
  return "${_rs_ret}"
}

# arg1: seconds to wait
# arg2: message to show while waiting
# args: command that succeeds once the awaited device is present
# prints: the message
# returns: 0 when the device appeared, 1 on timeout or when a key was pressed

luks_wait_for() {
  local secs msg i
  secs="${1}"
  msg="${2}"
  shift 2

  "$@" && return 0
  [ "${secs}" -gt 0 ] 2>/dev/null || return 1

  echo -e "${msg}"
  echo "Waiting ${secs} seconds; press any key to skip."
  for (( i = 0; i < secs; i++ )); do
    if IFS= read -r -s -n 1 -t 1 _; then
      return 1
    fi
    if "$@"; then
      udevadm settle >/dev/null 2>&1
      return 0
    fi
  done

  return 1
}

# arg1: JSON text, arg2: key
# prints: the value of a string, array (contents, comma separated) or boolean
# returns: 0 if the key was found
#
# Token JSON as exported by cryptsetup is compact and keys are unique, which
# keeps these simple extractors reliable.

luks_json_str() {
  local v
  v="$( sed -n "s/.*\"${2}\": *\"\([^\"]*\)\".*/\1/p" <<< "${1}" )"
  [ -n "${v}" ] && echo "${v}"
}

luks_json_arr() {
  local v
  v="$( sed -n "s/.*\"${2}\": *\[\([^]]*\)\].*/\1/p" <<< "${1}" )" || return 1
  grep -q "\"${2}\": *\[" <<< "${1}" || return 1
  v="${v//\"/}"
  echo "${v// /}"
}

luks_json_bool() {
  local v
  v="$( sed -n "s/.*\"${2}\": *\(true\|false\).*/\1/p" <<< "${1}" )"
  [ -n "${v}" ] && echo "${v}"
}

# arg1: LUKS device (or detached header)
# prints: "<id> <type>" for every LUKS2 token
# returns: nothing

luks_list_tokens() {
  cryptsetup luksDump "${1}" 2>/dev/null | awk '
    /^Tokens:/ { t = 1; next }
    /^[^[:space:]]/ { t = 0 }
    t && /^  [0-9]+: / { sub(":", "", $1); print $1, $2 }
  '
}

# arg1: token type, e.g. systemd-tpm2
# prints: nothing
# returns: 0 if the cryptsetup token plugin for the type is installed

luks_have_token_plugin() {
  local f

  # Force the native implementations (debugging and tests)
  [ -n "${LUKS_NO_TOKEN_PLUGINS}" ] && return 1

  for f in /usr/lib/cryptsetup /usr/lib64/cryptsetup /usr/lib/*/cryptsetup \
      /lib/cryptsetup /lib64/cryptsetup /lib/*/cryptsetup; do
    [ -e "${f}/libcryptsetup-token-${1}.so" ] && return 0
  done
  return 1
}

# prints: nothing
# returns: 0 if a TPM2 is available, loading drivers if necessary

luks_have_tpm2() {
  case "${LUKS_TPM2_TCTI}" in
    device:*) ;;
    *) return 0 ;;
  esac

  [ -c "${LUKS_TPM2_TCTI#device:}" ] && return 0

  if [ -z "${_luks_tpm2_probed}" ]; then
    _luks_tpm2_probed=1
    modprobe -q tpm_crb >/dev/null 2>&1
    modprobe -q tpm_tis >/dev/null 2>&1
    udevadm settle >/dev/null 2>&1
  fi

  [ -c "${LUKS_TPM2_TCTI#device:}" ]
}

# prints: nothing
# returns: 0 if the TPM refuses PIN-protected objects after too many wrong
#          PINs (dictionary-attack lockout)

luks_tpm2_in_lockout() {
  command -v tpm2_getcap >/dev/null 2>&1 || return 1
  TPM2TOOLS_TCTI="${LUKS_TPM2_TCTI}" tpm2_getcap properties-variable 2>/dev/null \
    | grep -q 'inLockout: *1'
}

luks_tpm2_lockout_message() {
  colorize red "The TPM is locked against PIN guessing after too many wrong PINs.\n"
  colorize red "Use another unlock method; the lockout clears by itself over time.\n"
}

# prints: nothing
# returns: 0 if a FIDO2 (HID) device is connected

luks_have_fido2_device() {
  local d
  for d in /dev/hidraw*; do
    [ -c "${d}" ] && return 0
  done
  return 1
}

# arg1: mapping name
# arg2: LUKS device
# arg3: token id
# arg4: PIN, may be empty
# args: additional arguments for cryptsetup open
# prints: nothing
# returns: exit status of cryptsetup
#
# Opens a volume through a cryptsetup token plugin. cryptsetup reads a token
# PIN from stdin when it is not a terminal.

luks_open_token_plugin() {
  local name dev id pin output ret
  name="${1}"
  dev="${2}"
  id="${3}"
  pin="${4}"
  shift 4

  if [ -n "${pin}" ]; then
    output="$( printf '%s\n' "${pin}" \
      | cryptsetup open --type luks --token-only --token-id "${id}" "$@" "${dev}" "${name}" 2>&1 )"
  else
    output="$( cryptsetup open --type luks --token-only --token-id "${id}" "$@" "${dev}" "${name}" \
      < /dev/null 2>&1 )"
  fi
  ret=$?

  [ "${ret}" -eq 0 ] || zdebug "token ${id} on ${dev} failed (${ret}): ${output}"
  return "${ret}"
}

# arg1: token JSON
# arg2: PIN, may be empty
# arg3: file that receives the LUKS passphrase
# prints: nothing
# returns: 0 on success, 1 if unsealing failed, 2 if the token is unsupported
#
# Unseals a systemd-tpm2 token with tpm2-tools, mirroring systemd's
# tpm2_unseal(): load the sealed object below the SRK, satisfy the policy
# (PolicyPCR, then PolicyAuthValue when a PIN is set) and unseal. The LUKS
# passphrase is the base64 encoding of the unsealed secret.

luks_tpm2_native_unseal() {
  local json pin out work blob_b64 srk pcrs bank salt auth hexsalt
  local blob_size priv_size pub_size tool ret

  json="${1}"
  pin="${2}"
  out="${3}"

  for tool in tpm2_load tpm2_startauthsession tpm2_policypcr tpm2_policyauthvalue \
      tpm2_unseal tpm2_flushcontext base64 od sha256sum; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
      zdebug "native TPM2 unlocking needs ${tool}"
      return 2
    fi
  done

  # Signed PCR policies, pcrlock and pre-254 primary keys need the plugin
  if [ -n "$( luks_json_str "${json}" tpm2-pubkey )" ] \
      || [ "$( luks_json_bool "${json}" tpm2_pcrlock )" = "true" ]; then
    zdebug "TPM2 token uses a signed or pcrlock policy"
    return 2
  fi

  if ! srk="$( luks_json_str "${json}" tpm2_srk )" || [ -z "${srk}" ]; then
    zdebug "TPM2 token predates systemd's persistent SRK"
    return 2
  fi

  blob_b64="$( luks_json_str "${json}" tpm2-blob )" || return 2
  pcrs="$( luks_json_arr "${json}" tpm2-pcrs )" || pcrs=""
  bank="$( luks_json_str "${json}" tpm2-pcr-bank )" || bank="sha256"

  if ! work="$( mktemp -d /run/zbm-tpm2.XXXXXX 2>/dev/null || mktemp -d /tmp/zbm-tpm2.XXXXXX )"; then
    return 1
  fi
  chmod 700 "${work}"

  # The blob holds a TPM2B_PRIVATE followed by a TPM2B_PUBLIC; anything after
  # those is an import seed, which only the plugin handles
  base64 -d > "${work}/blob" <<< "${blob_b64}" 2>/dev/null
  blob_size="$( wc -c < "${work}/blob" )"
  priv_size="$( od -An -tu1 -N2 "${work}/blob" | awk '{ print $1 * 256 + $2 + 2 }' )"
  pub_size="$( od -An -tu1 -j "${priv_size}" -N2 "${work}/blob" | awk '{ print $1 * 256 + $2 + 2 }' )"

  if [ -z "${priv_size}" ] || [ -z "${pub_size}" ] \
      || [ "$(( priv_size + pub_size ))" -ne "${blob_size}" ]; then
    zdebug "TPM2 blob has an unsupported layout"
    rm -rf "${work}"
    return 2
  fi

  head -c "${priv_size}" "${work}/blob" > "${work}/priv"
  tail -c "+$(( priv_size + 1 ))" "${work}/blob" > "${work}/pub"

  if [ -n "${pin}" ]; then
    # Newer systemd salts the PIN: PBKDF2-HMAC-SHA256, 10000 rounds, base64
    if salt="$( luks_json_str "${json}" tpm2_salt )" && [ -n "${salt}" ]; then
      if ! command -v openssl >/dev/null 2>&1; then
        zdebug "salted TPM2 PINs need openssl"
        rm -rf "${work}"
        return 2
      fi
      hexsalt="$( base64 -d <<< "${salt}" | od -An -tx1 | tr -d ' \n' )"
      pin="$( openssl kdf -keylen 32 -kdfopt digest:SHA256 -kdfopt "pass:${pin}" \
        -kdfopt "hexsalt:${hexsalt}" -kdfopt iter:10000 -binary PBKDF2 2>/dev/null \
        | base64 | tr -d '\n' )"
      if [ -z "${pin}" ]; then
        rm -rf "${work}"
        return 1
      fi
    fi

    # authValue = SHA-256 of the PIN with trailing zero bytes removed
    auth="$( printf '%s' "${pin}" | sha256sum | cut -c1-64 )"
    while [ "${auth%00}" != "${auth}" ]; do
      auth="${auth%00}"
    done
  fi

  ret=1
  export TPM2TOOLS_TCTI="${LUKS_TPM2_TCTI}"

  if tpm2_load -C "${LUKS_TPM2_SRK_HANDLE}" -u "${work}/pub" -r "${work}/priv" \
        -c "${work}/obj.ctx" >/dev/null 2>&1 \
      && tpm2_startauthsession --policy-session -S "${work}/session.ctx" >/dev/null 2>&1; then
    if { [ -z "${pcrs}" ] || tpm2_policypcr -S "${work}/session.ctx" \
          -l "${bank:-sha256}:${pcrs}" >/dev/null 2>&1; } \
        && { [ -z "${pin}" ] || tpm2_policyauthvalue -S "${work}/session.ctx" >/dev/null 2>&1; } \
        && tpm2_unseal -c "${work}/obj.ctx" \
          -p "session:${work}/session.ctx${auth:++hex:${auth}}" > "${work}/secret" 2>/dev/null; then
      base64 < "${work}/secret" | tr -d '\n' > "${out}"
      ret=0
    fi
    tpm2_flushcontext "${work}/session.ctx" >/dev/null 2>&1
  fi
  tpm2_flushcontext -t >/dev/null 2>&1

  rm -rf "${work}"
  return "${ret}"
}

# arg1: token JSON
# arg2: file that receives the LUKS passphrase
# prints: instructions on the console
# returns: 0 on success, 1 if no device produced the secret, 2 if unsupported
#
# Computes the FIDO2 hmac-secret for a systemd-fido2 token with fido2-assert.
# As in systemd, the LUKS passphrase is the base64-encoded hmac-secret.

luks_fido2_native() {
  local json out cred salt rp pinreq up uv dev cdh hmac
  local -a args devs pinmodes

  json="${1}"
  out="${2}"

  command -v fido2-assert >/dev/null 2>&1 || return 2
  command -v fido2-token >/dev/null 2>&1 || return 2

  cred="$( luks_json_str "${json}" fido2-credential )" || return 2
  salt="$( luks_json_str "${json}" fido2-salt )" || return 2
  rp="$( luks_json_str "${json}" fido2-rp )" || rp="io.systemd.cryptsetup"
  pinreq="$( luks_json_bool "${json}" fido2-clientPin-required )" || pinreq=""
  up="$( luks_json_bool "${json}" fido2-up-required )" || up="true"
  uv="$( luks_json_bool "${json}" fido2-uv-required )" || uv="false"

  # Tokens enrolled by systemd 248 lack the PIN flag: try without, then with
  case "${pinreq}" in
    true) pinmodes=( true ) ;;
    false) pinmodes=( false ) ;;
    *) pinmodes=( false true ) ;;
  esac

  mapfile -t devs < <( fido2-token -L 2>/dev/null | sed -n 's/^\(\/dev\/[^:]*\):.*/\1/p' )
  [ "${#devs[@]}" -gt 0 ] || return 1

  for dev in "${devs[@]}"; do
    for pinreq in "${pinmodes[@]}"; do
      args=( -G -h -t "up=${up}" -t "uv=${uv}" )
      [ "${pinreq}" = "true" ] && args+=( -t "pin=true" )

      [ "${up}" = "true" ] && colorize yellow "Touch the security key ${dev} when it blinks.\n"

      cdh="$( head -c 32 /dev/urandom | base64 | tr -d '\n' )"
      if hmac="$( printf '%s\n%s\n%s\n%s\n' "${cdh}" "${rp}" "${cred}" "${salt}" \
          | fido2-assert "${args[@]}" "${dev}" 2>/dev/null | tail -n 1 )" && [ -n "${hmac}" ]; then
        printf '%s' "${hmac}" > "${out}"
        return 0
      fi
      zdebug "fido2-assert on ${dev} (pin=${pinreq}) did not produce a secret"
    done
  done

  return 1
}

# arg1: mapping name
# arg2: LUKS device
# arg3: token id
# arg4: token JSON
# args: additional arguments for cryptsetup open
# prints: prompts on the console
# returns: 0 if the volume was opened

luks_try_tpm2_token() {
  local name dev id json pin tries attempt ret secret cached work
  name="${1}"
  dev="${2}"
  id="${3}"
  json="${4}"
  shift 4

  if ! luks_have_tpm2; then
    zinfo "no TPM2 available for token ${id} on ${dev}"
    return 1
  fi

  # Without a PIN, unlocking needs no interaction
  if [ "$( luks_json_bool "${json}" tpm2-pin )" != "true" ]; then
    if luks_have_token_plugin systemd-tpm2; then
      luks_open_token_plugin "${name}" "${dev}" "${id}" "" "$@" && return 0
    fi

    work="$( mktemp /run/zbm-key.XXXXXX 2>/dev/null || mktemp /tmp/zbm-key.XXXXXX )" || return 1
    luks_tpm2_native_unseal "${json}" "" "${work}"
    ret=$?
    if [ "${ret}" -eq 0 ]; then
      luks_cryptsetup_open "${name}" "${dev}" "${work}" "$@" < /dev/null
      ret=$?
    elif [ "${ret}" -eq 2 ] && ! luks_have_token_plugin systemd-tpm2; then
      zwarn "TPM2 token ${id} on ${dev} needs the systemd-tpm2 cryptsetup plugin"
    fi
    rm -f "${work}"
    [ "${ret}" -eq 0 ] && zinfo "unlocked ${dev} with TPM2"
    return "${ret}"
  fi

  # PIN-protected: a PIN that already worked is tried once, then prompts.
  # Wrong PINs count towards the TPM's dictionary-attack lockout, so the
  # number of attempts is kept small and no PIN is asked for during lockout.
  if luks_tpm2_in_lockout; then
    echo
    luks_tpm2_lockout_message
    zwarn "TPM2 in dictionary-attack lockout; skipping PIN token ${id} on ${dev}"
    return 1
  fi

  tries=3
  attempt=0
  for cached in "${_luks_tpm2_pins[@]}" ""; do
    while :; do
      if [ -n "${cached}" ]; then
        pin="${cached}"
      else
        attempt=$(( attempt + 1 ))
        [ "${attempt}" -le "${tries}" ] || return 1
        echo
        colorize cyan "Unlock $( colorize yellow "${dev}" ) with the TPM\n"
        luks_read_secret "TPM2 PIN (leave empty to use another method): " pin || return 1
        [ -n "${pin}" ] || return 1
      fi

      ret=1
      if luks_have_token_plugin systemd-tpm2; then
        luks_open_token_plugin "${name}" "${dev}" "${id}" "${pin}" "$@" && ret=0
      else
        work="$( mktemp /run/zbm-key.XXXXXX 2>/dev/null || mktemp /tmp/zbm-key.XXXXXX )" || return 1
        luks_tpm2_native_unseal "${json}" "${pin}" "${work}"
        secret=$?
        if [ "${secret}" -eq 0 ]; then
          luks_cryptsetup_open "${name}" "${dev}" "${work}" "$@" < /dev/null && ret=0
        elif [ "${secret}" -eq 2 ]; then
          rm -f "${work}"
          zwarn "TPM2 token ${id} on ${dev} needs the systemd-tpm2 cryptsetup plugin"
          return 1
        fi
        rm -f "${work}"
      fi

      if [ "${ret}" -eq 0 ]; then
        [ -n "${cached}" ] || _luks_tpm2_pins+=( "${pin}" )
        pin=""
        zinfo "unlocked ${dev} with TPM2 and PIN"
        return 0
      fi
      pin=""

      if luks_tpm2_in_lockout; then
        luks_tpm2_lockout_message
        return 1
      fi

      # A cached PIN is only ever tried once
      [ -n "${cached}" ] && break
      colorize red "The TPM did not release the key (wrong PIN or changed PCRs).\n"
    done
  done

  return 1
}

# arg1: mapping name
# arg2: LUKS device
# arg3: token id
# arg4: token JSON
# args: additional arguments for cryptsetup open
# prints: prompts on the console
# returns: 0 if the volume was opened

luks_try_fido2_token() {
  local name dev id json pin ret work tries attempt
  name="${1}"
  dev="${2}"
  id="${3}"
  json="${4}"
  shift 4

  if ! luks_wait_for "$( luks_token_wait )" \
      "$( colorize cyan "Insert the FIDO2 security key (YubiKey, Nitrokey, ...) for $( colorize yellow "${dev}" )" )" \
      luks_have_fido2_device; then
    zinfo "no FIDO2 security key for token ${id} on ${dev}"
    return 1
  fi

  if luks_have_token_plugin systemd-fido2; then
    echo
    colorize cyan "Unlock $( colorize yellow "${dev}" ) with a FIDO2 security key\n"

    if [ "$( luks_json_bool "${json}" fido2-clientPin-required )" = "true" ]; then
      tries=3
      attempt=0
      while [ "${attempt}" -lt "${tries}" ]; do
        attempt=$(( attempt + 1 ))
        luks_read_secret "Security key PIN (leave empty to use another method): " pin || return 1
        [ -n "${pin}" ] || return 1
        [ "$( luks_json_bool "${json}" fido2-up-required )" = "false" ] \
          || colorize yellow "Touch the security key when it blinks.\n"
        if luks_open_token_plugin "${name}" "${dev}" "${id}" "${pin}" "$@"; then
          pin=""
          zinfo "unlocked ${dev} with FIDO2 security key"
          return 0
        fi
        pin=""
        colorize red "The security key did not release the key.\n"
      done
      return 1
    fi

    [ "$( luks_json_bool "${json}" fido2-up-required )" = "false" ] \
      || colorize yellow "Touch the security key when it blinks.\n"
    if luks_open_token_plugin "${name}" "${dev}" "${id}" "" "$@"; then
      zinfo "unlocked ${dev} with FIDO2 security key"
      return 0
    fi
    return 1
  fi

  # Without the plugin, fido2-assert asks for the PIN itself
  echo
  colorize cyan "Unlock $( colorize yellow "${dev}" ) with a FIDO2 security key\n"
  work="$( mktemp /run/zbm-key.XXXXXX 2>/dev/null || mktemp /tmp/zbm-key.XXXXXX )" || return 1
  luks_fido2_native "${json}" "${work}"
  ret=$?
  if [ "${ret}" -eq 0 ]; then
    luks_cryptsetup_open "${name}" "${dev}" - "$@" < "${work}"
    ret=$?
  elif [ "${ret}" -eq 2 ]; then
    zwarn "FIDO2 token ${id} on ${dev} needs fido2-tools or the systemd-fido2 plugin"
  fi
  rm -f "${work}"

  [ "${ret}" -eq 0 ] && zinfo "unlocked ${dev} with FIDO2 security key"
  return "${ret}"
}

# arg1: mapping name
# arg2: LUKS device
# arg3: token id
# arg4: token JSON
# args: additional arguments for cryptsetup open
# prints: nothing
# returns: 0 if the volume was opened

luks_try_clevis_token() {
  local name dev id json slot ret work
  name="${1}"
  dev="${2}"
  id="${3}"
  json="${4}"
  shift 4

  if ! command -v clevis >/dev/null 2>&1; then
    zinfo "clevis token ${id} on ${dev} needs clevis in the image"
    return 1
  fi

  if ! slot="$( luks_json_arr "${json}" keyslots )" || [ -z "${slot}" ]; then
    return 1
  fi
  slot="${slot%%,*}"

  # Load TPM drivers in case the clevis policy uses the TPM
  luks_have_tpm2 >/dev/null 2>&1

  work="$( mktemp /run/zbm-key.XXXXXX 2>/dev/null || mktemp /tmp/zbm-key.XXXXXX )" || return 1
  if TPM2TOOLS_TCTI="${LUKS_TPM2_TCTI}" clevis luks pass -d "${dev}" -s "${slot}" \
      > "${work}" 2>/dev/null && [ -s "${work}" ]; then
    luks_cryptsetup_open "${name}" "${dev}" - "$@" < "${work}"
    ret=$?
  else
    zdebug "clevis could not recover the key for slot ${slot} on ${dev}"
    ret=1
  fi
  rm -f "${work}"

  [ "${ret}" -eq 0 ] && zinfo "unlocked ${dev} with clevis"
  return "${ret}"
}

# arg1: mapping name
# arg2: LUKS device
# arg3: LUKS header (the device itself unless detached)
# args: additional arguments for cryptsetup open
# prints: prompts on the console
# returns: 0 if a token opened the volume
#
# Tokens that need no interaction (TPM2 without PIN, clevis) are tried
# before those that do (TPM2 with PIN, FIDO2).

luks_try_tokens() {
  local name dev header id type json pass
  local -a tokens

  name="${1}"
  dev="${2}"
  header="${3}"
  shift 3

  mapfile -t tokens < <( luks_list_tokens "${header}" )
  [ "${#tokens[@]}" -gt 0 ] || return 1

  for pass in quiet interactive; do
    for id in "${tokens[@]}"; do
      type="${id#* }"
      id="${id%% *}"
      json="$( cryptsetup token export --token-id "${id}" "${header}" 2>/dev/null )" || continue

      case "${pass}:${type}" in
        quiet:systemd-tpm2)
          [ "$( luks_json_bool "${json}" tpm2-pin )" = "true" ] && continue
          luks_try_tpm2_token "${name}" "${dev}" "${id}" "${json}" "$@" && return 0
          ;;
        quiet:clevis)
          luks_try_clevis_token "${name}" "${dev}" "${id}" "${json}" "$@" && return 0
          ;;
        interactive:systemd-tpm2)
          [ "$( luks_json_bool "${json}" tpm2-pin )" = "true" ] || continue
          luks_try_tpm2_token "${name}" "${dev}" "${id}" "${json}" "$@" && return 0
          ;;
        interactive:systemd-fido2)
          luks_try_fido2_token "${name}" "${dev}" "${id}" "${json}" "$@" && return 0
          ;;
        interactive:*)
          # Any other token type that has a plugin in the image
          case "${type}" in
            systemd-tpm2|systemd-fido2|clevis) ;;
            *)
              if luks_have_token_plugin "${type}"; then
                luks_open_token_plugin "${name}" "${dev}" "${id}" "" "$@" && return 0
              else
                zdebug "no support for ${type} token ${id} on ${dev}"
              fi
              ;;
          esac
          ;;
      esac
    done
  done

  return 1
}

# arg1: key file
# arg2: crypttab options
# prints: nothing
# returns: 0 if the key file is GnuPG-encrypted

luks_is_gpg_keyfile() {
  case ",${2}," in
    *,gpg,*) return 0 ;;
  esac

  case "${1}" in
    *.gpg|*.asc|*.pgp) return 0 ;;
  esac

  return 1
}

# prints: path of a private GnuPG home (also exported as GNUPGHOME)
# returns: 0 on success
#
# The image's public keyring is copied to a short path in /run because the
# agent's sockets must fit in a sockaddr_un.

luks_gpg_setup() {
  local home
  command -v gpg >/dev/null 2>&1 || return 1

  mkdir -p /run
  home="$( mktemp -d /run/zbm-gpg.XXXXXX 2>/dev/null )" || return 1
  chmod 700 "${home}"

  if [ -d "${LUKS_GNUPG_DIR}" ]; then
    cp -a "${LUKS_GNUPG_DIR}/." "${home}/" 2>/dev/null
  fi

  # Start pcscd if the image has it, and prefer it over GnuPG's internal CCID
  # driver once it runs
  if command -v pcscd >/dev/null 2>&1 && [ ! -S /run/pcscd/pcscd.comm ]; then
    mkdir -p /run/pcscd
    pcscd >/dev/null 2>&1 && sleep 1
  fi
  if [ -S /run/pcscd/pcscd.comm ]; then
    printf 'disable-ccid\npcsc-shared\n' >> "${home}/scdaemon.conf"
  fi

  chmod -R go-rwx "${home}"
  export GNUPGHOME="${home}"
  echo "${home}"
}

# arg1: GnuPG home created by luks_gpg_setup
# prints: nothing
# returns: nothing

luks_gpg_cleanup() {
  [ -n "${1}" ] || return 0
  GNUPGHOME="${1}" gpgconf --kill all >/dev/null 2>&1
  rm -rf "${1}"
  unset GNUPGHOME
}

# prints: "<serial> <PIN tries left>" for the card that is plugged in
# returns: 0 if an OpenPGP card responded
#
# gpg --card-status also creates stubs for the card's keys in the keyring,
# provided their public keys are known.

luks_gpg_card_status() {
  local status serial tries
  status="$( gpg --batch --no-tty --with-colons --card-status 2>/dev/null )" || return 1
  serial="$( awk -F: '$1 == "serial" { print $2; exit }' <<< "${status}" )"
  # chvstatus:<once>:<max PIN len>:<max reset len>:<max admin len>:<PIN tries>:...
  tries="$( awk -F: '$1 == "chvstatus" { print $6; exit }' <<< "${status}" )"
  echo "${serial:-unknown} ${tries:-?}"
}

# arg1: GnuPG-encrypted file
# prints: the long key IDs the file is encrypted to (none if symmetric)
# returns: nothing

luks_gpg_recipients() {
  gpg --batch --no-tty --list-only --list-packets "${1}" 2>/dev/null \
    | sed -n 's/^:pubkey enc packet:.* keyid \([0-9A-Fa-f]*\).*/\1/p'
}

# arg1: long key ID
# prints: "card" if the secret key lives on a smart card, "soft" otherwise
# returns: 0 if a secret key (or card stub) for the ID is available

luks_gpg_secret_key() {
  local token
  # Field 15 of sec/ssb records: "+" for a key stored locally, "#" for a stub
  # whose card is unavailable, otherwise the serial number of the card
  token="$( gpg --batch --no-tty --with-colons --list-secret-keys 2>/dev/null \
    | awk -F: -v id="${1}" 'toupper($5) == toupper(id) && ($1 == "sec" || $1 == "ssb") { print "=" $15; exit }' )"

  case "${token}" in
    "=+") echo soft ;;
    ""|"="|"=#") return 1 ;;
    *) echo card ;;
  esac
}

# prints: nothing
# returns: 0 if a secret key for any of the recipients in $_luks_gpg_rcpts is
#          available (after reading any card that is plugged in)

luks_gpg_have_key() {
  local id
  luks_gpg_card_status >/dev/null 2>&1
  for id in ${_luks_gpg_rcpts}; do
    luks_gpg_secret_key "${id}" >/dev/null && return 0
  done
  return 1
}

# arg1: GnuPG-encrypted key file
# arg2: output file
# arg3: description of the volume, for prompts
# prints: prompts on the console
# returns: 0 if the key was decrypted
#
# The passphrase or card PIN is entered here and handed to gpg through a file
# descriptor (loopback pinentry). Card PINs are only requested once the card
# is known to hold a matching key, and the remaining PIN attempts are shown,
# so no attempt is wasted.

luks_gpg_decrypt() {
  local file out desc home id kind info serial left secret attempt status
  local -a gpgargs

  file="${1}"
  out="${2}"
  desc="${3}"

  if ! home="$( luks_gpg_setup )"; then
    zwarn "GnuPG is not available to decrypt ${file}"
    return 1
  fi
  export GNUPGHOME="${home}"

  gpgargs=( --batch --no-tty --yes --pinentry-mode loopback --passphrase-fd 3
    --status-fd 2 --output "${out}" --decrypt "${file}" )

  _luks_gpg_rcpts="$( luks_gpg_recipients "${file}" | tr '\n' ' ' )"

  if [ -n "${_luks_gpg_rcpts// /}" ]; then
    # Encrypted to a public key: find the card (or key) that can decrypt it
    if ! luks_wait_for "$( luks_token_wait )" \
        "$( colorize cyan "Insert the OpenPGP security key (YubiKey, Nitrokey, ...) for $( colorize yellow "${desc}" )" )" \
        luks_gpg_have_key; then
      zinfo "no secret key for ${file} (encrypted to ${_luks_gpg_rcpts})"
      luks_gpg_cleanup "${home}"
      return 1
    fi

    kind="soft"
    for id in ${_luks_gpg_rcpts}; do
      kind="$( luks_gpg_secret_key "${id}" )" && break
    done
  else
    kind="symmetric"
  fi

  attempt=0
  while [ "${attempt}" -lt 3 ]; do
    attempt=$(( attempt + 1 ))
    echo
    case "${kind}" in
      card)
        info="$( luks_gpg_card_status )" || info="unknown ?"
        serial="${info%% *}"
        left="${info##* }"
        colorize cyan "Unlock $( colorize yellow "${desc}" ) with OpenPGP card ${serial}\n"
        if [ "${left}" = "0" ]; then
          colorize red "The card PIN is blocked; unblock it with the admin PIN or reset code.\n"
          break
        fi
        luks_read_secret "Card PIN (${left} attempts left; leave empty to use another method): " secret || break
        ;;
      soft)
        colorize cyan "Unlock $( colorize yellow "${desc}" ) with GnuPG key file ${file}\n"
        luks_read_secret "Secret key passphrase (leave empty to use another method): " secret || break
        ;;
      *)
        colorize cyan "Unlock $( colorize yellow "${desc}" ) with GnuPG key file ${file}\n"
        luks_read_secret "Key file passphrase (leave empty to use another method): " secret || break
        ;;
    esac

    [ -n "${secret}" ] || break

    [ "${kind}" = "card" ] && colorize yellow "Touch the security key if it blinks.\n"

    status="$( gpg "${gpgargs[@]}" 3<<< "${secret}" 2>&1 >/dev/null )"
    secret=""

    if grep -q '^\[GNUPG:\] DECRYPTION_OKAY' <<< "${status}" && [ -s "${out}" ]; then
      luks_gpg_cleanup "${home}"
      return 0
    fi

    rm -f "${out}"
    zdebug "gpg failed to decrypt ${file}: ${status}"
    colorize red "Decryption failed.\n"
  done

  luks_gpg_cleanup "${home}"
  return 1
}

# arg1: mapping name
# arg2: LUKS device
# args: additional arguments for cryptsetup open
# prints: prompts on the console
# returns: 0 if the volume was opened
#
# YubiKey HMAC-SHA1 challenge-response in the style of yubikey-luks: the typed
# passphrase (optionally its SHA-256) is the challenge, and the response
# (optionally appended to the challenge) is the LUKS passphrase. Configured by
# the options ykchalresp[=slot], ykchalresp-hash and ykchalresp-concat.

luks_try_ykchalresp() {
  local name dev pass challenge response attempt
  name="${1}"
  dev="${2}"
  shift 2

  if ! command -v ykchalresp >/dev/null 2>&1; then
    zwarn "ykchalresp is not available to unlock ${dev}"
    return 1
  fi

  attempt=0
  while [ "${attempt}" -lt "${_luks_tries:-3}" ] || [ "${_luks_tries}" -eq 0 ]; do
    attempt=$(( attempt + 1 ))

    if ! luks_wait_for "$( luks_token_wait )" \
        "$( colorize cyan "Insert the YubiKey for $( colorize yellow "${dev}" )" )" \
        ykchalresp_present; then
      zinfo "no YubiKey present for ${dev}"
      return 1
    fi

    echo
    colorize cyan "Unlock $( colorize yellow "${dev}" ) with YubiKey challenge-response (slot ${_luks_yk_slot:-2})\n"
    luks_read_secret "Passphrase (leave empty to use another method): " pass || return 1
    [ -n "${pass}" ] || return 1

    challenge="${pass}"
    if [ -n "${_luks_yk_hash}" ]; then
      challenge="$( printf '%s' "${pass}" | sha256sum | cut -c1-64 )"
    fi

    colorize yellow "Touch the YubiKey if it blinks.\n"
    # The challenge goes through stdin so it never appears in the process list
    if ! response="$( printf '%s' "${challenge}" | ykchalresp "-${_luks_yk_slot:-2}" -i- 2>/dev/null )" \
        || [ -z "${response}" ]; then
      pass=""
      challenge=""
      colorize red "The YubiKey did not respond.\n"
      continue
    fi

    if [ -n "${_luks_yk_concat}" ]; then
      response="${challenge}${response}"
    fi
    pass=""
    challenge=""

    if printf '%s' "${response}" | luks_cryptsetup_open "${name}" "${dev}" - "$@"; then
      response=""
      zinfo "unlocked ${dev} with YubiKey challenge-response"
      return 0
    fi
    response=""
    colorize red "No key available with this passphrase.\n"
  done

  return 1
}

# prints: nothing
# returns: 0 if a YubiKey is connected (always 0 when ykinfo is unavailable,
#          in which case ykchalresp itself reports a missing key)

ykchalresp_present() {
  command -v ykinfo >/dev/null 2>&1 || return 0
  ykinfo -s >/dev/null 2>&1
}
