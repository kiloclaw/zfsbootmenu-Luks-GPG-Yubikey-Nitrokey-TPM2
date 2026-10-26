# Containerized Image Building

Files in this directory provide a means for creating a container that is
capable of building ZFSBootMenu kernel and initramfs components as well as a
standalone UEFI bundle. The container image is built atop Ubuntu and provides
the basic components necessary to build an image from an arbitrary
ZFSBootMenu repository, including a build script that runs by default when the
container is started.

These images build and run with both Docker and Podman. Podman is recommended
as it provides daemon-free container management and allows containers to run as
unprivileged users. Sample commands below refer to the `podman` command.
However, because `podman` and `docker` have compatible command-line interfaces,
the commands should work just as well by substituting `docker` for `podman`.

# Creating a ZFSBootMenu Builder Image

The image is not published in a registry; build it from this repository. From
the root of the ZFSBootMenu tree, run

    podman build -t localhost/zbm-builder packaging/container

to create the image `localhost/zbm-builder:latest`, which is the default image
of `zbm-builder.sh`. Any other tag may be used and given to `zbm-builder.sh`
with `-i`.

Alternatively, the script `image-build.sh` uses `buildah` to build the same
`Dockerfile`. It requires a single argument, the tag to use when naming the
image, and accepts these options:

- `-u <release>` builds on another Ubuntu release (default `26.10`);
- `-f <flavour>` installs another Ubuntu kernel flavour (default `generic`);
- `-p <package>` adds an Ubuntu package to the image (may be repeated).

An optional second argument is a Git commit-like reference (a hash or tag) that
will be recorded as `/etc/zbm-commit-hash` in the image. The contents of this
file are used to checkout a specific state of the ZFSBootMenu repository. If
the tag is unspecified on the command line, the build script will attempt to
capture a reference to the current HEAD commit if the image is built in an
active git repository. If a commit-like name is not provided and cannot be
discovered, no default will be recorded and containers will attempt to build
from the current `master`.

By default, the `image-build.sh` script uses the `build-init.sh` next to it
as the entrypoint for build containers. To use another entrypoint, set the
`ZBM_BUILDER` environment variable to the location of the `build-init.sh`
script to use.

The same settings are available as build arguments when building with
`podman build` or `docker build`: `UBUNTU_RELEASE`, `KERNEL_FLAVOUR`,
`ZBM_COMMIT_HASH` and `PACKAGES` (a space-separated list).

## Builder Image Contents

`setup-image.sh` installs the image contents while the image is built:

- The kernel and ZFS come from Ubuntu. Ubuntu builds the ZFS module for each
  of its kernels and ships it in a separate package
  (`linux-main-modules-zfs-<version>-<flavour>`), so the image needs no DKMS
  build and no kernel headers. Firmware and microcode packages are left out.

- The Ubuntu release selects the kernel series and the OpenZFS version. The
  default release, 26.10, provides Linux 7.3 and OpenZFS 2.4.4. A later
  release can be chosen as described above; earlier ones, including 26.04 LTS,
  do not package `yq-go`, which the entrypoint needs. The `realtime` kernel
  flavour has no ZFS module.

- The image records its kernel series (for example `7.3.*`) in
  `/etc/zbm-kernel-version`. `config/zbm-builder/config.yaml` selects the same
  series with `Kernel.Version`. When the configured version matches no
  installed kernel, for example after building the image from another Ubuntu
  release, the entrypoint uses the recorded series instead and says so. A
  version that matches an installed kernel is left alone.

- Kernel and ZFS packages are held with `apt-mark`, so the entrypoint's `-u`
  option updates everything else.

- Images are built with dracut. Ubuntu does not package `mkinitcpio`, so
  configurations that set `Global.InitCPIO` are refused.

- GnuPG with `scdaemon`, libfido2, clevis, tpm2-tools, ykpers and openssl are
  installed for LUKS unlocking, together with systemd's cryptsetup token
  plugins. Images therefore unlock TPM2, FIDO2 and PKCS#11 LUKS2 tokens
  through systemd's own code, including TPM2 tokens with signed PCR policies
  or `--tpm2-pcrlock`, which ZFSBootMenu's built-in TPM2 code does not
  support.

The builder image does **not** contain a ZFSBootMenu installation or a copy of
the upstream git repository. Instead, the entrypoint `/build-init.sh` will
fetch a ZFSBootMenu archive when the container is instantiated (or allow a
local copy to be bind-mounted) and, as noted above, attempt to check out a
specific commit based on the contents of `/etc/zbm-commit-hash`.

# Running a ZFSBootMenu Builder Container

When running a container from the ZFSBootMenu builder image, it is generally
expected that some compatible volume (generally, a local directory) will be
bind-mounted as a volume at the path `/build` inside the container. The volume
may either be empty or contain a custom ZFSBootMenu configuration and will
contain build products (a UEFI bundle, separate kernel and initramfs
components, or both) in a `build` subdirectory.

The container entrypoint expects to run `generate-zbm` directly from a
ZFSBootMenu source repository that is available at `/zbm` within the container.
If the entrypoint finds `/zbm` nonexistent or devoid of files, it will fetch a
copy of the upstream source repository and unpack it where expected. (The tag,
commit or branch to fetch can be specified at runtime or a default will be
chosen as encoded in the container image.) Any pre-existing and non-empty
`/zbm` within the container must contain a copy of the source repository. This
is useful, *e.g.*, to bind-mount a local clone of the repository into the
container.

## Command-Line Arguments and Environment Variables

The build script accepts several command-line arguments or environment
variables that override its default behavior. Run the container with the `-h`
command-line argument to see a summary of build options and their default
options. The options are:

- `$BUILDROOT` specifies a default root for image builds. The build root is
  expected to hold configuration files and, optionally, an output directory,
  hostid and pool cache files. The value of `$BUILDROOT` is `/build` by
  default.

  The environment variable or default can be overridden with the `-b` option.

- `$ZBMOUTPUT` specifies an alternative output directory for ZFSBootMenu build
  products. The container *always* overrides configurations to store build
  products (UEFI bundles and kernel components, as configured) in a temporary
  directory; these products will be copied to `$ZBMOUTPUT` after successful
  image creation. The value of `$ZBMOUTPUT` is `${BUILDROOT}/build` by default.

  The environment variable or default can be overridden with the `-o` option.

- `$ZBMTAG` specifies any "commit-ish" label recognized by `git` as a pointer
  to a specific git commit. This can be a branch name (to grab the head of that
  branch), tag or commit hash. If `/zbm` in the container is not pre-populated,
  the container will fetch and unpack the named tag. By default, the value of
  `$ZBMTAG` will be taken from the contents of `/etc/zbm-commit-hash` if the
  container was built with the `ZBM_COMMIT_HASH` build argument; otherwise, the
  default is `master`. The tag is ignored if `/zbm` in the container is not
  empty.

  The environment variable or default can be overridden with the `-t` option.

- `$SOURCE_DATE_EPOCH`, when set, makes the images reproducible: two builds of
  the same tree in the same container image give the same bytes, wherever the
  tree and the build root are. Set it to the time of the tree's last commit,
  `SOURCE_DATE_EPOCH="$(git log -1 --format=%ct)"`. The Rust `generate-zbm`
  then passes `--reproducible` to dracut (mkinitcpio and `objcopy` read the
  variable themselves), and the image's `BUILD_ID` is that day.
  `zbm-builder.sh` passes the variable on when it is set.

A couple of additional arguments may only be set from the command line:

- `-e <statement>` provides a statement that will be evaluated via `yq-go eval`
  to modify the `generate-zbm` configuration file immediately before an image
  is built. This option may be specified more than once.

  > Do not use this unless you review the build script and understand, without
  > documentation, what will happen!

- `-p <package>` specifies an Ubuntu package to install in the container
  before images are generated. This option may be specified more than once.

- `-u` updates the Ubuntu packages in the container, except the held kernel
  and ZFS packages.

- `-k <series>` installs the newest Ubuntu kernel of a series (`X.Y`) for
  which Ubuntu publishes a ZFS module and builds with it. The kernel flavour
  comes from `$ZBM_KERNEL_FLAVOUR` (default `generic`). With `-z`, the ZFS
  userspace packages are updated too.

- `-g <generator>` selects the `generate-zbm` that builds the images: `rust`
  (the default for trees that contain the Rust crates) or `perl`, the legacy
  program, which older trees always use. Both produce the same images. A build
  container without `cargo`, such as one built from an older `Dockerfile`, uses
  `perl` and leaves `zbm` out of the images, with a warning; the legacy runtime,
  the only one so far, does not need it. `-g rust` fails there instead.

## ZFSBootMenu Configuration and Execution

After the ZFSBootMenu container entrypoint fetches (or identifies) a copy of
the ZFSBootMenu source repository, it "installs" the copy into the container by
symlinking key components of the source repository into the container
filesystem:

- If the source repository is sufficiently new, a symbolic link

      /usr/share/zfsbootmenu -> /zbm/zfsbootmenu

  will point to the core ZFSBootMenu library.

- For newer versions of ZFSBootMenu, the symbolic link

      /usr/lib/dracut/modules.d/90zfsbootmenu -> /zbm/dracut

  will point to the dracut module; for older versions, the link

      /usr/lib/dracut/modules.d/90zfsbootmenu -> /zbm/90zfsbootmenu

  will serve the same purpose.

Configuration files are handled in a multi-pass approach that synthesizes a
composite configuration from increasingly specific sources. In the first pass,
generic upstream configurations are linked *if the source exists*:

    /etc/zfsbootmenu/config.yaml -> /zbm/etc/zfsbootmenu/config.yaml
    /etc/zfsbootmenu/dracut.conf.d/* -> /zbm/etc/zfsbootmenu/dracut.conf.d/*

Next, container-specific defaults are linked *if the source exists*:

    /etc/zfsbootmenu/config.yaml -> /zbm/etc/zbm-builder/config.yaml
    /etc/zfsbootmenu/dracut.conf.d/* -> /zbm/etc/zbm-builder/dracut.conf.d/*

Finally, build-specific configurations are linked *if the source exists*:

    /etc/zfsbootmenu/config.yaml -> ${BUILDROOT}/config.yaml
    /etc/zfsbootmenu/dracut.conf.d/* -> ${BUILDROOT}/dracut.conf.d/*

Conflicting links will *replace* any links made by earlier passes. This allows
each level of configurations to mask or augment earlier defaults.

For trees that contain the Rust crates, the entrypoint then builds `zbm` and
the Rust `generate-zbm` with the container's `cargo`, offline from the
dependencies vendored in the tree, in a temporary directory (set
`CARGO_TARGET_DIR` to keep the build between runs). It installs `zbm` as
`/usr/libexec/zfsbootmenu/zbm`, where the dracut module finds it, and
`brush`, the shell of the Rust runtime, as `/usr/libexec/zfsbootmenu/brush`;
images include `zbm`, but the legacy runtime they boot does not use it.

Before these passes, the entrypoint writes

    /etc/zfsbootmenu/dracut.conf.d/00-zbm-builder-generic.conf

containing `hostonly="no"`. Images made in a container must work on any
machine, and Ubuntu's dracut is "host-only" by default, which would include
only the kernel modules loaded where the container runs and leave out ZFS and
dm-crypt. Configuration files from the
passes above may still override the setting.

In addition, the hostid file is linked if it exists:

    /etc/hostid -> ${BUILDROOT}/hostid

## Container Customization

When launched, the container entrypoint will run any executable "hook" files it
finds in either of the directories `${BUILDROOT}/rc.pre.d` or
`${BUILDROOT}/rc.d`. These hooks provide a means to "terraform" the build
container before producing a ZFSBootMenu image. For example, hooks might be
used to

- Copy additional dracut modules from `$BUILDROOT` into
  `/usr/lib/dracut/modules.d`, such as `dracut-crypt-ssh` for remote access
  over SSH.

- Write files that a dracut configuration in `${BUILDROOT}/dracut.conf.d`
  adds to the image with `install_items`, such as an
  `/etc/cmdline.d/dracut-network.conf` with network parameters.

The `rc.pre.d` hooks will execute after installing any requested packages in
the container, but before confirming the existence of (or populating) a
ZFSBootMenu repository at `/zbm`. These early hooks provide a means for,
*e.g.*, overriding the standard process for fetching ZFSBootMenu source
archives.

The `rc.d` hooks will execute after completion of the ZFSBootMenu setup process
described in the preceding section, but before the standard configuration is
modified according to any `-e` arguments provided to the container and
`generate-zbm` is execute. These late hooks provide a last-minute opportunity
to customize ZFSBootMenu configuration before creating an image.

## Build Examples

To use the previously created `localhost/zbm-builder` image to produce
ZFSBootMenu files from the default configuration, simply run

    podman run -v .:/build localhost/zbm-builder

After some console output, the container should terminate and the directory
`./build` should contain the UEFI bundle `vmlinuz.EFI` as well as the
components `vmlinuz-bootmenu` (a stock Ubuntu kernel) and corresponding
ZFSBootMenu initramfs `initramfs-bootmenu.img`.

To provide the hostid and pool cache files to the build container and run from
the `/etc/zfsbootmenu/build` directory, copy the desired files and run the
container with the appropriate volume mount:

    cp /etc/hostid /etc/zfsbootmenu/build
    podman run -v /etc/zfsbootmenu/build:/build localhost/zbm-builder

To create an image from a local repository available at `/sw/zfsbootmenu` and
again use a build root of `/etc/zfsbootmenu/build`, run

    podman run -v /etc/zfsbootmenu/build:/build -v /sw/zfsbootmenu:/zbm:ro localhost/zbm-builder

Because the build container does not modify the repository found in `/zbm`, it
is possible to mount that volume read-only (as indicated by the `:ro` suffix)
without consequence.
