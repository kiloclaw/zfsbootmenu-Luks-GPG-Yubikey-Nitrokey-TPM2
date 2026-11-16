Building with mkinitcpio
========================

ZFSBootMenu also supports the `mkinitcpio <https://gitlab.archlinux.org/archlinux/mkinitcpio/mkinitcpio/>`_ initramfs
generator used by Arch Linux and available for Void Linux, but it must be configured first.

Since `version 2.0.0 <https://github.com/zbm-dev/zfsbootmenu/releases/tag/v2.0.0>`_, ZFSBootMenu will install a standard
:zbm:`mkinitcpio.conf <etc/zfsbootmenu/mkinitcpio.conf>` in the ``/etc/zfsbootmenu`` configuration directory. This file
is generally the same as a standard ``mkinitcpio.conf``, except some additional declarations may be added to control
aspects of the ``zfsbootmenu`` mkinitcpio module. The configuration file includes extensive inline documentation in the
form of comments; configuration options specific to ZFSBootMenu are also described in the
:ref:`zfsbootmenu(7) <zbm-mkinitcpio-options>` manual page.

ZFSBootMenu still expects to use Dracut by default. To override this behavior and instead use mkinitcpio, edit
``/etc/zfsbootmenu/config.yaml`` and add the following options:

.. code-block:: yaml

  Global:
    InitCPIO: true
    ## NOTE: The following three lines are OPTIONAL
    InitCPIOHookDirs:
      - /etc/zfsbootmenu/initcpio
      - /usr/lib/initcpio

.. note::

  In some ZFSBootMenu guides, like :doc:`remote-access`, some mkinitcpio modules will be installed to
  ``/etc/zfsbootmenu/initcpio`` to keep them isolated from system-installed modules. To accommodate this non-standard
  installation, ``InitCPIOHookDirs`` must be defined in ``/etc/zfsbootmenu/config.yaml``. Furthermore, because
  overriding the hook directory causes mkinitcpio to ignore its default module path, the default ``/usr/lib/initcpio``
  must be manually specified. If all hooks are installed in ``/usr/lib/initcpio`` or ``/etc/initcpio``, the ZFSBootMenu
  configuration does **not** need to specify ``InitCPIOHookDirs``.

Without further changes, running ``generate-zbm`` should now produce a ZBM image based on mkinitcpio rather than Dracut.

Whenever ``generate-zbm`` is run to generate images based on mkinitcpio, it forcefully adds the required
``zfsbootmenu`` hook after any hooks defined in the ``HOOKS`` array of ``/etc/zfsbootmenu/mkinitcpio.conf``. Because
the ``zfsbootmenu`` hook completely takes over execution of its initramfs image, it is installed only at its **last**
entry in the hook list, and the run hook of every other hook in ``HOOKS`` runs before it, wherever it stands in the
array. The default configuration file explicitly includes the ``zfsbootmenu`` hook in the array as a visual reminder
that it will be included; mkinitcpio then lists the hook twice, and the earlier entry installs nothing. When running
mkinitcpio directly, without ``generate-zbm``, keep ``zfsbootmenu`` last in ``HOOKS``: the run hooks of hooks listed
after it never run, and the build warns of those hooks (``hooks after zfsbootmenu never run``).

mkinitcpio's ``init`` runs hooks in stages, and a hook may have a function for each. The early hooks of every hook in
``HOOKS`` run first, before the modules of ``MODULES`` are loaded and before the ``udev`` hook loads the drivers of the
devices it finds, so a network card whose driver is a module does not exist yet. The run hooks then run one after
another, in the order of ``HOOKS``, and the ``zfsbootmenu`` run hook, installed last, starts ZFSBootMenu. ZFSBootMenu
never returns to ``init``, so late and cleanup hooks never run: whatever ZFSBootMenu needs, such as a network
(:doc:`remote-access`), must be set up by a run hook.
