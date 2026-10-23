# Unlocking

Two features of the unlock dialog that answer the most confusing moments of an encrypted boot: *why is it asking
for my passphrase, when the TPM always unlocked it?* and *why is my passphrase wrong?*. The prototype draws them on
its fake machine; R4 (`zbm-app`, on `zbm-luks` and `zbm-crypt`) and R5 build them. Nothing here is in the images yet.

```sh
cargo run -p zbm-tui -- --demo --tpm-changed   # the firmware was updated: the TPM keeps luks-zroot-a's key
cargo run -p zbm-tui -- --demo --locked        # every volume asks; try a wrong passphrase, Ctrl-K and Ctrl-R
```

The demo's passphrase is `demo`, its TPM2 PIN `1234`.

## Why the TPM kept the key, and sealing it again

This is the menu side of [ROADMAP.md](../../../ROADMAP.md) #11 (re-seal the TPM after firmware updates) and part of
#4 (clear unlock errors).

A TPM2 key is sealed to some PCRs (`0` and `7` in the demo: firmware code and Secure Boot state). After a firmware
update, a change of the Secure Boot keys or a new boot loader, one of them no longer has the value the key was sealed
to, and the TPM keeps the key. Today the user then just sees a passphrase prompt.

- **Before asking.** Start-up compares the PCRs a key is sealed to with their values now. When one changed, it does
  not ask for the TPM2 PIN, which the TPM would refuse anyway: the dialog asks for the next method (the passphrase)
  and says why, in the warning role: *“The TPM kept the key. PCR 0 (firmware code): the firmware was updated to
  1.21.”* (wireframe `unlock-tpm-changed`). Choosing the TPM2 PIN anyway brings the same notice back, and no PIN try
  is spent.
- **What changed.** Which PCR changed is known when the values at sealing were recorded: ZFSBootMenu records them in
  the token's metadata when it seals. The event log (`/sys/kernel/security/tpm0/binary_bios_measurements`) then says
  what the new measurement is (*the firmware was updated to 1.21*). For a key sealed by another tool, such as
  `systemd-cryptenroll`, which records only the policy hash, the dialog names the PCRs the key is sealed to and that
  one of them changed.
- **Sealing again.** Once the volume is open with the passphrase, an alert offers to seal the key to the PCRs as they
  are now (wireframe `reseal-confirm`). It lists what changed and says the risk: re-sealing trusts the machine as it
  is now, so it is for changes the user made; if they did not, the firmware may have been tampered with. It wants the
  volume's name typed, like the other alerts that cannot be undone from the menu, and its button is **Re-seal**.
  `Esc` keeps the old seal and start-up goes on.
- **Later.** The pools view labels the volume `TPM2 changed` (wireframe `pools-tpm-changed`), and the command palette
  offers *“Re-seal the TPM2 Key of luks-zroot-a”* until it is done.
- `Reseal(volume)` seals the key again with the same PCRs and PIN, and records the new values. The next boot unlocks
  with the TPM.

## The keyboard layout

The console turns key positions into characters with the image's keymap, usually `us`. A passphrase set on a German
or French keyboard then cannot be typed: `z` and `y`, `a` and `q`, and most symbols are elsewhere.

- The unlock dialog says which layout typing uses: *“Keyboard: us · Ctrl-K: layout · Ctrl-R: show”*.
- After a wrong passphrase, that line turns into a warning that names the likely cause: *“Keyboard: us. Set with
  another layout? Ctrl-K changes it.”* (wireframe `unlock-wrong`).
- `Ctrl-K` switches to the next layout the image knows (`us`, `de`, `fr`, `uk` in the prototype) and clears what was
  typed, which was typed with the other one. The menu then translates each key from the console's layout to the
  chosen one by its position, so the key labelled `Z` on a German keyboard types `z` (wireframe `unlock-layout`).
- The chosen layout applies to every key, in every view, until the menu ends: the key labelled `/` on a German
  keyboard (`Shift-7`) still opens the search. Keys with `Ctrl` or `Alt` are not translated.
- `Ctrl-R` shows what is typed, and hides it again. It is off at every prompt and never on by default: the user turns
  it on, at a console they can see is not watched.
- Over a serial console the terminal sends characters in its own layout, so nothing is translated, and the line says
  *“Keyboard: the terminal's own”*.

The prototype translates the main block of keys (letters, digits, the symbols beside them). R5 decides between
translating in the menu, as the prototype does, which needs no keymap files in the image, and loading the keymap with
`loadkeys`, which also changes the recovery shell. `generate-zbm` can add the keymap of the system that builds the
image (`/etc/vconsole.conf`) to the image's layouts, so the right one is usually there.

## What `zbm-app` gains

In [interfaces.md](interfaces.md): `tpm2` on each volume (the PCRs, and those that changed with what the event log
says), `keyboard` in the snapshot, the `Reseal` action, `UnlockRequested` with a notice (a wrong secret, or why this
method instead of another), and the `ResealOffered` event.
