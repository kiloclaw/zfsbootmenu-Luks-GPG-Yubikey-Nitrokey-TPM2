#!/bin/bash
# shellcheck disable=SC2034  # test harness: variables are read by the sourced libraries; ls of the private /dev/mapper is intended
# Unit suite: core unlocking and teardown of zfsbootmenu-luks.sh, with real
# LUKS2 headers on loop devices and stubbed device-mapper and zpool.
# Usage: unlock.sh [ZFSBootMenu source tree]   (root; see docs/luks/testing.rst)
# shellcheck source=isolate.sh
source "$(dirname "$0")/isolate.sh"
# shellcheck source=impl.sh
source "$(dirname "$0")/impl.sh"; zbm_impl_check
REPO="${1:-$(cd "$(dirname "$0")/../../.." && pwd)}"
S="$(cd "$(dirname "$0")" && pwd)"
IMG="${ZBMTEST_UNIT_WORK:-/var/tmp/zbm-luks-unit}/unlock"; mkdir -p "$IMG"
PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); echo "  ok   - $*"; }
bad()  { FAIL=$((FAIL+1)); echo "  FAIL - $*"; }
check(){ local m="$1"; shift; if "$@"; then ok "$m"; else bad "$m"; fi; }

mkluks() { # name pass
  truncate -s 32M "$IMG/$1.img"
  printf '%s' "$2" | cryptsetup luksFormat -q --type luks2 --pbkdf pbkdf2 \
    --pbkdf-force-iterations 1000 --label "$1" --key-file=- "$IMG/$1.img"
  losetup -f --show "$IMG/$1.img"
}
DEV_A=$(mkluks zfsA secret1); DEV_B=$(mkluks zfsB secret1); DEV_C=$(mkluks extC other)
truncate -s 16M "$IMG/plain.img"; DEV_D=$(losetup -f --show "$IMG/plain.img")
# key device: ext4 holding /keys/zfsA.key == "secret1"
truncate -s 16M "$IMG/key.img"; mkfs.ext4 -q -L ZBMKEY "$IMG/key.img"
DEV_K=$(losetup -f --show "$IMG/key.img"); mkdir -p "$IMG/m"; mount "$DEV_K" "$IMG/m"
mkdir -p "$IMG/m/keys"; printf 'secret1' > "$IMG/m/keys/zfsA.key"; umount "$IMG/m"
UUID_A=$(cryptsetup luksUUID "$DEV_A")
echo "A=$DEV_A B=$DEV_B C=$DEV_C D=$DEV_D K=$DEV_K"

cleanup() {
  rm -f /dev/mapper/luks-* /dev/mapper/z* /dev/mapper/ext* 2>/dev/null
  for d in "$DEV_A" "$DEV_B" "$DEV_C" "$DEV_D" "$DEV_K"; do losetup -d "$d"; done
}
trap cleanup EXIT

# Run a scenario in a subshell with fresh state.
#   $1 description, $2 function setting up + calling code; stdin -> console
run() {
  export FAKE; FAKE="$(mktemp -d)"; mkdir -p "$FAKE"/{holders,maps,content,pools,busy}
  rm -f /dev/mapper/* 2>/dev/null
  echo "zfs_member" > "$FAKE/content/${DEV_A##*/}"
  echo "zfs_member" > "$FAKE/content/${DEV_B##*/}"
  echo "ext4"       > "$FAKE/content/${DEV_C##*/}"
  echo "== $1"
  (
    export PATH="$S/stubs:$PATH" BASE="$FAKE/base"
    mkdir -p "$BASE/cmdline.d"
    : > "$FAKE/prompts"
    zinfo()  { echo "INFO: $*"  >> "$FAKE/log"; }
    zwarn()  { echo "WARN: $*"  >> "$FAKE/log"; }
    zerror() { echo "ERROR: $*" >> "$FAKE/log"; }
    zdebug() { [ $# -gt 0 ] && echo "DEBUG: $*" >> "$FAKE/log"; return 0; }
    colorize() { shift; echo -ne "$@"; }
    # timed_prompt answers come from $FAKE/answers (0 = RETURN, 1 = ESC)
    timed_prompt() {
      echo "timed_prompt $*" >> "$FAKE/prompts"
      local a; a=$(head -n1 "$FAKE/answers" 2>/dev/null); sed -i 1d "$FAKE/answers" 2>/dev/null
      return "${a:-1}"
    }
    zbm_impl_load "$REPO" "$FAKE/crypttab" || exit 1
    # sysfs holders cannot be faked; consult the stub's record instead
    luks_is_open() { [ -e "$FAKE/holders/$(basename "$(readlink -f "$1")")" ]; }
    "$2"
  ) > "$FAKE/out" 2>&1
  RC=$?
  if [ -n "$DUMP" ] && [[ "$1" == *"$DUMP"* ]]; then echo "--- out"; cat "$FAKE/out"; echo "--- calls"; cat "$FAKE/calls"; echo "--- log"; cat "$FAKE/log"; fi
}
mapped() { [ -b "/dev/mapper/$1" ]; }
state_has() { grep -q "^$1	" "$FAKE/base/luks-mappings"; }
npw() { grep -c "Passphrase (leave empty" "$FAKE/out"; }

# ---------------------------------------------------------------- auto mode
t_auto() { luks_unlock_all; }
run "auto: shared passphrase prompts once; non-ZFS volume closed" t_auto <<< $'secret1\nother'
check "rc 0" [ "$RC" -eq 0 ]
check "A mapped as luks-UUID" mapped "luks-$UUID_A"
check "B mapped (cached passphrase)" mapped "luks-$(cryptsetup luksUUID "$DEV_B")"
check "C (ext4) closed again" bash -c "! ls /dev/mapper | grep -q $(cryptsetup luksUUID "$DEV_C")"
check "two passphrase prompts total (A, C)" [ "$(npw)" -eq 2 ]
check "state lists A" state_has "luks-$UUID_A"
check "state omits C" bash -c "! grep -q $(cryptsetup luksUUID "$DEV_C") $FAKE/base/luks-mappings"
check "plain device D never touched" bash -c "! grep -q 'open.*$DEV_D' $FAKE/calls"

run "auto: empty passphrase skips volumes" t_auto <<< $'\n\n\n'
check "nothing mapped" bash -c '[ -z "$(ls /dev/mapper)" ]'
check "skip logged" grep -q "skipping LUKS volume" "$FAKE/log"

run "auto: EOF on console fails gracefully" t_auto < /dev/null
check "nothing mapped" bash -c '[ -z "$(ls /dev/mapper)" ]'
check "no console logged" grep -q "no console available" "$FAKE/log"

t_retry() { echo 1 > "$FAKE/answers"; LUKS_DEFAULT_TRIES=2 luks_unlock_all; }
run "auto: wrong passphrases exhaust tries, ESC skips" t_retry <<< $'bad1\nbad2\n\n\n'
check "retry prompt shown" grep -q "Unable to unlock" "$FAKE/prompts"
check "error shown on 2nd try" grep -q "No key available" "$FAKE/out"
check "A not mapped" bash -c "! [ -b /dev/mapper/luks-$UUID_A ]"

t_retry2() { echo 0 > "$FAKE/answers"; LUKS_DEFAULT_TRIES=1 luks_unlock_all; }
run "auto: RETURN after failure retries and succeeds" t_retry2 <<< $'bad\nsecret1\nother'
check "A mapped after retry" mapped "luks-$UUID_A"

# ------------------------------------------------------------ cmdline mode
t_off() { echo off > "$BASE/cmdline.d/zbm.luks"; luks_unlock_all; }
run "zbm.luks=off disables everything" t_off < /dev/null
check "no cryptsetup open calls" bash -c "! grep -q 'cryptsetup open' $FAKE/calls"

t_cmd() { echo "UUID=$UUID_A,PARTLABEL=does-not-exist" > "$BASE/cmdline.d/zbm.luks"; echo 1 > "$FAKE/answers"; luks_unlock_all; }
run "zbm.luks=UUID=...,missing: unlock listed, prompt for missing" t_cmd <<< $'secret1'
check "A mapped" mapped "luks-$UUID_A"
check "B not mapped (not listed)" bash -c "! ls /dev/mapper | grep -q $(cryptsetup luksUUID "$DEV_B")"
check "missing device prompt" grep -q "does-not-exist is missing" "$FAKE/prompts"
check "rc 0 (one unlocked)" [ "$RC" -eq 0 ]

t_bare() { echo "luks-$UUID_A" > "$BASE/cmdline.d/zbm.luks"; luks_unlock_all; }
run "zbm.luks=luks-<uuid> accepted" t_bare <<< $'secret1'
check "A mapped" mapped "luks-$UUID_A"

t_gkey() { echo "$DEV_A" > "$BASE/cmdline.d/zbm.luks"; echo "/keys/zfsA.key:LABEL=ZBMKEY" > "$BASE/cmdline.d/zbm.luks_key"; luks_unlock_all; }
run "zbm.luks_key=path:LABEL= unlocks from key device" t_gkey < /dev/null
check "A mapped without prompt" mapped "luks-$UUID_A"
check "no passphrase prompt" [ "$(npw)" -eq 0 ]
check "key device unmounted" bash -c "! grep -q /tmp/luks-key /proc/mounts"

# ----------------------------------------------------------- crypttab mode
t_tab() {
  printf 'secret1' > "$FAKE/plainkey"; chmod 600 "$FAKE/plainkey"
  cat > "$ZBM_CRYPTTAB" <<-TAB
	# name      device              key                  options
	pool-a      UUID=$UUID_A        $FAKE/plainkey       discard,no-read-workqueue
	pool-b      $DEV_B              none                 tries=2
	ext-c       $DEV_C              -                    noauto
	gone        PARTLABEL=nothere   none                 nofail
	TAB
  luks_unlock_all
}
run "crypttab: key file, prompt w/ retry, noauto, nofail" t_tab <<< $'wrong\nsecret1'
check "pool-a mapped by keyfile" mapped pool-a
check "discard flag passed" grep -q "open --type luks --key-file=$FAKE/plainkey --allow-discards --perf-no_read_workqueue .*pool-a" "$FAKE/calls"
check "pool-b mapped on 2nd attempt" mapped pool-b
check "ext-c not opened (noauto)" bash -c "! [ -b /dev/mapper/ext-c ]"
check "nofail missing device: no prompt" bash -c "! grep -q nothere $FAKE/prompts"
check "keyfile-unlocked A pass NOT cached -> B prompted" [ "$(npw)" -eq 2 ]
check "rc 0" [ "$RC" -eq 0 ]

t_tab_ks() {
  printf '#!/bin/sh\n[ "$CRYPTTAB_NAME" = ks-a ] && printf secret1\n' > "$FAKE/ks"; chmod +x "$FAKE/ks"
  printf 'ks-a %s none keyscript=%s\n' "$DEV_A" "$FAKE/ks" > "$ZBM_CRYPTTAB"
  luks_unlock_all
}
run "crypttab: keyscript" t_tab_ks < /dev/null
check "ks-a mapped via keyscript" mapped ks-a

t_tab_badkey() { printf 'k %s /no/such/key\n' "$DEV_A" > "$ZBM_CRYPTTAB"; luks_unlock_all; }
run "crypttab: missing key file falls back to passphrase" t_tab_badkey <<< $'secret1'
check "k mapped" mapped k
check "warning logged" grep -q "key file /no/such/key not found" "$FAKE/log"

t_dup() { printf 'a1 %s\na2 UUID=%s\n' "$DEV_A" "$UUID_A" > "$ZBM_CRYPTTAB"; luks_unlock_all; }
run "crypttab: duplicate device detected" t_dup <<< $'secret1'
check "a1 mapped" mapped a1
check "a2 not mapped" bash -c "! [ -b /dev/mapper/a2 ]"
check "duplicate warning" grep -q "configured more than once" "$FAKE/log"

t_open_already() { printf 'a1 %s\n' "$DEV_A" > "$ZBM_CRYPTTAB"; luks_unlock_all <<< $'secret1'; luks_unlock_all < /dev/null; }
run "already-open device is skipped on second run" t_open_already
check "skip logged" grep -q "already in use" "$FAKE/log"
check "a1 recorded once" [ "$(grep -c '^a1' "$FAKE/base/luks-mappings")" -eq 1 ]

# --------------------------------------------------------------- teardown
t_close() {
  printf 'a1 %s\nb1 %s\n' "$DEV_A" "$DEV_B" > "$ZBM_CRYPTTAB"
  luks_unlock_all <<< $'secret1'
  echo /dev/mapper/a1 > "$FAKE/pools/zroot"
  echo /dev/mapper/b1 > "$FAKE/pools/tank"
  echo /dev/vda > "$FAKE/pools/other"
  luks_close_all
}
run "teardown: export LUKS pools, close mappings, leave others" t_close
check "zroot exported" grep -qx zroot "$FAKE/exported"
check "tank exported" grep -qx tank "$FAKE/exported"
check "other pool untouched" [ -e "$FAKE/pools/other" ]
check "a1 closed" bash -c "! [ -b /dev/mapper/a1 ]"
check "b1 closed" bash -c "! [ -b /dev/mapper/b1 ]"
check "state emptied" bash -c "! [ -s $FAKE/base/luks-mappings ]"

t_close_busy() {
  printf 'a1 %s\nb1 %s\n' "$DEV_A" "$DEV_B" > "$ZBM_CRYPTTAB"
  luks_unlock_all <<< $'secret1'
  touch "$FAKE/busy/b1"
  luks_close_all
}
run "teardown: busy mapping kept in state, rc 1" t_close_busy
check "rc 1" [ "$RC" -eq 1 ]
check "a1 closed" bash -c "! [ -b /dev/mapper/a1 ]"
check "b1 still recorded" grep -q '^b1' "$FAKE/base/luks-mappings"

t_close_one() {
  printf 'a1 %s\nb1 %s\n' "$DEV_A" "$DEV_B" > "$ZBM_CRYPTTAB"
  luks_unlock_all <<< $'secret1'
  luks_close_all b1
}
run "zluks close <name> closes only that mapping" t_close_one
check "a1 still open" mapped a1
check "b1 closed" bash -c "! [ -b /dev/mapper/b1 ]"

# ------------------------------------------------------------ resolution
t_resolve() {
  [ "$(luks_resolve_device "LABEL=zfsA")" = "$DEV_A" ] || return 1
  [ "$(luks_resolve_device "label=zfsA")" = "$DEV_A" ] || return 2
  [ "$(luks_resolve_device "$UUID_A")" = "$DEV_A" ] || return 3
  [ "$(luks_resolve_device "LABEL=ZBMKEY")" = "$DEV_K" ] || return 4
  ! luks_resolve_device "BOGUS=1" || return 5
  ! luks_resolve_device "notauuid" || return 6
  return 0
}
run "device spec resolution" t_resolve
check "LABEL/label/bare UUID resolve; bogus rejected (rc=$RC)" [ "$RC" -eq 0 ]

echo
echo "passed: $PASS failed: $FAIL"
[ "$FAIL" -eq 0 ]
