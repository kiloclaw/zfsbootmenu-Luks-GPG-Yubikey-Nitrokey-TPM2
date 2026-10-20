#!/bin/bash
# shellcheck disable=SC2010,SC2034,SC2155  # test harness: variables are read by the sourced libraries; ls of the private /dev/mapper is intended
# Unit suite: failure paths, races, token listing and teardown matching.
# Usage: edge.sh [ZFSBootMenu source tree]   (root; see docs/luks/testing.rst)
# shellcheck source=isolate.sh
source "$(dirname "$0")/isolate.sh"
REPO="${1:-$(cd "$(dirname "$0")/../../.." && pwd)}"; S="$(cd "$(dirname "$0")" && pwd)"
K=/tmp/ze; rm -rf "$K"; mkdir -p "$K"
PASS=0; FAIL=0
ok(){ PASS=$((PASS+1)); echo "  ok   - $*"; }; bad(){ FAIL=$((FAIL+1)); echo "  FAIL - $*"; }
check(){ local m="$1"; shift; if "$@"; then ok "$m"; else bad "$m"; fi; }

mkdir -p "$K/tpm"; swtpm_setup --tpm2 --tpmstate "$K/tpm" --overwrite >/dev/null 2>&1
[ -f /tmp/ze-swtpm.pid ] && kill "$(cat /tmp/ze-swtpm.pid)" 2>/dev/null; sleep 0.5
swtpm socket --tpm2 --tpmstate dir="$K/tpm" --server type=tcp,port=2721 --ctrl type=tcp,port=2722 --flags not-need-init,startup-clear -d --pid file=/tmp/ze-swtpm.pid; sleep 1
TCTI="swtpm:host=127.0.0.1,port=2721"; export SYSTEMD_TPM2_DEVICE="$TCTI" TPM2TOOLS_TCTI="$TCTI"

mkluks(){ truncate -s 32M "$K/$1.img"; printf 'pw' | cryptsetup luksFormat -q --type "${2:-luks2}" --pbkdf pbkdf2 --pbkdf-force-iterations 1000 ${3:+--label "$1"} --key-file=- "$K/$1.img"; losetup -f --show "$K/$1.img"; }
addkey(){ printf 'pw' | cryptsetup luksAddKey -q --pbkdf pbkdf2 --pbkdf-force-iterations 1000 --key-file=- "$1" "$2"; }

STALE=$(mkluks stale luks2 L); PASSWORD=pw systemd-cryptenroll --tpm2-device="$TCTI" --tpm2-pcrs= "$STALE" >/dev/null 2>&1; cryptsetup luksKillSlot -q "$STALE" 1 <<< pw 2>/dev/null || printf 'pw' | cryptsetup luksKillSlot -q --key-file=- "$STALE" 1
MULTI=$(mkluks multi luks2 L); PASSWORD=pw systemd-cryptenroll --tpm2-device="$TCTI" --tpm2-pcrs= "$MULTI" >/dev/null 2>&1
export FIDO_CRED=$(head -c 64 /dev/urandom | base64 -w0) FIDO_SALT=$(head -c 32 /dev/urandom | base64 -w0) FIDO_HMAC=$(head -c 32 /dev/urandom | base64 -w0)
printf '%s' "$FIDO_HMAC" > "$K/fp"; addkey "$MULTI" "$K/fp"
printf '{"type":"systemd-fido2","keyslots":["2"],"fido2-credential":"%s","fido2-salt":"%s","fido2-rp":"io.systemd.cryptsetup","fido2-clientPin-required":false,"fido2-up-required":true,"fido2-uv-required":false}' "$FIDO_CRED" "$FIDO_SALT" > "$K/f.json"; cryptsetup token import --json-file "$K/f.json" "$MULTI"
FIDOONLY=$(mkluks fidoonly luks2 L); addkey "$FIDOONLY" "$K/fp"; sed 's/"keyslots":\["2"\]/"keyslots":["1"]/' "$K/f.json" > "$K/f1.json"; cryptsetup token import --json-file "$K/f1.json" "$FIDOONLY"
TOK=$(mkluks tok luks2 L); PASSWORD=pw systemd-cryptenroll --tpm2-device="$TCTI" --tpm2-pcrs= "$TOK" >/dev/null 2>&1
L1=$(mkluks one luks1)
PLAIN=$(mkluks plain luks2 L)
truncate -s 32M "$K/hdrdata.img"; truncate -s 16M "$K/hdr"; HDRDATA=$(losetup -f --show "$K/hdrdata.img"); printf 'pw' | cryptsetup luksFormat -q --type luks2 --header "$K/hdr" --pbkdf pbkdf2 --pbkdf-force-iterations 1000 --key-file=- "$HDRDATA"
head -c 64 /dev/urandom > "$K/good.key"; GK=$(mkluks gk luks2 L); addkey "$GK" "$K/good.key"; head -c 64 /dev/urandom > "$K/wrong.key"
export GNUPGHOME="$K/gen"; mkdir -m 700 "$GNUPGHOME"
for n in a b; do gpg -q --batch --pinentry-mode loopback --passphrase "pass$n" --quick-gen-key "K$n <$n@t>" ed25519 sign never; done
FA=$(gpg --list-keys --with-colons a@t | awk -F: '/^fpr/{print $10; exit}'); FB=$(gpg --list-keys --with-colons b@t | awk -F: '/^fpr/{print $10; exit}')
gpg -q --batch --pinentry-mode loopback --passphrase passa --quick-add-key "$FA" cv25519 encr never
gpg -q --batch --pinentry-mode loopback --passphrase passb --quick-add-key "$FB" cv25519 encr never
gpg -q --batch --trust-model always -r "$FA" -r "$FB" --encrypt -o "$K/good.key.gpg" "$K/good.key"
mkdir -m 700 "$K/onlyb"; gpg --batch --pinentry-mode loopback --passphrase passb --export-secret-keys "$FB" | GNUPGHOME="$K/onlyb" gpg -q --batch --pinentry-mode loopback --passphrase passb --import; gpg --batch --export "$FA" | GNUPGHOME="$K/onlyb" gpg -q --batch --import
gpg -q --batch --pinentry-mode loopback --passphrase symp --symmetric -o "$K/good.key.sym.gpg" "$K/good.key"
gpgconf --kill all; GNUPGHOME="$K/onlyb" gpgconf --kill all; unset GNUPGHOME
printf '#!/bin/sh\nprintf pw\n' > "$K/ks"; chmod +x "$K/ks"
[ -e /dev/hidraw99 ] || mknod /dev/hidraw99 c 1 3
cleanup(){ rm -f /dev/mapper/* 2>/dev/null; detach_own_loops "$K"; rm -f /dev/hidraw99; kill "$(cat /tmp/ze-swtpm.pid)" 2>/dev/null; }
trap cleanup EXIT

run(){ export FAKE; FAKE="$(mktemp -d)"; mkdir -p "$FAKE"/{holders,maps,content,pools,busy}; rm -f /dev/mapper/* 2>/dev/null
  for d in $STALE $MULTI $FIDOONLY $TOK $L1 $PLAIN $HDRDATA $GK $CLV2; do echo zfs_member > "$FAKE/content/${d##*/}"; done
  echo "== $1"
  ( export PATH="$S/kstubs:$S/stubs:$PATH" BASE="$FAKE/base" LUKS_TPM2_TCTI="$TCTI" ANSWERS="$FAKE/answers"
    mkdir -p "$BASE/cmdline.d"; echo 0 > "$BASE/cmdline.d/zbm.luks_token_wait"
    zinfo(){ echo "INFO: $*" >> "$FAKE/log"; }; zwarn(){ echo "WARN: $*" >> "$FAKE/log"; }; zerror(){ echo "ERROR: $*" >> "$FAKE/log"; }
    zdebug(){ [ $# -gt 0 ] && echo "DEBUG: $*" >> "$FAKE/log"; return 0; }; colorize(){ shift; echo -ne "$@"; }
    timed_prompt(){ echo "timed_prompt $*" >> "$FAKE/prompts"; sleep 1; local a; a=$(head -n1 "$ANSWERS" 2>/dev/null); sed -i 1d "$ANSWERS" 2>/dev/null; return "${a:-1}"; }
    _ZFSBOOTMENU_CORE=1; source "$REPO/legacy/zfsbootmenu/lib/zfsbootmenu-kcl.sh"
    ZBM_CRYPTTAB="$FAKE/crypttab"; LUKS_KEYS_LIB="$REPO/legacy/zfsbootmenu/lib/zfsbootmenu-luks-keys.sh"; source "$REPO/legacy/zfsbootmenu/lib/zfsbootmenu-luks.sh"
    luks_is_open(){ [ -e "$FAKE/holders/$(basename "$(readlink -f "$1")")" ]; }
    "$2" ) > "$FAKE/out" 2>&1; RC=$?
  N=$((N+1)); mkdir -p "$K/cases"; { echo "### $1 (rc=$RC)"; cat "$FAKE/out"; echo "--- calls"; cat "$FAKE/calls" 2>/dev/null; echo "--- log"; cat "$FAKE/log" 2>/dev/null; echo "--- prompts"; cat "$FAKE/prompts" 2>/dev/null; echo "--- mapper"; ls /dev/mapper; } > "$K/cases/$N.txt" 2>&1; }
mapped(){ [ -b "/dev/mapper/$1" ]; }; prompts(){ grep -c "$1" "$FAKE/out"; }; tab(){ printf '%s\n' "$@" > "$ZBM_CRYPTTAB"; }
noleft(){ ! ls /run/zbm-key.* /run/zbm-gpg.* /run/zbm-tpm2.* /tmp/zbm-key.* /tmp/zbm-tpm2.* 2>/dev/null | grep -q .; }

echo "### key management"
t(){ LUKS_NO_TOKEN_PLUGINS=1; tab "v $STALE none"; luks_unlock_all; }
run "TPM2 token whose key slot was removed -> passphrase fallback" t <<< $'pw'
check "mapped via passphrase" mapped v; check "TPM unseal attempted first" grep -q "key-file=/run/zbm-key" "$FAKE/calls"; check "no leftovers" noleft

t(){ LUKS_NO_TOKEN_PLUGINS=1; LUKS_TPM2_TCTI="device:/dev/no-such-tpm"; tab "v $MULTI none"; luks_unlock_all; }
run "TPM2 + FIDO2 tokens on one volume, no TPM present -> FIDO2 unlocks" t < /dev/null
check "mapped" mapped v; check "TPM skipped" grep -q "no TPM2 available" "$FAKE/log"; check "FIDO2 used" grep -q "with FIDO2" "$FAKE/log"

t(){ luks_list_tokens "$MULTI"; }
run "token list: TPM2 + FIDO2 tokens on one volume" t < /dev/null
check "token 0 systemd-tpm2" grep -qx "0 systemd-tpm2" "$FAKE/out"; check "token 1 systemd-fido2" grep -qx "1 systemd-fido2" "$FAKE/out"
# With systemd 261's token plugins, cryptsetup 2.8 prints the luksDump text of
# every token after the first to stderr. Emulate that and list the tokens.
mkdir -p "$K/stderrdump"
printf '%s\n' '#!/bin/bash' \
  'if [ "$1" = luksDump ] && [[ " $* " != *" --dump-json-metadata "* ]]; then' \
  '  /usr/sbin/cryptsetup "$@" | awk '"'"'/^  1: /{e=1} {if (e) print > "/dev/stderr"; else print}'"'"'; exit "${PIPESTATUS[0]}"; fi' \
  'exec /usr/sbin/cryptsetup "$@"' > "$K/stderrdump/cryptsetup"; chmod 755 "$K/stderrdump/cryptsetup"
t(){ PATH="$K/stderrdump:$PATH"; luks_list_tokens "$MULTI"; }
run "token list: luksDump prints later tokens to stderr (systemd 261 plugins)" t < /dev/null
check "token 0 found" grep -qx "0 systemd-tpm2" "$FAKE/out"; check "token 1 found despite stderr" grep -qx "1 systemd-fido2" "$FAKE/out"
t(){ luks_list_tokens "$L1"; }
run "token list: LUKS1 volume has no tokens" t < /dev/null
check "nothing listed" bash -c "[ ! -s '$FAKE/out' ]"
# three token types, then a gap in the token ids, and a detached header
THREE=$(mkluks three luks2 L); PASSWORD=pw systemd-cryptenroll --tpm2-device="$TCTI" --tpm2-pcrs= "$THREE" >/dev/null 2>&1
addkey "$THREE" "$K/fp"; sed 's/"keyslots":\["2"\]/"keyslots":["2"]/' "$K/f.json" > "$K/f3.json"; cryptsetup token import --json-file "$K/f3.json" "$THREE"
printf 'pw' | clevis luks bind -y -k - -d "$THREE" tpm2 '{}' >/dev/null 2>&1
t(){ luks_list_tokens "$THREE"; }
run "token list: TPM2, FIDO2 and clevis (nested JSON) tokens" t < /dev/null
check "three tokens" [ "$(wc -l < "$FAKE/out")" -eq 3 ]; check "clevis listed" grep -qx "2 clevis" "$FAKE/out"
cryptsetup token remove --token-id 1 "$THREE"
t(){ luks_list_tokens "$THREE"; }
run "token list: gap in the token ids after removing token 1" t < /dev/null
check "token 0 kept" grep -qx "0 systemd-tpm2" "$FAKE/out"; check "token 2 kept" grep -qx "2 clevis" "$FAKE/out"; check "two tokens" [ "$(wc -l < "$FAKE/out")" -eq 2 ]
sed 's/"keyslots":\["2"\]/"keyslots":["0"]/' "$K/f.json" > "$K/f0.json"; cryptsetup token import --json-file "$K/f0.json" "$K/hdr"
t(){ luks_list_tokens "$K/hdr"; }
run "token list: detached LUKS2 header" t < /dev/null
check "token read from the header file" grep -qx "0 systemd-fido2" "$FAKE/out"

t(){ tab "v $TOK none keyscript=$K/ks"; luks_unlock_all; }
run "key script wins over tokens" t < /dev/null
check "mapped" mapped v; check "no token export" bash -c "! grep -q 'token export' $FAKE/calls"

t(){ tab "v $HDRDATA none header=$K/hdr"; luks_unlock_all; }
run "detached LUKS header" t <<< $'pw'
check "mapped" mapped v; check "header passed to cryptsetup" grep -q -- "--header $K/hdr" "$FAKE/calls"

# clevis as the second token, behind a TPM2 token with a PIN. clevis itself
# finds a slot's token in the luksDump text, which lacks later tokens with
# systemd 261's token plugins (emulated here, in front of the stub).
CLV2=$(mkluks clev2 luks2 L); PASSWORD=pw NEWPIN=1234 systemd-cryptenroll --tpm2-device="$TCTI" --tpm2-pcrs= --tpm2-with-pin=yes "$CLV2" >/dev/null 2>&1
printf 'pw' | clevis luks bind -y -k - -d "$CLV2" tpm2 '{}' >/dev/null 2>&1
mkdir -p "$K/stderrstub"
printf '%s\n' '#!/bin/bash' \
  'if [ "$1" = luksDump ] && [[ " $* " != *" --dump-json-metadata "* ]]; then' \
  '  /usr/sbin/cryptsetup "$@" | awk '"'"'/^  1: /{e=1} {if (e) print > "/dev/stderr"; else print}'"'"'; exit "${PIPESTATUS[0]}"; fi' \
  "exec '$S/stubs/cryptsetup' \"\$@\"" > "$K/stderrstub/cryptsetup"; chmod 755 "$K/stderrstub/cryptsetup"
t(){ PATH="$K/stderrstub:$PATH"; tab "v $CLV2 none"; luks_unlock_all; }
run "clevis as second token, luksDump prints it to stderr (systemd 261 plugins)" t < /dev/null
check "mapped with no prompt" mapped v; check "unlocked via clevis" grep -q "unlocked $CLV2 with clevis" "$FAKE/log"
check "no PIN prompt" [ "$(prompts 'TPM2 PIN')" -eq 0 ]

# clevis token in a detached header: the JWE of CLV2 opens a new key slot
# of the header
CLPASS=$(cryptsetup token export --token-id 1 "$CLV2" | jose fmt -j- -Og jwe -o- | jose jwe fmt -i- -c | clevis decrypt 2>/dev/null)
slots(){ cryptsetup luksDump "$1" 2>/dev/null | sed -n '/^Keyslots:/,/^Tokens:/p' | sed -En 's/^  ([0-9]+): luks2.*/\1/p' | sort; }
before=$(slots "$K/hdr")
printf 'pw' | cryptsetup luksAddKey -q --header "$K/hdr" --pbkdf pbkdf2 --pbkdf-force-iterations 1000 --key-file=- "$HDRDATA" <(printf '%s' "$CLPASS")
NEWSLOT=$(comm -13 <(echo "$before") <(slots "$K/hdr"))
cryptsetup token export --token-id 1 "$CLV2" | sed -E 's/"keyslots": *\[[^]]*\]/"keyslots":["'"$NEWSLOT"'"]/' > "$K/clevhdr.json"
cryptsetup token import --json-file "$K/clevhdr.json" "$K/hdr"
t(){ tab "v $HDRDATA none header=$K/hdr"; luks_unlock_all; }
run "clevis token in a detached LUKS2 header" t < /dev/null
check "clevis slot added (${NEWSLOT:-none})" [ -n "$NEWSLOT" ]
check "mapped with no prompt" mapped v; check "unlocked via clevis" grep -q "unlocked $HDRDATA with clevis" "$FAKE/log"

t(){ tab "v $L1 none"; luks_unlock_all; }
run "LUKS1 volume (no tokens)" t <<< $'pw'
check "mapped" mapped v; check "no errors" bash -c "! grep -q ERROR $FAKE/log"

t(){ tab "v $GK $K/wrong.key"; luks_unlock_all; }
run "wrong key file content -> passphrase fallback" t <<< $'pw'
check "mapped" mapped v; check "warned" grep -q "key file $K/wrong.key failed" "$FAKE/log"

t(){ LUKS_GNUPG_DIR="$K/onlyb"; tab "v $GK $K/good.key.gpg"; luks_unlock_all; }
run "GnuPG file encrypted to two recipients, only second key present" t <<< $'passb'
check "mapped" mapped v; check "secret key prompt" [ "$(prompts 'Secret key passphrase')" -eq 1 ]

t(){ echo "$K/good.key.sym.gpg" > "$BASE/cmdline.d/zbm.luks_key"; tab "v $GK none"; luks_unlock_all; }
run "zbm.luks_key pointing at a GnuPG file" t <<< $'symp'
check "mapped" mapped v

# Gentoo V3 guide: no crypttab at all, only zbm.luks=UUID= and zbm.luks_key=
t(){ rm -f "$ZBM_CRYPTTAB"; echo "UUID=$(cryptsetup luksUUID $GK)" > "$BASE/cmdline.d/zbm.luks"; echo "$K/good.key.sym.gpg" > "$BASE/cmdline.d/zbm.luks_key"; luks_unlock_all; }
run "no crypttab: zbm.luks=UUID= + symmetric GnuPG zbm.luks_key" t <<< $'symp'
check "mapped as luks-<UUID>" mapped "luks-$(cryptsetup luksUUID $GK)"; check "cmdline mode" bash -c "! grep -q 'mode: auto' $FAKE/log"; check "one prompt" [ "$(grep -c timed_prompt $FAKE/prompts 2>/dev/null || echo 0)" -le 1 ]; check "no leftovers" noleft
t(){ rm -f "$ZBM_CRYPTTAB"; LUKS_GNUPG_DIR="$K/onlyb"; echo "UUID=$(cryptsetup luksUUID $GK)" > "$BASE/cmdline.d/zbm.luks"; echo "$K/good.key.gpg" > "$BASE/cmdline.d/zbm.luks_key"; luks_unlock_all; }
run "no crypttab: zbm.luks=UUID= + public-key GnuPG zbm.luks_key" t <<< $'passb'
check "mapped as luks-<UUID>" mapped "luks-$(cryptsetup luksUUID $GK)"; check "unlocked with GnuPG key file" grep -q "with GnuPG key file" "$FAKE/log"; check "no leftovers" noleft
t(){ rm -f "$ZBM_CRYPTTAB"; echo "UUID=$(cryptsetup luksUUID $GK)" > "$BASE/cmdline.d/zbm.luks"; echo "$K/good.key.sym.gpg" > "$BASE/cmdline.d/zbm.luks_key"; luks_unlock_all; }
run "no crypttab: GnuPG passphrase wrong, then empty -> LUKS passphrase fallback" t <<< $'wrong\n\npw'
check "mapped via fallback" mapped "luks-$(cryptsetup luksUUID $GK)"
# Gentoo V4 guide: no crypttab, only zbm.luks=UUID=, passphrase prompt
t(){ rm -f "$ZBM_CRYPTTAB"; echo "UUID=$(cryptsetup luksUUID $GK)" > "$BASE/cmdline.d/zbm.luks"; luks_unlock_all; }
run "no crypttab: zbm.luks=UUID= with passphrase only" t <<< $'pw'
check "mapped as luks-<UUID>" mapped "luks-$(cryptsetup luksUUID $GK)"; check "only the listed volume" [ "$(ls /dev/mapper | grep -c luks-)" -eq 1 ]

t(){ luks_unlock_all; }
run "auto discovery unlocks TPM2 volume silently, closes swap" t < /dev/null
echo swap > "$FAKE/content/${PLAIN##*/}"
check "TOK mapped" bash -c "ls /dev/mapper | grep -q luks-$(cryptsetup luksUUID $TOK)"
check "no prompt for TOK" bash -c "! grep -q 'Unlock LUKS volume $TOK' $FAKE/out"

t(){ : > "$ZBM_CRYPTTAB"; luks_unlock_all; }
run "empty crypttab -> auto mode" t < /dev/null
check "auto mode" grep -q "mode: auto" "$FAKE/log"

echo "### races"
t(){ tab "v LABEL=zfsLATE none"; echo 0 > "$ANSWERS"; echo 0 >> "$ANSWERS"; echo 0 >> "$ANSWERS"; echo 0 >> "$ANSWERS"
     ( sleep 2; truncate -s 32M "$K/late.img"; printf 'pw' | cryptsetup luksFormat -q --type luks2 --pbkdf pbkdf2 --pbkdf-force-iterations 1000 --label zfsLATE --key-file=- "$K/late.img"; losetup -f "$K/late.img" ) & luks_unlock_all; wait; }
run "device appears 2 s after unlock starts (crypttab)" t <<< $'pw'
check "mapped" mapped v; check "missing-device prompt shown first" grep -q "zfsLATE is missing" "$FAKE/prompts"

t(){ LUKS_NO_TOKEN_PLUGINS=1; echo 5 > "$BASE/cmdline.d/zbm.luks_token_wait"; mv /dev/hidraw99 /dev/xhid; ( sleep 2; mv /dev/xhid /dev/hidraw99 ) & tab "v $FIDOONLY none"; luks_unlock_all; wait; }
run "FIDO2 key plugged in 2 s into the wait" t < /dev/null
check "mapped" mapped v; check "waited" grep -q "Insert the FIDO2 security key" "$FAKE/out"

t(){ tab "v $PLAIN none"; ( sleep 3; printf 'pw' | cryptsetup open --key-file=- "$PLAIN" ext ) & luks_unlock_all; wait; }
run "volume unlocked from another session while the prompt waits" t < <(sleep 15)
check "rc 0" [ "$RC" -eq 0 ]; check "noticed" grep -q "unlocked elsewhere" "$FAKE/log"; check "not recorded as ours" bash -c "! grep -q . $FAKE/base/luks-mappings 2>/dev/null"

t(){ tab "v $PLAIN none"; luks_unlock_all <<< $'pw' & luks_unlock_all <<< $'pw'; wait; }
run "two sessions unlock the same volume at once" t
check "exactly one mapping" [ "$(ls /dev/mapper | grep -vcx control)" -eq 1 ]
check "recorded once" [ "$(grep -c '^v' $FAKE/base/luks-mappings)" -eq 1 ]; check "no ERROR crash" bash -c "! grep -qiE 'syntax|command not found' $FAKE/out"
t(){ local r n; for r in $(seq 1 50); do rm -f "$LUKS_STATE"; for n in 1 2 3 4; do luks_record_mapping v /dev/loop99 0 & done; wait
     [ "$(grep -c '^v' "$LUKS_STATE")" -eq 1 ] || echo DUPLICATE; done; }
run "four sessions record the same mapping at once, 50 rounds" t
check "never recorded twice" bash -c "! grep -q DUPLICATE '$FAKE/out'"; check "no lock left" bash -c "[ ! -e '$FAKE/base/luks-mappings.lock' ]"

echo "### import-loop retries (late disks)"
t(){ tab "v $PLAIN none"; luks_unlock_all <<< $'\n'; LUKS_RETRY=1 luks_unlock_all <<< $'pw'; }
run "volume skipped by the user is not prompted again on retry" t
check "retry rc 1 (nothing new)" [ "$RC" -eq 1 ]; check "only one prompt" [ "$(prompts 'Passphrase (leave')" -eq 1 ]; check "not mapped" bash -c "! [ -b /dev/mapper/v ]"

t(){ tab "v LABEL=zfsLATE2 none"; LUKS_RETRY=1 luks_unlock_all < /dev/null; r1=$?
     truncate -s 32M "$K/late2.img"; printf 'pw' | cryptsetup luksFormat -q --type luks2 --pbkdf pbkdf2 --pbkdf-force-iterations 1000 --label zfsLATE2 --key-file=- "$K/late2.img"; losetup -f "$K/late2.img"
     echo zfs_member > "$FAKE/content/$(basename "$(losetup -j "$K/late2.img" | cut -d: -f1)")"
     LUKS_RETRY=1 luks_unlock_all <<< $'pw'; r2=$?; echo "r1=$r1 r2=$r2"; [ $r1 -eq 1 ] && [ $r2 -eq 0 ]; }
run "retry: device missing -> no wait; device appears -> unlocked" t
check "missing device: no wait prompt" bash -c "! [ -s $FAKE/prompts ]"; check "first retry rc 1, second rc 0" [ "$RC" -eq 0 ]; check "mapped" mapped v

t(){ tab "a $PLAIN none" "b $L1 none"; luks_unlock_all <<< $'pw'; LUKS_RETRY=1 luks_unlock_all < /dev/null; }
run "retry with everything already open does nothing" t
check "rc 1" [ "$RC" -eq 1 ]; check "one prompt only" [ "$(prompts 'Passphrase (leave')" -eq 1 ]

t(){ echo off > "$BASE/cmdline.d/zbm.luks"; LUKS_RETRY=1 luks_unlock_all < /dev/null; }
run "retry with zbm.luks=off reports nothing new (no endless import loop)" t
check "retry rc 1" [ "$RC" -eq 1 ]

t(){ luks_available(){ return 1; }; LUKS_RETRY=1 luks_unlock_all < /dev/null; }
run "retry without cryptsetup reports nothing new" t
check "retry rc 1" [ "$RC" -eq 1 ]

t(){ echo off > "$BASE/cmdline.d/zbm.luks"; luks_unlock_all < /dev/null; }
run "initial run with zbm.luks=off still succeeds" t
check "rc 0" [ "$RC" -eq 0 ]

# The import loop itself (init.d/50-import-pools), with stand-ins for the
# import and the LUKS library: during the retry delay a late LUKS volume and an
# unencrypted device of another pool appear together. The LUKS volume must be
# unlocked before the import that then succeeds, or the boot pool on it is
# never imported.
hookloop(){
  local d="$1"; mkdir -p "$d"; : > "$d/log"
  printf '%s\n' 'luks_unlock_all(){ if [ -e "$D/late" ] && [ ! -e "$D/opened" ]; then touch "$D/opened"; echo unlock-opened >> "$D/log"; return 0; fi; echo unlock-none >> "$D/log"; return 1; }' > "$d/lib.sh"
  sed "s|/lib/zfsbootmenu-luks.sh|$d/lib.sh|g" "$REPO/legacy/zfsbootmenu/init.d/50-import-pools" > "$d/hook"
  ( D="$d"; export D; BASE="$d/base"; mkdir -p "$BASE"
    zinfo(){ :; }; zdebug(){ return 1; }; zerror(){ :; }; colorize(){ shift; echo -n "$@"; }
    cryptsetup(){ :; }
    import_pool(){ if [ -e "$D/opened" ]; then echo import-all >> "$D/log"; touch "$D/imported-boot"; touch "$D/imported-any"
                   elif [ -e "$D/late" ]; then echo import-plain-only >> "$D/log"; touch "$D/imported-any"
                   else echo import-none >> "$D/log"; fi; }
    check_for_pools(){ [ -e "$D/imported-any" ]; }
    match_hostid(){ return 1; }
    timed_prompt(){ echo delay >> "$D/log"; touch "$D/late"; return 0; }
    log_unimportable(){ :; }; emergency_shell(){ echo EMERGENCY >> "$D/log"; exit 1; }
    zpool(){ :; }; is_zfs_filesystem(){ return 1; }
    import_policy=hostid ZFSBOOTMENU_INITIALIZATION=yes source "$d/hook" )
  tr '\n' ' ' < "$d/log"; echo
}
echo "== import loop: late LUKS volume and unencrypted device appear in the same retry delay"
seq_=$(hookloop "$K/hookloop")
echo "  sequence: $seq_"
check "late volume unlocked before the import that succeeds" grep -q "delay unlock-opened import-all" <<< "$seq_"
check "boot pool imported" [ -e "$K/hookloop/imported-boot" ]

echo "### failure paths"
t(){ LUKS_NO_TOKEN_PLUGINS=1; LUKS_TPM2_TCTI="device:/dev/no-such-tpm"; mv /dev/hidraw99 /dev/xhid; LUKS_GNUPG_DIR="$K/onlyb"; tab "a $MULTI $K/good.key.gpg" "b $STALE none"; luks_unlock_all; local r=$?; mv /dev/xhid /dev/hidraw99; return $r; }
run "everything fails on EOF: nothing mapped, nothing left behind" t < /dev/null
check "rc nonzero" [ "$RC" -ne 0 ]; check "nothing mapped" [ -z "$(ls /dev/mapper | grep -vx control)" ]; check "no leftovers" noleft; check "no gpg agents left running" bash -c "! pgrep -f '[g]pg-agent.*zbm-gpg' >/dev/null"

echo "### teardown matching"
t(){ tab "a1 $PLAIN none" "b1 $L1 none"; luks_unlock_all <<< $'pw'
     echo /dev/mapper/a1p1 > "$FAKE/pools/onpart"; printf '/dev/mapper/b1\n/dev/vda2\n' > "$FAKE/pools/mixed"; echo /dev/vdb > "$FAKE/pools/plainpool"; luks_close_all; }
run "pool on a partition of a mapping and a mixed LUKS/plain pool" t
check "onpart exported" grep -qx onpart "$FAKE/exported"; check "mixed exported" grep -qx mixed "$FAKE/exported"; check "plain pool untouched" [ -e "$FAKE/pools/plainpool" ]
check "a1 closed" bash -c "! [ -b /dev/mapper/a1 ]"; check "b1 closed" bash -c "! [ -b /dev/mapper/b1 ]"

echo; echo "passed: $PASS failed: $FAIL"; [ "$FAIL" -eq 0 ]
