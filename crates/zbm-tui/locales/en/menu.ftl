# The English text of the menu (ROADMAP.md, R5): every string a
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
# Enter on a device of the workspace's Pools tab: the kernel's messages
# about it.
action-messages = kernel messages
    .title = Messages
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
action-destroy = destroy
    .title = Destroy
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
action-fold = fold or unfold
    .title = Fold
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
action-tab-boot = the Boot tab
    .title = Boot
action-tab-snapshots = the Snapshots tab, of the selected environment
    .title = Snapshots
action-tab-pools = the Pools tab
    .title = Pools
action-tab-log = the Log tab
    .title = Log
action-next-tab = the next tab
    .title = Next Tab
action-previous-tab = the previous tab
    .title = Previous Tab
# Enter on a review page that refuses its change.
action-close = close
    .title = Close
# Tab and Shift-Tab on a review page: from the field to the buttons and
# back to the field.
action-next-place = the next button, or the field
    .title = Next
action-previous-place = the previous button, or the field
    .title = Previous
action-more = more
    .title = More
# Backspace and Ctrl-U in a field that takes text.
action-erase = erase the last character
    .title = Erase
action-clear = clear the field
    .title = Clear
# Tab, Ctrl-W and Ctrl-R in the command line (:).
action-complete = complete the query to the selected command
    .title = Complete
action-delete-word = delete the last word of the query
    .title = Delete Word
action-recent = show only the commands chosen before, or all again
    .title = Recent

## Key names, as the toolbar and the help view show them.

key-enter = Enter
key-esc = Esc
key-tab = Tab
key-shift-tab = Shift-Tab
key-up = Up
key-down = Down
key-left = Left
key-right = Right
key-page-up = PgUp
key-page-down = PgDn
key-home = Home
key-end = End
key-delete = Del
key-backspace = Backspace
key-space = Space
key-function = F{ $number }
key-ctrl = Ctrl-{ $key }
# A classic binding of the fzf menu: MOD is Alt or Ctrl.
key-classic = MOD+{ $letter }
# A classic binding whose Ctrl form means something else in the menu.
key-alt = Alt+{ $letter }
keys-up-down = Up/Down
keys-left-right = Left/Right

## The frame around every view.

menu-too-small = The menu needs { $width }x{ $height } at least.
# The word before the outcome of the last action, in the notification line.
outcome-done = Done
outcome-warning = Warning
outcome-failed = Failed
outcome-cancelled = Cancelled
    .what = what was done before it stays
# An action that runs while the menu goes on, in the notification line.
working = Working for { $seconds } s; Esc cancels

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
# The questions start-up asks: Enter goes on, as when the time runs out, Esc
# stops.
question-unlock = No passphrase opened it.
    .title = { $volume } Did Not Open
question-missing-volume = The LUKS device { $device } is not there yet.
    .title = { $device } Is Missing
question-missing-devices = A device that zbm.wait_for names is not there yet.
    .title = Devices Are Missing
question-import = Its devices may not be there yet, or another host may hold it.
question-import-none =
    .title = No Pool Imported
question-unsupported = A pool has features this build of ZFS does not support, so it stays read-only.
    .title = Unsupported Features
question-retrying = { $seconds ->
        [one] Trying again in { $seconds } second.
       *[other] Trying again in { $seconds } seconds.
    }
question-looking = { $seconds ->
        [one] Looking again in { $seconds } second.
       *[other] Looking again in { $seconds } seconds.
    }
question-continuing = { $seconds ->
        [one] Continuing in { $seconds } second.
       *[other] Continuing in { $seconds } seconds.
    }
button-try-again = Try Again
button-look-again = Look Again
button-stop-waiting = Stop Waiting

## Snapshots, kernels and the command line.

snapshots-by = Snapshots by { $order }
sort-name = name
sort-creation = creation
sort-size = size
snapshots-none = No Snapshots
    .text = Press n to take one.
# The snapshots column before its snapshots came: the machine lists them,
# or it did not, as Esc stopped it.
snapshots-reading = Reading the Snapshots...
    .text = The machine lists them; Esc stops it.
snapshots-not-read = Snapshots Not Read
    .text = Open the snapshots again to read them.
# The Snapshots tab chosen during start-up, which has no boot environment
# to show the snapshots of yet.
snapshots-after-startup = the snapshots show once start-up is over
# The workspace's snapshots as a timeline, in groups by when they were
# taken, newest first: today, the days of this week, the weeks of this
# month, then the months. Each header says how many snapshots its group
# holds and the space they hold.
timeline-environments = Environments
timeline-today = Today
timeline-day = { $weekday } { $day } { $month }
timeline-week = Week of { $day } { $month }
timeline-month = { $month } { $year }
timeline-month-name = { $month ->
        [1] January
        [2] February
        [3] March
        [4] April
        [5] May
        [6] June
        [7] July
        [8] August
        [9] September
        [10] October
        [11] November
       *[12] December
    }
timeline-weekday-name = { $weekday ->
        [1] Monday
        [2] Tuesday
        [3] Wednesday
        [4] Thursday
        [5] Friday
        [6] Saturday
       *[7] Sunday
    }
timeline-count = { $count ->
        [one] { $count } snapshot
       *[other] { $count } snapshots
    }
timeline-folded = folded
# What the line view says after a snapshot marked for a diff.
snapshot-marked = marked
timeline-open-group = open the group with Enter to choose one of its snapshots
# A diff compares two marked snapshots, or one with now; never more. Short,
# so that the warning stays whole on a 52x20 screen.
diff-marks = { $count } marked: a diff compares two
# The marked snapshots' list is not read: the menu cannot tell their order.
diff-unread = Marked snapshots not read: no diff
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
command-line-save-title = Command Line for Every Boot
command-line-save-hint = For every boot: Enter shows what changes, Esc keeps the saved one.
command-line-control = the command line holds a control character, which cannot be saved
command-line-too-long = the command line is longer than the { $longest } bytes ZFS keeps

## Pools and LUKS volumes.

pools-title = Pools and LUKS Volumes
pools-none = No Pools
    .text = No pool was found. Press ! for the recovery shell.
pool-not-imported = not imported
pool-read-write = read/write
pool-read-only = read-only
# The host ID, or the host name, of another host that last imported the
# pool.
pool-host = host { $hostid }
# The host name and host ID of another host that last imported the pool.
pool-host-named = host { $hostname } ({ $hostid })
pool-host-unknown = another host
# The numeric identifier of a pool that shares its name with another.
pool-id = id { $id }
# Before a pool's name in the list, padded to eight columns.
pool-label = Pool
volume-label = Volume
volume-open = open
volume-locked = locked
volume-skipped = skipped
volume-failed = failed: { $reason }
volume-tpm2-changed = TPM2 changed
# The header of the volumes of no pool listed, in the workspace's Pools tab.
pools-no-pool = No pool
# The headings `zpool status` prints above the devices of a pool that hold
# something other than its data, in the workspace's Pools tab: the
# deduplication table, metadata, the intent log, the read cache and the
# hot spares.
pools-class-dedup = dedup
pools-class-special = special
pools-class-log = logs
pools-class-cache = cache
pools-class-spare = spares

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
log-startup-title = Start-Up So Far
log-startup-later = The log since start shows once start-up is over.

## The boot history, above the log in the workspace's Log tab.

history-title = Saved Boots
history-reading = Reading the saved boots...
history-off = No boot is kept without zbm.save_logs.
history-off-how = It keeps the last five: see zfsbootmenu(7).
history-unavailable = The saved boots cannot be read: { $why } ({ $code }).
history-none = No boot is saved yet.
history-loaded = { $when }  { $environment }  { $kernel }  { $measured }
history-measured = measured
history-not-measured = not measured
history-no-record = { $name }  saved without a record, from the shell or after a crash
history-unreadable = { $name }  its record cannot be read: { $why }

## Help.

help-keys = Keys
help-keys-scroll = Keys, Up/Down to Scroll
help-this-view = THIS VIEW
help-classic = CLASSIC BINDINGS (MOD IS ALT OR CTRL)
# The workspace's help: the version of the ZFS module running, and the
# `MOD+key` aliases of the fzf menu, named apart from `zbm.menu=classic`.
help-zfs-version = ZFS { $version }
help-aliases = FZF ALIASES (MOD IS ALT OR CTRL)
# The help of the command line: the names it finds its commands by.
help-command-names = COMMAND NAMES, THEN WHAT THEY ACT ON

## The key section of zfsbootmenu(7), which `cargo xtask gen-docs` writes:
## the places that have no view of their own.

manual-every-view = Every View
manual-sheets = Sheets and Alerts
manual-fields = Fields That Take Text
manual-command-line = The Command Line
manual-command-names = Command Names
manual-command-names-intro = The command line finds a command by its name, in the menu's language or in English, then by the name of what it acts on: an environment, a snapshot, a kernel, a pool, a volume or a view. Tab completes the selected entry's command and target, and F1 lists the names.
# A classic binding of the fzf menu that works with Alt only: its Ctrl form
# is a key of the menu, as Ctrl-P is Select.
manual-alt-only = Alt only: { $key } is { $action }.
# A classic binding whose Ctrl form a terminal sends as another key.
manual-sent-as = On a serial line or over SSH, { $key } arrives as { $sent }: use Alt.
# What the moves do, with their way: the help view says "Select" for both.
manual-select-up = select the row above
manual-select-down = select the row below
manual-scroll-up = scroll up
manual-scroll-down = scroll down
manual-page-up = move a page up
manual-page-down = move a page down
manual-top = move to the top
manual-bottom = move to the bottom
# Where a key works, when only some of a place's dialogs or fields read it.
manual-in-choices = in a list of choices; a review page scrolls
manual-on-review-pages = on a review page
manual-in-lists = in a list's filter and the methods of the unlock prompt
manual-in-filters = in a list's filter
manual-in-unlock = in the unlock prompt, once start-up is over

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
# The tab bar of the workspace, from 120 columns: the host ID ZFS uses and
# the time of day, in UTC as the boot image has no time zone.
status-host = host { $id }
status-time = { $time } UTC

## The workspace of the redesign (ADR 0007): its tab bar, sidebar and
## command hint.

workspace-name = ZFSBootMenu
workspace-hint = : commands   / filter   ? every key
sidebar-pools = Pools
sidebar-volumes = Volumes
sidebar-attention = Attention
sidebar-open = { $open } of { $total }
sidebar-ro = ro
sidebar-rw = rw
sidebar-not-imported = not imported
attention-none = nothing
attention-foreign = { $pool }: host { $hostid }
# A pool another host used last, whose host could not be read.
attention-foreign-unknown = { $pool }: another host
attention-not-imported = { $pool }: not imported
attention-checkpoint = { $pool }: checkpoint
# A device of a pool that is missing: its name, then UNAVAIL, REMOVED or
# FAULTED.
attention-device = { $pool }: { $device } { $state }
attention-log-errors = { $count ->
        [one] { $count } error in the log
       *[other] { $count } errors in the log
    }
attention-log-warnings = { $count ->
        [one] { $count } warning in the log
       *[other] { $count } warnings in the log
    }
attention-failed = failed
attention-stage = { $stage } failed
details-next-steps = NEXT STEPS

## The main view: the list of boot environments and its inspector.

environments-none = No Boot Environments
    .text = No imported pool has a boot environment. Press p for the pools, ! for recovery.
    .notice = no boot environment found: see the pools, or open the recovery shell
# The same hint in the workspace, with the keys of its map: pools is the key
# of the Pools tab, recovery the key of the recovery tools.
environments-none-workspace = No imported pool has a boot environment. Press { $pools } for the pools, { $recovery } for recovery.
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
fact-environment = Environment
fact-kernel = Kernel
    .count = { $count ->
        [one] { $count } kernel
       *[other] { $count } kernels
    }
fact-snapshots = Snapshots
    .newest = newest { $snapshot }, { $age }
fact-last-boot = Last Boot
    .loaded = Loaded { $when } with { $kernel }
fact-encryption = Encryption
fact-command-line = Command Line
fact-taken = Taken
    .when = { $date } { $time } UTC, { $age }
fact-used = Used
fact-health = Health
fact-kind = Kind
fact-path = Path
fact-host = Last Host
    .named = { $hostname } ({ $hostid })
    .unknown = unknown
fact-status = Status
fact-action = Action
fact-errors = Errors
    .counts = { $read } read, { $write } write, { $cksum } checksum
fact-data-errors = Data Errors
fact-note = Note
fact-device = Device
fact-state = State
fact-devices = Devices
fact-class = Class
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
fallback-target-environment = { $environment }, the last good one,
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
button-destroy = Destroy
button-promote = Promote
button-rewind = Rewind
button-force-import = Force Import
button-restore = Restore
button-reseal = Re-seal
button-create = Create
# Before what the user types to confirm a destructive action.
confirm-type = Type { $name } to confirm:
error-you-can = You can:
palette-title = Commands
# The palette's title while Ctrl-R shows only the commands chosen before.
palette-recent = Recent
palette-boot = Boot { $environment }
palette-edit = Edit the command line of { $environment }
palette-reset-tries = Give { $environment } Its Boot Tries Back
palette-boot-clone = Boot a clone of { $snapshot }
palette-recovery-tools = Recovery tools
palette-rescan-pools = Rescan the Pools
palette-export = Export { $pool }
palette-destroy = Destroy { $snapshot }
palette-destroy-marked = { $count ->
    [one] Destroy the Marked Snapshot
   *[other] Destroy the { $count } Marked Snapshots
}
palette-revert = Revert the Command Line of { $environment }
palette-default = Make { $environment } the Default
palette-snapshot = Take a Snapshot of { $environment }
palette-chroot = Chroot into { $target }
palette-default-kernel = Make { $kernel } the Default Kernel of { $environment }
palette-inherit = Inherit the Default Kernel of { $environment }
palette-set-command-line = Set the Command Line of { $environment }
palette-inherit-command-line = Let { $environment } Inherit Its Command Line
palette-roll-back = Roll Back to { $snapshot }
palette-clone = Clone { $snapshot }
palette-duplicate = Duplicate { $snapshot }
palette-promote = Clone { $snapshot } and Promote the Clone
palette-diff = Show Differences from { $snapshot } to Now
palette-read-write = Import { $pool } Read/Write
palette-read-only = Import { $pool } Read-Only
palette-import = Import { $pool }
palette-import-last-host = Import { $pool } with Its Last Host ID
palette-import-force = Force the Import of { $pool }
palette-rewind = Rewind { $pool } to Its Checkpoint
palette-unlock = Unlock { $volume }
palette-go-environment = Go to { $environment }
palette-go-pool = Go to the Pool { $pool }
palette-go-volume = Go to the Volume { $volume }
# The help of a view, or of the command line, by its name.
palette-help = Help: { $view }
# The sections of the command line, drawn above their entries.
section-recent = Recent
section-actions = Actions
section-items = Items
section-help = Help
# What a command asks for before it acts, drawn after its keys: a review
# page that asks for its target's name, the read/write import of a pool
# imported read-only, the key of a locked environment, and the PIN of a
# locked menu, which is also the lock's mark after the toolbar's label of a
# key the lock keeps (ADR 0010).
cost-review = review
cost-writes = read/write
cost-key = key
cost-pin = PIN
# The command line, as help names what Esc returns to.
view-commands = Command Line
# The names of the command line's commands, which it finds in the menu's
# language and in English, before what they act on: "boot zroot/ROOT/void".
# Short, lower case, one word where it can be; what follows says what each
# does, for its help and zfsbootmenu(7).
command-boot = boot
    .what = boot an environment, or a new clone of a snapshot
command-edit = edit
    .what = edit the command line an environment boots with, until reverted
command-revert = revert
    .what = go back to an environment's own command line
command-default = default
    .what = make an environment, or a kernel of it, the default
command-inherit = inherit
    .what = inherit the default kernel of an environment
command-commandline = commandline
    .what = set the command line an environment boots with, for every boot
command-inherit-commandline = inherit commandline
    .what = let an environment inherit its command line again
command-snapshot = snapshot
    .what = take a snapshot of an environment
command-chroot = chroot
    .what = open a shell in an environment or a snapshot
command-tries = tries
    .what = give an environment its boot tries back
command-rollback = rollback
    .what = roll an environment back to a snapshot
command-clone = clone
    .what = clone a snapshot as a new boot environment
command-promote = promote
    .what = clone a snapshot and promote the clone
command-duplicate = duplicate
    .what = duplicate a snapshot as an independent copy
command-diff = diff
    .what = show the differences from a snapshot to now
command-destroy = destroy
    .what = destroy the selected snapshot, or the marked ones
command-export = export
    .what = export a pool
command-import = import
    .what = import a pool with this host's ID
command-import-last-host = import -l
    .what = import a pool with the host ID that last imported it
command-import-force = import -f
    .what = force the import of a pool another host may use
command-rewind = rewind
    .what = rewind a pool to its checkpoint
command-read-write = readwrite
    .what = import a pool read/write
command-read-only = readonly
    .what = import a pool read-only
command-unlock = unlock
    .what = unlock a LUKS volume
command-reseal = reseal
    .what = seal a volume's TPM2 key again
command-rescan = rescan
    .what = scan for pools again
command-shell = shell
    .what = open the recovery shell
command-power = power
    .what = followed by { command-power.off }, { command-power.reboot } or { command-power.firmware }: power off, reboot, or reboot into the firmware setup
    .off = off
    .reboot = reboot
    .firmware = firmware
command-sort = sort
    .what = sort the snapshots another way
command-fold = fold
    .what = fold the group of the selected snapshot, or unfold it
command-mark = mark
    .what = mark the selected snapshot, or unmark it
command-filter = filter
    .what = filter the list
command-go = go
    .what = go to an environment, a pool or a volume
command-kernels = kernels
    .what = show the kernels of an environment
command-snapshots = snapshots
    .what = show the snapshots of an environment
command-pools = pools
    .what = show the pools and volumes
command-recovery = recovery
    .what = show the recovery tools
command-log = log
    .what = show the log
command-help = help
    .what = show the keys of a view, or of the command line

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
method-fido2-pin = security key PIN
    .label = Security key PIN
method-yubikey-passphrase = YubiKey passphrase
    .label = Passphrase
method-openpgp-card = OpenPGP card
method-openpgp-card-pin = card PIN
    .label = Card PIN
unlock-waiting = Waiting for the { $method }: touch it, or plug it in.
# How long start-up still waits for the device.
unlock-wait-left = { $seconds ->
        [one] { $seconds } second left
       *[other] { $seconds } seconds left
    }
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
pool-import-read-only = Import Read-Only
pool-import-last-host = Import with Its Last Host ID
pool-import-this-host = Import with This Host ID
pool-force-import = Force the Import
pool-rewind = Rewind to the Checkpoint
pool-no-checkpoint = { $pool } has no checkpoint to rewind to.
pool-shadowed = A pool named { $pool } is imported: export it to import this one.
pool-read-write-already = { $pool } is imported read/write already.
pool-export = Export

## A device's kernel messages, in the workspace's Pools tab.

device-log-title = Kernel messages of { $device }
device-log-unknown = The kernel lists no block device { $device }.
device-log-none = No kernel message names { $names }.
device-log-ports = Messages that name only a port, as ata3.00 or usb 1-2 do, are not shown.
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
not-offered = not available on this machine
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
snapshot-boot-clone = Boot a New Clone
snapshot-roll-back = Roll Back to It
snapshot-clone = Clone as a New Boot Environment
snapshot-duplicate = Duplicate as an Independent Copy
snapshot-promote = Clone and Promote
snapshot-diff = Show Differences to Now
snapshot-diff-marked = Show Differences between the Marked
snapshot-chroot = Chroot into It, Read-Only
snapshot-destroy = Destroy It
rollback = { $environment } goes back to { $snapshot }: every change since is lost.
    .title = Roll Back { $environment }
    .none-destroyed = No newer snapshot is destroyed.
    .destroyed = { $count ->
        [one] One newer snapshot is destroyed ({ $size }): { $newest }.
       *[other] { $count } newer snapshots are destroyed ({ $size }), the newest { $newest }.
    }
    .undo = It cannot be undone: to keep things as they are, clone the snapshot instead.
promote = The clone takes over the snapshots of { $environment } up to { $snapshot }.
    .title = Clone and Promote { $snapshot }
    .depends = { $environment } then depends on the clone, and cannot be destroyed without it.

## The review page of a change (ADR 0016, points 12 to 15): what it loses,
## keeps and allows, as the machine plans it, then the target's name typed.

review-title = Review the Change
review-rollback = Roll Back { $environment } to { $snapshot }
review-destroy = { $count ->
    [one] Destroy { $snapshot }
   *[other] Destroy { $count } Snapshots of { $environment }
}
review-planning = Reading what it would change…
review-wait = wait for what it would change to be read
review-refused = ZFS would refuse this:
review-refused-clone = { $snapshot } has the clone { $clone }: destroy or promote the clone first.
review-refused-hold = { $snapshot } is held by { $tag }: release the hold first.
review-refused-held = { $snapshot } is held: release its holds first.
review-refused-active = { $pool } is imported by { $host } now: forcing the import would corrupt it.
# Headers, in capitals as the other sections of the menu.
review-lost = WHAT IS LOST
review-steps = STEPS
review-pool-use = WHO ELSE MAY USE IT
# Which rows of what is lost show, when not all do.
review-lost-rows = { $first } to { $last } of { $count }
review-total = { $size } in all
review-lost-none = Nothing.
review-lost-bookmark = the bookmark { $bookmark }
review-lost-written = what was written since { $snapshot }
review-lost-newer = every snapshot and bookmark newer than { $snapshot }, which could not be listed
review-lost-checkpoint = every write since the checkpoint of { $created }
review-lost-checkpoint-unknown = every write since the checkpoint
review-lost-discarding = the checkpoint is being discarded: the rewind may fail
review-lost-dataset = the filesystem { $dataset }, made since
review-lost-datasets-unknown = the filesystems made since, which could not be listed
review-lost-snapshot = the snapshot { $snapshot }, taken since
review-lost-snapshots-unknown = the snapshots taken since, which could not be listed
# The line view's rows of what is lost stop after 20; what stops a change,
# where the page has no more room.
review-more = and { $count } more
review-freed = { $count ->
    [one] Destroying the snapshot frees { $size }.
   *[other] Destroying the snapshots frees { $size }.
}
review-kept = Kept: { $kept }.
review-kept-snapshots = { $count ->
    [one] one older snapshot
   *[other] { $count } older snapshots
}
review-kept-bookmarks = { $count ->
    [one] one older bookmark
   *[other] { $count } older bookmarks
}
review-kept-every-bookmark = { $count ->
    [one] its bookmark
   *[other] its { $count } bookmarks
}
review-no-undo = No undo: what is lost cannot be brought back.
review-undo = It can be undone.
review-undo-export = Undo: export { $pool }.
review-undo-destroy = Undo: destroy { $snapshot }.
# $access is pool-read-only or pool-read-write.
review-undo-import = Undo: import { $pool } { $access } again.
review-undo-default = Undo: make { $environment } the default again.
review-undo-inherit-kernel = Undo: let { $environment } inherit its default kernel again.
review-undo-command-line = Undo: set the command line of { $environment } back to { $line }.
review-undo-inherit-command-line = Undo: let { $environment } inherit its command line again.
review-host-id-stays = No undo: the host ID it adopts stays adopted, even once the pool is exported.
review-safer-clone = Safer: clone { $snapshot } as a new boot environment, and keep everything.
review-safer-import = Safer: import it read-only, without the rewind, and lose nothing.
review-button-clone = Clone Instead
review-button-read-only = Import Read-Only
review-this-host = This host has it imported: no other host uses it now.
review-host = Last imported by { $host }.
review-host-unknown = The host that last imported it is unknown.
review-written = Last written { $age }, on { $date }.
review-active = Another host has it imported now.
# The steps of a change, each followed by its command.
review-step-read-write = import { $pool } read/write
review-step-load-key = load the key of { $dataset }
review-step-default = make { $environment } the default of { $pool }
review-step-set = set { $property } of { $target }
review-step-inherit = { $dataset } inherits { $property }
review-step-snapshot = take { $snapshot }
review-step-rollback = roll { $dataset } back to { $snapshot }
review-step-destroy = { $count ->
    [one] destroy one snapshot
   *[other] destroy { $count } snapshots
}
review-step-clone = copy { $snapshot } as { $target }
review-step-promote = promote { $dataset }
review-step-inherit-key = { $dataset } takes its parent's key
review-step-export = export { $pool }
# $access is pool-read-only or pool-read-write.
review-step-import = import { $pool } { $access }
review-step-force-import = force the import of { $pool } { $access }
review-step-rewind = import { $pool } rewound to its checkpoint
# Only when the import with this machine's host ID fails.
review-step-adopt-hostid = if the import fails, take the host ID that last imported { $pool }
review-step-boot = boot { $environment }
review-step-restore = { $count ->
    [one] restore one path from { $snapshot }
   *[other] restore { $count } paths from { $snapshot }
}

## The dry run of a change chosen on the command line (ADR 0007, point 5;
## ADR 0016, point 12): its review page without the field.

dry-run-asks = Nothing has changed yet: Enter runs it, Esc leaves it.
dry-run-loses-nothing = Nothing is lost, and the menu has no undo for it.
dry-run-snapshot = Take the Snapshot { $snapshot }
dry-run-clone = Clone { $snapshot } as { $environment }
dry-run-clone-promote = Clone { $snapshot } as { $environment } and Promote It
dry-run-duplicate = Duplicate { $snapshot } as { $environment }
dry-run-boot-clone = Boot a Clone of { $snapshot } as { $environment }
dry-run-default = Make { $environment } the Default
dry-run-kernel = Make { $kernel } the Default Kernel of { $environment }
dry-run-inherit-kernel = Let { $environment } Inherit Its Default Kernel
dry-run-command-line = Save the Command Line of { $environment } for Every Boot
dry-run-inherit-command-line = Let { $environment } Inherit Its Command Line
dry-run-command-line-was = Until now: { $line } ({ $origin })
dry-run-command-line-kernels = A kernel's own .kcl file, and zbm.kcl_override, still come before it.
dry-run-command-line-measured = With measured boot, PCR 8 changes for every later boot.
dry-run-restore = { $count ->
    [one] Restore One Path from { $snapshot }
   *[other] Restore { $count } Paths from { $snapshot }
}
# $access is pool-read-only or pool-read-write.
dry-run-import = Import { $pool } { $access }
dry-run-import-last-host = Import { $pool } { $access } with Its Last Host ID
dry-run-export = Export { $pool }

## Naming a new snapshot or boot environment.

# Before the name being typed.
name-label = Name:
name-snapshot = The snapshot keeps { $environment } as it is now.
    .title = New Snapshot of { $environment }
name-clone = The new boot environment shares the data of { $snapshot } until either changes.
    .title = Clone { $snapshot }
name-boot-clone = The new boot environment is cloned from { $snapshot }, then boots; it stays afterwards.
    .title = Boot a Clone of { $snapshot }
name-duplicate = The copy of { $snapshot } shares nothing with it, and takes its space again.
    .title = Duplicate { $snapshot }
name-taken = { $name } exists already: choose another name
name-empty = type a name first

## Restoring files from a snapshot.

restore = They replace the files in { $environment } as they are now.
    .title = { $count ->
        [one] Restore { $count } File from { $snapshot }
       *[other] Restore { $count } Files from { $snapshot }
    }
    .more = and { $count } more
    .safety = { $pool } is imported read/write, and a safety snapshot { $safety }-<time> is taken first: roll back to it to undo.
restore-nothing = { $path } was created after { $snapshot }: there is nothing to restore

## Keyboard layouts and sealing a TPM2 key again.

layout-serial = keys come from a terminal: its own layout applies
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

## The line view (ROADMAP.md UI8, ADR 0011): the menu as lines, for screen
## readers, `TERM=dumb` and serial lines.

lines-stage-pending = { $stage }: waiting
lines-stage-running = { $stage }: running
lines-stage-done = { $stage }: done
lines-stage-skipped = { $stage }: skipped
lines-stage-failed = { $stage }: failed, { $reason }
lines-go = ok: { $go }
lines-stop = back: { $stop }
# In the workspace's Boot tab: the view, then what its columns select, as
# zroot > zroot/ROOT/void > 6.12.9_1.
lines-path = { $title }: { $path }
lines-number = Type a number to choose it, or a number and a key, as 2 k.
lines-text = Type it, then Enter; a blank line goes back.
# A review page in the line view, which says only the first rows of what
# is lost.
lines-review = Type the name, then Enter; a blank line goes back. The line view says only the first 20 rows of what is lost, and cannot take the safer action: go back, and take it from its menu.
lines-keys = Keys: { $keys }
lines-words = Words: ok, back, help; a blank line shows this again.
# The workspace's: k moves up there, l goes into the kernels, and the
# tabs have words.
lines-number-workspace = Type a number to choose it, or a number and a key, as 2 l.
lines-words-workspace = Words: ok, back, help, boot, snapshots, pools, log; a blank line shows this again.
lines-unknown = Unknown: { $typed }. Type help for the keys.
lines-none = Nothing here.
lines-countdown-stopped = The countdown stopped.
lines-word-ok = ok
lines-word-back = back
lines-word-help = help
lines-word-boot = boot
lines-word-snapshots = snapshots
lines-word-pools = pools
lines-word-log = log
