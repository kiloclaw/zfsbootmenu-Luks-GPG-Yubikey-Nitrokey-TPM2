# The English text of the menu (REFACTOR-ROADMAP.md, R5): every string a
# user reads, by message id. A translation is a copy of this file in
# locales/<language>/ with the same ids. Attributes `.title` hold the forms
# that titles, buttons and the toolbar use: title case is a rule of English.

## What a key does: the toolbar shows the title, the help view too.
action-boot = boot
    .title = Boot
action-choose = choose
    .title = Choose
action-details = details
    .title = Details
action-restore = restore
    .title = Restore
action-show-files = show files
    .title = Show Files
action-run = run
    .title = Run
action-confirm = confirm
    .title = Confirm
action-cancel = cancel
    .title = Cancel
action-back = back
    .title = Back
action-kernels = kernels
    .title = Kernels
action-snapshots = snapshots
    .title = Snapshots
action-edit = edit
    .title = Edit
action-default = default
    .title = Default
action-pools = pools
    .title = Pools
action-read-write = read/write
    .title = Read/Write
action-chroot = chroot
    .title = Chroot
action-sort = sort
    .title = Sort
action-log = log
    .title = Log
action-recovery = recovery
    .title = Recovery
action-power = power
    .title = Power
action-roll-back = roll back
    .title = Roll Back
action-clone = clone
    .title = Clone
action-clone-promote = clone+promote
    .title = Clone+promote
action-duplicate = duplicate
    .title = Duplicate
action-snapshot = snapshot
    .title = Snapshot
action-diff = diff
    .title = Diff
action-mark = mark
    .title = Mark
action-unlock = unlock
    .title = Unlock
action-import = import
    .title = Import
action-rewind = rewind
    .title = Rewind
action-filter = filter
    .title = Filter
action-commands = commands
    .title = Commands
action-help = help
    .title = Help
action-scroll = scroll
    .title = Scroll
action-select = select
    .title = Select
action-move = move
    .title = Move
action-redraw = redraw
    .title = Redraw
action-revert = revert
    .title = Revert
action-shell = shell
    .title = Shell
action-inherit = inherit
    .title = Inherit
action-keyboard-layout = keyboard layout
    .title = Keyboard Layout
action-show-typed = show what is typed
    .title = Show What Is Typed
action-segment = files/packages
    .title = Files/Packages
action-more = more
    .title = More

## Key names, as the toolbar and the help view show them.

key-enter = Enter
key-esc = Esc
key-tab = Tab
key-up = Up
key-down = Down
key-left = Left
key-right = Right
key-page-up = PgUp
key-page-down = PgDn
key-home = Home
key-end = End
key-space = Space
key-function = F{ $number }
key-ctrl = Ctrl-{ $key }
# A classic binding of the fzf menu: MOD is Alt or Ctrl.
key-classic = MOD+{ $letter }
keys-up-down = Up/Down
keys-left-right = Left/Right

## The frame around every view.

menu-too-small = The menu needs { $width }x{ $height } at least.
# The word before the outcome of the last action, in the notification line.
outcome-done = Done
outcome-warning = Warning
outcome-failed = Failed

## Lists: their count, the search and their empty states.

# How many items a list shows, while a search hides some.
list-count-of = { $shown } of { $total }
list-no-matches = No Matches
    .text = Nothing matches { $filter }. Esc clears the search.
quoted = “{ $text }”
quoted-ascii = "{ $text }"

## Start-up.

startup-title = Starting ZFSBootMenu
# The stage that runs, of all of them.
startup-step = { $current } of { $total }

## Snapshots, kernels and the command line.

snapshots-by = Snapshots by { $order }
sort-name = name
sort-creation = creation
sort-size = size
snapshots-none = No Snapshots
    .text = Press n to take one.
# What labels the default kernel, and the default environment.
label-default = default
kernels-title = Kernels
kernels-none = No Kernels
    .text = This boot environment has no kernel in /boot.
command-line-title = Command Line
command-line-with = Command Line with { $kernel }
command-line-hint = For this boot only: Enter boots with it, Esc keeps the saved one.
command-line-reverted = command line of { $environment } reverted
command-line-unclosed-quote = the command line has an unclosed " quote

## Pools and LUKS volumes.

pools-title = Pools and LUKS Volumes
pools-none = No Pools
    .text = No pool was found. Press ! for the recovery shell.
pool-not-imported = not imported
pool-read-write = read/write
pool-read-only = read-only
# The host ID of another host that last imported the pool.
pool-host = host { $hostid }
# Before a pool's name in the list, padded to eight columns.
pool-label = Pool
volume-label = Volume
volume-open = open
volume-locked = locked
volume-skipped = skipped
volume-failed = failed: { $reason }
volume-tpm2-changed = TPM2 changed

## Recovery tools and the log.

recovery-title = Recovery Tools
recovery-none = No Tools
tool-shell = Recovery shell
    .title = Recovery Shell
tool-chroot = Chroot into the selected environment
    .title = Chroot into the Selected Environment
tool-log = Log
    .title = Log
tool-pools = Pools and volumes
    .title = Pools and Volumes
tool-firmware-setup = Firmware setup
    .title = Firmware Setup
tool-reboot = Reboot
    .title = Reboot
tool-power-off = Power off
    .title = Power Off
log-title = Log Since Start
log-info = info
log-warning = warning
log-error = error

## Help.

help-keys = Keys
help-keys-scroll = Keys, Up/Down to Scroll
help-this-view = THIS VIEW
help-classic = CLASSIC BINDINGS (MOD IS ALT OR CTRL)

## Sizes and ages.

# A size in binary units: unit is the power of 1024.
size = { $value } { $unit ->
        [0] B
        [1] KiB
        [2] MiB
        [3] GiB
       *[4] TiB
    }
age-now = just now
age-minutes = { $count } min ago
age-hours = { $count } h ago
age-days = { $count } d ago

## The navigation bar: view titles, longest first, and the machine's status.

view-startup = Starting ZFSBootMenu
    .short = Starting
view-main = Boot Environments
    .short = Boot
view-snapshots = Snapshots
    .of = Snapshots of { $environment }
view-kernels = Kernels
    .of = Kernels of { $environment }
view-command-line = Command Line
    .of = Command Line of { $environment }
view-pools = Pools and Volumes
    .short = Pools
view-recovery = Recovery
view-log = Log
view-help = Help
view-changes = Changes
    .since = Changes since { $snapshot }
status-pools-ok = { $count } pools ok
    .short = pools ok
status-volumes-open = { $open }/{ $total } volumes open
    .short = { $open }/{ $total } open
status-log-problems = { $count } in the log

## The main view: the list of boot environments and its inspector.

environments-none = No Boot Environments
    .text = No imported pool has a boot environment. Press p for the pools, ! for recovery.
    .notice = no boot environment found: see the pools, or open the recovery shell
label-locked = locked
label-boots = boots
kernel-none = no kernel
value-none = none
value-empty = (empty)
details-title = Details
details-none = No Selection
details-kernels = KERNELS
details-newest-snapshots = NEWEST SNAPSHOTS
details-checks = CHECKS
# The labels of the inspector's facts, right-aligned in twelve columns.
fact-checks = Checks
fact-boot-tries = Boot Tries
fact-next-boot = Next Boot
fact-pool = Pool
fact-kernel = Kernel
    .count = { $count ->
        [one] { $count } kernel
       *[other] { $count } kernels
    }
fact-snapshots = Snapshots
    .newest = newest { $snapshot }, { $age }
fact-encryption = Encryption
fact-command-line = Command Line
encryption-unlocked = key loaded
encryption-locked = : the key is asked at boot
origin-local = set here
origin-inherited = from { $from }
origin-edited = edited for this boot

## Boot health: the banner, the badges of a row and the checks.

countdown-seconds = { $seconds } s
countdown-hint = Press any key to choose another.
banner-countdown = Booting { $environment } in { $seconds } s. Press any key to choose another.
    .short = Booting { $environment } in { $seconds } s.
    .shortest = Booting in { $seconds } s.
banner-once = Booting { $environment } once, as the running system asked, in { $seconds } s.
    .short = Booting { $environment } once in { $seconds } s.
    .shortest = Booting in { $seconds } s.
banner-used-up = { $failed } failed to start { $attempts } times; its boot tries are used up.
    .short = { $failed } failed to start { $attempts } times.
banner-fallback = { $failed } failed to start { $attempts } times. Booting { $target } in { $seconds } s.
    .shorter = { $failed } failed { $attempts } times. Booting { $target } in { $seconds } s.
    .short = { $failed } failed { $attempts } times. Fallback in { $seconds } s.
    .shortest = Fallback in { $seconds } s.
fallback-target-environment = { $environment }, the last good one
fallback-target-clone = a clone of { $snapshot }
badge-problems = { $count ->
        [one] { $count } problem
       *[other] { $count } problems
    }
badge-warnings = { $count ->
        [one] { $count } warning
       *[other] { $count } warnings
    }
badge-next-boot = next boot
checks-none = not checked
checks-passed = all { $count } passed
tries-left = { $left } of { $total } left
next-boot-once = once, as the running system asked

## Dialogs: buttons, confirmations, errors and the command palette.

button-cancel = Cancel
button-ok = OK
button-continue = Continue
button-roll-back = Roll Back
button-promote = Promote
button-rewind = Rewind
button-force-import = Force Import
button-restore = Restore
button-reseal = Re-seal
# Before what the user types to confirm a destructive action.
confirm-type = Type { $name } to confirm:
error-you-can = You can:
palette-title = Commands
palette-boot = Boot { $environment }
palette-edit = Edit the command line of { $environment }
palette-reset-tries = Give { $environment } Its Boot Tries Back
palette-boot-clone = Boot a clone of { $snapshot }
palette-recovery-tools = Recovery tools

## The changes since a snapshot: files and packages.

segment-files = Files
segment-packages = Packages
changes-none = No Changes
    .text = Nothing changed since the snapshot.
preview-title = Preview
change-removed = removed
change-created = created
change-modified = modified
change-renamed = renamed
preview-created = Created after the snapshot: there is nothing to restore.
preview-removed = Removed since the snapshot: restoring brings it back.
preview-renamed = Renamed to { $to }: restoring brings back the old name.
preview-binary = A binary file: no text to show.
packages-since = Packages since { $snapshot }
    .manager = { $manager } Packages since { $snapshot }
packages-none = No Package Changes
    .text = The snapshot has the same packages.
packages-no-database = No Package Database
    .text = No xbps, dpkg, pacman or rpm database was found in the snapshot.
package-title = Package
package-installed = installed
    .since = Installed since the snapshot: { $versions }.
package-removed = removed
    .since = Removed since the snapshot, which has { $versions }. Booting a clone of the snapshot, or rolling back, brings it back with its package record.
package-upgraded = upgraded
    .since = Upgraded { $versions }.
package-downgraded = downgraded
    .since = Downgraded { $versions }.
package-no-files = None of the files that changed belongs to it.
package-changed-files = CHANGED FILES
package-files-hint = Enter shows them among the files.

## Unlocking a LUKS volume.

unlock-title = Unlock { $volume }
unlock-opened = { $volume } is open
unlock-skipped = { $volume } stays locked
unlock-failed = unable to unlock { $volume }
unlock-device = on { $device }
button-skip = Skip
button-unlock = Unlock
# How a sentence names a key method; `.label` labels the field of a typed one.
method-passphrase = passphrase
    .label = Passphrase
method-tpm2-pin = TPM2 PIN
    .label = TPM2 PIN
method-tpm2 = TPM2
method-clevis = Clevis
method-fido2 = security key
method-yubikey = YubiKey
method-gnupg = key file passphrase
    .label = Key file passphrase
method-key-device = key device
unlock-waiting = Waiting for the { $method }: touch it, or plug it in.
unlock-tries-left = { $count ->
        [one] { $count } try left
       *[other] { $count } tries left
    }
unlock-other-methods = { $keys } to use the { $methods } instead
# Between the other methods.
unlock-or = { " " }or{ " " }
unlock-show = Ctrl-R: show
unlock-hide = Ctrl-R: hide
keyboard-terminal = Keyboard: the terminal's own
keyboard-layout = Keyboard: { $layout }
keyboard-change = Ctrl-K: layout
keyboard-wrong = Keyboard: { $layout }. Set with another layout? Ctrl-K changes it.

## Pools: their dialog, importing and rewinding.

pool-title = Pool { $pool }
pool-last-host = last imported by host { $hostid }
pool-import-read-write = Import Read/Write
pool-import-last-host = Import with Its Last Host ID
pool-import-this-host = Import with This Host ID
pool-force-import = Force the Import
pool-rewind = Rewind to the Checkpoint
import-failed =
    .title = { $pool } Did Not Import
import-retry = Retry
import-title = Import { $pool }
import-shell = Open the Recovery Shell
force-import = { $pool } was last imported by another host.
    .title = Force the Import of { $pool }
    .risk = If that host is still running and using it, forcing the import can corrupt it.
rewind = Every change to { $pool } since its checkpoint is lost.
    .title = Rewind { $pool } to Its Checkpoint
    .discarded = The checkpoint itself is discarded.
error-nothing-to-confirm = nothing to confirm
confirm-type-first = type { $name } to confirm

## Power.

power-title = Power
power-off = Power Off
power-reboot = Reboot
power-firmware-setup = Reboot into Firmware Setup

## Booting an environment whose checks failed.

may-not-boot =
    .title = { $environment } May Not Boot
    .anyway = Boot Anyway

## The actions on a snapshot.

snapshot-title = Snapshot { $snapshot }
snapshot-boot-clone = Boot a Temporary Clone
snapshot-roll-back = Roll Back to It
snapshot-clone = Clone as a New Boot Environment
snapshot-duplicate = Duplicate as an Independent Copy
snapshot-promote = Clone and Promote
snapshot-diff = Show Differences to Now
snapshot-chroot = Chroot into It, Read-Only
rollback = { $environment } goes back to { $snapshot }: every change since is lost.
    .title = Roll Back { $environment }
    .none-destroyed = No newer snapshot is destroyed.
    .destroyed = { $count ->
        [one] One newer snapshot is destroyed ({ $size }): { $newest }.
       *[other] { $count } newer snapshots are destroyed ({ $size }), the newest { $newest }.
    }
    .safety = A safety snapshot { $safety } is taken first: roll back to it to undo.
promote = The clone takes over the snapshots of { $environment } up to { $snapshot }.
    .title = Clone and Promote { $snapshot }
    .depends = { $environment } then depends on the clone, and cannot be destroyed without it.

## Restoring files from a snapshot.

restore = They replace the files in { $environment } as they are now.
    .title = { $count ->
        [one] Restore { $count } File from { $snapshot }
       *[other] Restore { $count } Files from { $snapshot }
    }
    .more = and { $count } more
    .safety = { $pool } is imported read/write, and a safety snapshot { $safety } is taken first: roll back to it to undo.
restore-nothing = { $path } was created after { $snapshot }: there is nothing to restore

## Keyboard layouts and sealing a TPM2 key again.

layout-serial = keys come from a serial terminal: its own layout applies
layout-only-one = this image knows one keyboard layout
layout-typing = typing with the { $layout } layout
reseal = Re-sealing trusts the machine as it is now (PCRs { $pcrs }).
    .title = Re-seal the TPM2 Key of { $volume }
    .only-yours = Do it only for changes you made; else keep the passphrase.
pcr = PCR { $index } ({ $name })
pcr-firmware-code = firmware code
pcr-firmware-settings = firmware settings
pcr-option-roms = option ROMs
pcr-option-rom-settings = option ROM settings
pcr-boot-loader = boot loader
pcr-partition-table = partition table
pcr-secure-boot = Secure Boot state
pcr-initramfs = initramfs
pcr-kernel-image = unified kernel image
pcr-command-line = kernel command line
pcr-shim = shim certificates
pcr-system-identity = system identity
pcr-other = other measurements
