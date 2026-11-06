# Hand-written fixtures

Not recorded: `cargo xtask record-zfs` builds a pool on one healthy disk, which has none of what these show. Each
file is written to the print formats of OpenZFS 2.3 and 2.4 (`zpool_main.c`: `status_callback`,
`status_callback_json`, `vdev_stats_nvlist`, `print_status_config`, `print_class_vdevs`, `show_import`,
`print_import_config`; `dump_nvlist` of `libnvpair` for `zdb`), with the column widths those functions compute, so
that a recording of the same pools would match them.

- `status-degraded.txt`: `zpool status -p -P` of a mirror with a missing device (`UNAVAIL`, `was <path>`), data
  errors, and a vdev of each class (`special`, `logs`, `cache`, `spares`), then a healthy pool, then a mirror with a
  device faulted (`too many errors`) and a spare that cannot be opened.
- `status-degraded.json`: the same pools as `zpool status -j -p -P` prints them, the states of the kernel
  (`CANT_OPEN`) and the reasons (`aux`) where the text has its words.
- `exported-foreign.txt`: `zpool import` listing a pool last used by another system, a degraded pool with a log
  device, and a second pool with the same name.
- `exported-2.1.txt`: the first pool of the listing in the alignment of OpenZFS 2.1 and 2.2, one more space before
  each key but `status:`.
- `zdb-config.txt`: `zdb -e -C <id>` of the foreign pool, its host ID 0xdeadbeef.
- `no-pools.err`: what `zpool import` writes to standard error when it finds no pool, with nothing on standard
  output and exit status 0.
