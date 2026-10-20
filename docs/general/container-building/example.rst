A Simple Host-Specific Container Build
======================================

``zbm-builder.sh`` mounts a build directory (by default, the current working directory) into the container to provide a
path to inject custom configuration into the container. If the system will manage ZFSBootMenu images exclusively via a
build container, an obvious location for the build directory is ``/etc/zfsbootmenu``. Start by creating this directory
and populating a simple ``config.yaml`` for container builds::

  mkdir -p /etc/zfsbootmenu

  cat > /etc/zfsbootmenu/config.yaml <<EOF
  Components:
    Enabled: false
  EFI:
    Enabled: true
    Versions: false
  Kernel:
    Prefix: zfsbootmenu
    CommandLine: zfsbootmenu ro quiet loglevel=0 nomodeset
  EOF

  curl -L -o /etc/zfsbootmenu/zbm-builder.sh https://raw.githubusercontent.com/zbm-dev/zfsbootmenu/master/zbm-builder.sh
  chmod 755 /etc/zfsbootmenu/zbm-builder.sh

The build container creates images with ``dracut``; it has no ``mkinitcpio``. Component generation is disabled, so
``generate-zbm`` will produce only a UEFI bundle. That bundle has numeric versioning disabled, so ``generate-zbm`` will
produce an unversioned ``zfsbootmenu.EFI``; if the generator detects an existing ``zfsbootmenu.EFI`` in the output
directory, it will make a single backup of that file as ``zfsbootmenu-backup.EFI`` before overwriting it. A simple
kernel command-line is specified and may be overridden as necessary.

Additional ``dracut`` settings go in files in ``/etc/zfsbootmenu/dracut.conf.d``, which the container adds to its
default ZFSBootMenu configuration. Each file name must end in ``.conf``.

Custom Font
-----------

On high-resolution screens, the Linux kernel does not always do a good job choosing a console font. ZFSBootMenu images
include Terminus fonts in several sizes and, by default, pick the largest one that still leaves at least 110 columns on
the screen. To always use one size instead, add an early-setup hook that sets it after the automatic choice::

  mkdir -p /etc/zfsbootmenu/hooks/early-setup.d

  cat > /etc/zfsbootmenu/hooks/early-setup.d/40-console-font.sh <<EOF
  #!/bin/sh
  setfont /usr/share/zfsbootmenu/fonts/ter-v28b.psf 2>/dev/null || true
  EOF

  chmod 755 /etc/zfsbootmenu/hooks/early-setup.d/40-console-font.sh

The bundled sizes are ``ter-v12n``, ``ter-v14b``, ``ter-v20b``, ``ter-v24b``, ``ter-v28b`` and ``ter-v32b``. When
``zbm-builder.sh`` finds a ``hooks`` subdirectory in the build directory, it includes the hooks found there in the
image; see :ref:`zbm-dracut-options`.

The container entrypoint can also "terraform" the container before it builds an image: when it finds an ``rc.d``
subdirectory in the build root, it runs each executable file therein before generating a ZFSBootMenu image. If any of
these executables should fail, image generation is aborted. The remote-access setup below uses this.

Host-Specific Files
-------------------

By default, ``zbm-builder.sh`` will copy the file ``/etc/hostid`` from the host to the build directory so that the
hostid of the generated ZFSBootMenu image will match that of your host. This is often desirable for customized builds,
but it would be undesirable for copies of these files in ``/etc/zfsbootmenu`` to fall out of synchronization with the
host versions. To avoid this issue, tell ``zbm-builder.sh`` to remove any copies in ``/etc/zfsbootmenu`` before
determining whether the host versions should be copied in for image creation::

  echo "REMOVE_HOST_FILES=yes" >> /etc/zfsbootmenu/zbm-builder.conf

If you would rather not see those files at all, it is possible to instruct ``generate-zbm`` to remove them after they
are used. Edit the configuration at ``/etc/zfsbootmenu/config.yaml`` and add the following key:

.. code-block:: yaml

  Global:
    PostHooksDir: /build/cleanup.d

Alternatively, tell the build container to add this option dynamically::

  echo "BUILD_ARGS+=( -e '.Global.PostHooksDir=\"/build/cleanup.d\"' )" \
      >> /etc/zfsbootmenu/zbm-builder.conf

Next, create a post-generation hook to remove the files::

  mkdir -p /etc/zfsbootmenu/cleanup.d

  cat > /etc/zfsbootmenu/cleanup.d/hostfiles <<EOF
  #!/bin/sh
  rm -f /build/zpool.cache /build/hostid
  EOF

  chmod 755 /etc/zfsbootmenu/cleanup.d/hostfiles

The Output Directory
--------------------

At this point, it should be possible to generate images by running

.. code-block::

  cd /etc/zfsbootmenu && ./zbm-builder.sh

However, these images will reside in ``/etc/zfsbootmenu/build`` and will require manual management. A better alternative
is to let ``generate-zbm`` manage the ZFSBootMenu output directory directly. Assuming that ZFSBootMenu images should be
installed in ``/boot/efi/EFI/zfsbootmenu``, tell ``zbm-builder.sh`` to mount the directory inside the container, and
tell the container that it should write its images to the mounted directory::

  cat >> /etc/zfsbootmenu/zbm-builder.conf <<EOF
  RUNTIME_ARGS+=( -v /boot/efi/EFI/zfsbootmenu:/output )
  BUILD_ARGS+=( -o /output )
  EOF

Now, running

.. code-block::

  cd /etc/zfsbootmenu && ./zbm-builder.sh

should create images directly in ``/boot/efi/EFI/zfsbootmenu`` and create a backup of any existing ``zfsbootmenu.EFI``.

Networking in Rootfull Containers
---------------------------------

Manipulating files in ``/etc/zfsbootmenu`` and ``/boot/efi/EFI/zfsbootmenu`` may require root privileges, which means
that ``zbm-builder.sh`` and the build container will need to run as root. In some configurations, ``podman`` may not
provide working networking for rootfull containers by default. A simple fix is to allow the containers to use the host
network stack, which can be accomplished by running

.. code-block::

  echo "RUNTIME_ARGS+=( --net=host )" >> /etc/zfsbootmenu/zbm-builder.conf

Adding Remote Access Capabilities
---------------------------------

ZFSBootMenu images can run an SSH server for remote access, as described in :doc:`/general/remote-access`. Container
builds use the Dracut instructions there, with the changes below. All files must be in the build directory, because
the container cannot see the rest of the host.

The build container needs two more Ubuntu packages: ``dropbear-bin`` for the SSH server and ``network-manager``, which
Dracut uses to configure the network in the image (Ubuntu's Dracut has no other network back end that works without
systemd)::

  echo "BUILD_ARGS+=( -p dropbear-bin -p network-manager )" >> /etc/zfsbootmenu/zbm-builder.conf

Fetch the ``dracut-crypt-ssh`` module into the build directory::

  git clone https://github.com/dracut-crypt-ssh/dracut-crypt-ssh /etc/zfsbootmenu/dracut-crypt-ssh

Create SSH host keys in the PEM format that ``dracut-crypt-ssh`` converts for ``dropbear``, and the list of keys that
may log in::

  mkdir -p /etc/zfsbootmenu/dropbear
  for keytype in rsa ecdsa ed25519; do
    ssh-keygen -q -N "" -m PEM -t "${keytype}" -f "/etc/zfsbootmenu/dropbear/ssh_host_${keytype}_key"
  done

  cp "/home/${dropbear_user}/.ssh/authorized_keys" /etc/zfsbootmenu/dropbear/root_key

Replace ``${dropbear_user}`` with the user whose ``authorized_keys`` file should govern access to ZFSBootMenu. The file
must be a copy, not a symbolic link: the link target would not exist in the container.

Add a "terraform" script that installs the module in the container without its optional helpers, which would need to
be compiled, and that writes the network parameters for the image::

  mkdir -p /etc/zfsbootmenu/rc.d

  cat > /etc/zfsbootmenu/rc.d/crypt-ssh <<'EOF'
  #!/bin/sh
  set -e
  mod=/usr/lib/dracut/modules.d/60crypt-ssh
  mkdir -p "${mod}"
  cp /build/dracut-crypt-ssh/modules/60crypt-ssh/*.sh \
    /build/dracut-crypt-ssh/modules/60crypt-ssh/*.rules "${mod}/"
  sed -i '/helper\//d' "${mod}/module-setup.sh"
  mkdir -p /etc/cmdline.d
  echo "ip=dhcp rd.neednet=1" > /etc/cmdline.d/dracut-network.conf
  EOF

  chmod 755 /etc/zfsbootmenu/rc.d/crypt-ssh

Replace ``ip=dhcp`` with a static configuration if desired; see :manpage:`dracut.cmdline(7)`. Finally, tell Dracut to
include the module, the keys and the network parameters::

  cat > /etc/zfsbootmenu/dracut.conf.d/dropbear.conf <<EOF
  add_dracutmodules+=" crypt-ssh "
  dropbear_rsa_key=/build/dropbear/ssh_host_rsa_key
  dropbear_ecdsa_key=/build/dropbear/ssh_host_ecdsa_key
  dropbear_ed25519_key=/build/dropbear/ssh_host_ed25519_key
  dropbear_acl=/build/dropbear/root_key
  install_items+=" /etc/cmdline.d/dracut-network.conf "
  EOF

The next build includes an SSH server listening on port 222. Connect with ``ssh -p 222 root@<address>``.
