#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab

## REMOTE SSH ACCESS

# This script builds a zbm with remote access support via ssh. The image and
# efi executable will contain private host keys and public keys used for
# mutual authentication. It is based on
# https://github.com/zbm-dev/zfsbootmenu/wiki/Remote-Access-to-ZBM

# All keys / settings are placed into ${BUILD_DIR} (the current working
# directory by default). Its contents can be customized as needed.
# They are regenerated if missing.

# Server private keys are generated automatically as needed. It is
# recommended to not reuse existing private keys.
# User authentication for the root account is done via 'authorized_keys'.
# The current users' authorized keys are copied if it is not present.

## ROOT FILESYSTEM

# The root filesystem needs to be unlockable by the primary initrd (the one that
# is started by ZBM), otherwise, the operating system itself will block booting
# and ask for the key again.
# This can be done by using a key file stored in that initrd.
# (Instructions below for ubuntu, but can be adapted to various platforms.)

# This key file will be stored unencrypted in the initramfs, so make sure access
# to it is protected.

# ```
# mkdir -p -m 700 /etc/zfs/keys
# cp passwordfile /etc/zfs/keys/rpool.key
# zfs change-key \
#     -o keylocation=file:///etc/zfs/keys/rpool.key \
#     -o keyformat=passphrase rpool
# sed -i '/FILES=/a FILES+=(/etc/zfs/keys/rpool.key) /etc/mkinitcpio.conf
# update-initramfs -u
# chmod go= /boot
# ```

## USAGE

# To unlock the pool remotely, log in via ssh, then start zfsbootmenu, enter the
# key and continue as usual.

# ```
# ssh root@host -p 222
# zfsbootmenu
# ```

## SCRIPT ARGUMENTS

# This script forwards arguments to the zbm-builder.sh helper script, but
# overrides any build-directory specification. In addition, the script adds
# arguments to bind-mount \${BUILD_DIR}/cmdline.d and install the Ubuntu
# packages `dropbear-bin` and `network-manager` inside the container. Ubuntu
# does not package the `dracut-crypt-ssh` module, so the script fetches it into
# \${BUILD_DIR}/dracut-crypt-ssh with git (unless it is already there) and
# installs it in the container with an rc.d hook. Dracut configures the network
# in the image with NetworkManager, the only network back end in Ubuntu's
# Dracut that works without systemd.


BUILD_DIR=$(realpath "${BUILD_DIR:-${PWD}}")
ZBM_BUILDER=$(realpath "${ZBM_BUILDER:-zbm-builder.sh}")

if [ ! -x "${ZBM_BUILDER}" ]; then
  echo "ERROR: Cannot find build script ${ZBM_BUILDER}, please set \$ZBM_BUILDER"
  exit 1
fi

mkdir -p "${BUILD_DIR}"


## PREPARE SERVER PRIVATE KEYS

# If no local ssh host keys are available, create them
# WARNING: These private keys will be part of the final image!
RS_SSH_KEYTYPES=(rsa ecdsa ed25519)
RS_KEYDIR="${BUILD_DIR}/dropbear"
mkdir -p "${RS_KEYDIR}"

RS_KEYGEN_OPT=(-m PEM -N '' -C "zbm-server-key")
for keytype in "${RS_SSH_KEYTYPES[@]}"; do
  RS_PK_FILE="${RS_KEYDIR}/ssh_host_${keytype}_key"
  [ -f "${RS_PK_FILE}" ] && continue
  ssh-keygen -t "${keytype}" "${RS_KEYGEN_OPT[@]}" -f "${RS_PK_FILE}"
done


## PREPARE AUTHORIZED KEYS

# Attempt to find the authorized_keys file of the original user, instead of
# root, since this script will likely be invoked via sudo. Otherwise, fall
# back to the current user.
if [ -n "${SUDO_USER}" ]; then
  SUDO_HOME=$(eval echo ~"${SUDO_USER}")
fi
if [ -r "${SUDO_HOME}/.ssh/authorized_keys"  ]; then
  RS_AUTH_SRC="${SUDO_HOME}/.ssh/authorized_keys"
else
  RS_AUTH_SRC="${HOME}/.ssh/authorized_keys"
fi

RS_AUTH="${RS_KEYDIR}/authorized_keys"
if [ ! -f "${RS_AUTH}" ]; then
  if [ -r "${RS_AUTH_SRC}" ]; then
    echo "Cannot find ${RS_AUTH}, copying from ${RS_AUTH_SRC}"
    cp -v "${RS_AUTH_SRC}" "${RS_AUTH}"
  else
    echo "ERROR: Cannot find ${RS_AUTH}, and ${RS_AUTH_SRC} is not available, please provide it manually"
    exit 1
  fi
fi


## PREPARE THE DRACUT MODULE

RS_MODSRC="${BUILD_DIR}/dracut-crypt-ssh"
if [ ! -d "${RS_MODSRC}/modules/60crypt-ssh" ]; then
  git clone --depth 1 https://github.com/dracut-crypt-ssh/dracut-crypt-ssh "${RS_MODSRC}" || {
    echo "ERROR: unable to fetch dracut-crypt-ssh into ${RS_MODSRC}"
    exit 1
  }
fi

# Generated hook, not user customizable: install the module without its
# optional helpers, which would need to be compiled
mkdir -p "${BUILD_DIR}/rc.d"
cat > "${BUILD_DIR}/rc.d/crypt-ssh" <<'EOF'
#!/bin/sh
set -e
mod=/usr/lib/dracut/modules.d/60crypt-ssh
mkdir -p "${mod}"
cp /build/dracut-crypt-ssh/modules/60crypt-ssh/*.sh \
  /build/dracut-crypt-ssh/modules/60crypt-ssh/*.rules "${mod}/"
sed -i '/helper\//d' "${mod}/module-setup.sh"
EOF
chmod 755 "${BUILD_DIR}/rc.d/crypt-ssh"


## PREPARE SETTINGS

mkdir -p "${BUILD_DIR}/cmdline.d"
RS_DNC="${BUILD_DIR}/cmdline.d/dracut-network.conf"
if [ ! -f "${RS_DNC}" ]; then
  # ip=dhcp tries to bring up all interfaces
  # ip=single-dhcp stops after bringing up the first
  # See https://www.man7.org/linux/man-pages/man7/dracut.cmdline.7.html
  echo "ip=single-dhcp rd.neednet=1" > "${RS_DNC}"
fi

# Generated config file, not user customizable
mkdir -p "${BUILD_DIR}/dracut.conf.d"
RS_DDC="${BUILD_DIR}/dracut.conf.d/dracut-dropbear.conf"
cat > "${RS_DDC}" <<-EOF
	# Enable dropbear ssh server and pull in network configuration args
	add_dracutmodules+=" crypt-ssh "
	install_optional_items+=" /etc/cmdline.d/dracut-network.conf "
	dropbear_acl=/build/dropbear/authorized_keys
EOF
for keytype in "${RS_SSH_KEYTYPES[@]}"; do
  echo "dropbear_${keytype}_key=/build/dropbear/ssh_host_${keytype}_key" >> "${RS_DDC}"
done

## ZBM BUILDING

# Separate arguemnts into those for the helper and those for the container
HELPER_ARGS=( )
BUILDER_ARGS=( )

_builder=""
for _arg in "$@"; do
  # Pulling helper arguments first
  if [ -z "${_builder}" ]; then
    # If the argument is "--", drop it and switch to container args
    if [ "${_arg}" = "--" ]; then
      _builder="yes"
      continue
    fi

    HELPER_ARGS+=( "${_arg}" )
  else
    BUILDER_ARGS+=( "${_arg}" )
  fi
done

"${ZBM_BUILDER}" "${HELPER_ARGS[@]}" -b "${BUILD_DIR}" \
  -O -v -O "${BUILD_DIR}/cmdline.d:/etc/cmdline.d:ro" \
  -- "${BUILDER_ARGS[@]}" -p dropbear-bin -p network-manager
