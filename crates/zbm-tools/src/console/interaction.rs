//! [`ConsoleInteraction`]: what domain code asks of the person, shown on a
//! plain console as the legacy runtime showed it.

use std::time::Duration;

use zbm_platform::System;
use zbm_platform::console::{Screen, Timed};
use zbm_platform::interaction::{
    Asked, Awaited, Interaction, PromptRequest, Resume, ResumeRequest, SecretKind, SecretRequest,
    Subject, Tell, Told, Wait, WaitRequest,
};

use super::{Answer, TimedPrompt, colorize, retry_delay};

/// The plain console, the [`Interaction`] of a terminal without the menu:
/// timed prompts as [`TimedPrompt`]s, secrets read with echo off, devices
/// waited for with a countdown any key stops, messages in colour, and the
/// resume question as the legacy runtime asked it.
pub struct ConsoleInteraction<'a> {
    system: System<'a>,
    colors: bool,
}

impl<'a> ConsoleInteraction<'a> {
    /// The console of `system`, in colour.
    pub fn new(system: System<'a>) -> ConsoleInteraction<'a> {
        ConsoleInteraction {
            system,
            colors: true,
        }
    }

    /// The console, in colour when `colors` holds, plain otherwise.
    #[must_use]
    pub fn colors(self, colors: bool) -> ConsoleInteraction<'a> {
        ConsoleInteraction { colors, ..self }
    }

    /// `text` in `color`, or plain without colours.
    fn paint(&self, color: &str, text: &str) -> String {
        if self.colors {
            colorize(color, text)
        } else {
            text.to_owned()
        }
    }

    /// Writes `text` to the console.
    fn say(&self, text: &str) {
        let _ = self.system.console.write(text.as_bytes());
    }

    /// A blank line, then `heading` in cyan with `device` in it highlighted.
    fn heading(&self, heading: &str, device: &str) {
        let highlighted = if device.is_empty() {
            heading.to_owned()
        } else {
            heading.replacen(device, &self.paint("yellow", device), 1)
        };
        self.say("\n");
        self.say(&self.paint("cyan", &format!("{highlighted}\n")));
    }

    /// Asks for a secret after `prompt`, with echo off; nothing when the
    /// input ends. Echo goes off before the prompt appears, so no keystroke
    /// can echo.
    fn read_secret(&self, prompt: &str) -> Asked {
        let console = self.system.console;
        console.set_echo(false);
        self.system.console.screen(Screen::ShowCursor);
        self.say(prompt);
        let secret = console.read_secret().ok().flatten();
        console.set_echo(true);
        self.say("\n");
        secret.into()
    }

    /// The passphrase prompt of a volume: its description, a note when the
    /// last passphrase was wrong, then the passphrase, read while noticing
    /// an unlock performed elsewhere (`zluks` over SSH).
    fn passphrase(&self, request: &SecretRequest<'_>) -> Asked {
        let console = self.system.console;
        let description = request.description.unwrap_or(request.volume.device);
        console.set_echo(false);
        self.system.console.screen(Screen::ShowCursor);
        self.say("\n");
        let line = format!("Unlock LUKS volume {}\n", self.paint("yellow", description));
        self.say(&self.paint("cyan", &line));
        if request.attempt > 1 {
            self.say(&self.paint("red", "No key available with this passphrase.\n"));
        }
        self.say("Passphrase (leave empty to skip): ");
        let elsewhere = || request.opened_elsewhere.is_some_and(|opened| opened());
        let read = loop {
            match console.read_secret_within(Duration::from_secs(2)) {
                Ok(Timed::Read(passphrase)) => break Asked::Secret(passphrase),
                Ok(Timed::TimedOut) if elsewhere() => break Asked::Elsewhere,
                Ok(Timed::TimedOut) => {}
                // Nothing more can be done interactively.
                Ok(Timed::Ended) | Err(_) => break Asked::Ended,
            }
        };
        console.set_echo(true);
        self.say("\n");
        read
    }

    /// The message of a wait for `awaited`, for the volume on `device`.
    fn awaited(&self, awaited: &Awaited, device: &str) -> String {
        let text = match awaited {
            Awaited::SecurityKey => format!(
                "Insert the FIDO2 security key (YubiKey, Nitrokey, ...) for {}",
                self.paint("yellow", device)
            ),
            Awaited::OpenPgpKey => format!(
                "Insert the OpenPGP security key (YubiKey, Nitrokey, ...) for {}",
                self.paint("yellow", device)
            ),
            Awaited::YubiKey => format!("Insert the YubiKey for {}", self.paint("yellow", device)),
            Awaited::KeyDevice { spec, file } => format!(
                "Insert the key device {} holding {file}",
                self.paint("yellow", spec)
            ),
        };
        self.paint("cyan", &text)
    }
}

/// The warning of the resume question about `pool`, as the legacy runtime
/// shows it, painted with `paint`.
fn resume_warning(pool: &str, paint: impl Fn(&str, &str) -> String) -> String {
    format!(
        "WARNING!!!

This system appears to have an active suspend partition.

The action you are requesting requires the ZFS pool

    {pool}

be imported read-write. Importing read-write and then resuming
from an active suspend partition may DESTROY YOUR POOL.

If you choose to proceed, ZFSBootMenu can attempt to remove any
\"resume=\" arguments from your kernel command line and append a
\"noresume\" argument to prevent your system from attempting to
restore from the active suspend partition.

Type {noresume} to proceed with the import, allowing ZFSBootMenu
to add a \"noresume\" argument to your kernel command line.

Type {dangerous} to proceed with the import without allowing
ZFSBootMenu to modify your kernel command line. Make sure to
add the \"noresume\" argument yourself if necessary.

Type any other text, or just press enter, to abort.

Proceed {no} ?
",
        pool = paint("red", pool),
        noresume = paint("green", "NORESUME"),
        dangerous = paint("red", "DANGEROUS"),
        no = paint("red", "[No]"),
    )
}

/// The heading and the prompt of a secret of `kind`, for the volume on
/// `device`; `None` for the volume's passphrase, which has a prompt of its
/// own ([`ConsoleInteraction::passphrase`]).
fn secret_prompt(kind: &SecretKind, device: &str) -> Option<(Option<String>, String)> {
    const OTHER: &str = "leave empty to use another method";
    Some(match kind {
        SecretKind::Passphrase => return None,
        SecretKind::Tpm2Pin => (
            Some(format!("Unlock {device} with the TPM")),
            format!("TPM2 PIN ({OTHER}): "),
        ),
        SecretKind::Fido2Pin => (None, format!("Security key PIN ({OTHER}): ")),
        SecretKind::YubiKeyPassphrase(slot) => (
            Some(format!(
                "Unlock {device} with YubiKey challenge-response (slot {slot})"
            )),
            format!("Passphrase ({OTHER}): "),
        ),
        SecretKind::KeyFilePassphrase(file) => (
            Some(format!("Unlock {device} with GnuPG key file {file}")),
            format!("Key file passphrase ({OTHER}): "),
        ),
        SecretKind::SecretKeyPassphrase(file) => (
            Some(format!("Unlock {device} with GnuPG key file {file}")),
            format!("Secret key passphrase ({OTHER}): "),
        ),
        SecretKind::CardPin { serial, left } => (
            Some(format!("Unlock {device} with OpenPGP card {serial}")),
            format!("Card PIN ({left} attempts left; {OTHER}): "),
        ),
    })
}

impl Interaction for ConsoleInteraction<'_> {
    /// The timed prompt; a skipped device or a retry leaves the screen clear
    /// with the cursor on, as the legacy unlock loop leaves it.
    fn ask(&self, request: &PromptRequest) -> Answer {
        let mut prompt = TimedPrompt::new();
        if let Some(delay) = request.delay {
            prompt = prompt.delay(retry_delay(delay));
        }
        let answer = prompt
            .message(colorize("red", &request.message))
            .on_return(&request.on_return)
            .on_escape(&request.on_escape)
            .prompt(|seconds| {
                format!(
                    "Retrying in {} seconds",
                    colorize("yellow", &format!("{seconds:02}"))
                )
            })
            .run(&self.system);
        let clear = match request.subject {
            Subject::Retry { .. } => true,
            Subject::Missing(_) => answer == Answer::Escape,
        };
        if clear {
            self.system.console.screen(Screen::ShowCursor);
            self.system.console.screen(Screen::Clear);
        }
        answer
    }

    fn secret(&self, request: &SecretRequest<'_>) -> Asked {
        let device = request.volume.device;
        let Some((heading, prompt)) = secret_prompt(&request.kind, device) else {
            return self.passphrase(request);
        };
        if let Some(heading) = heading {
            self.heading(&heading, device);
        }
        self.read_secret(&prompt)
    }

    /// Waits up to the request's seconds for the device, showing what is
    /// awaited, unless the user presses a key.
    fn wait(&self, request: &WaitRequest<'_>) -> Wait {
        let present = request.present;
        if present() {
            return Wait::Present;
        }
        let seconds = request.seconds;
        if seconds == 0 {
            return Wait::TimedOut;
        }
        let system = &self.system;
        let message = self.awaited(&request.awaited, request.volume.device);
        self.say(&format!("{message}\n"));
        self.say(&format!(
            "Waiting {seconds} seconds; press any key to skip.\n"
        ));
        let second = Duration::from_secs(1);
        for _ in 0..seconds {
            let before = system.clock.now();
            match system.console.read_key(Some(second)) {
                Ok(Some(_)) => return Wait::Skipped,
                // The end of the input returns at once, so wait here.
                _ if system.clock.now().saturating_sub(before) < second => {
                    system.clock.sleep(second);
                }
                _ => {}
            }
            if present() {
                return Wait::Present;
            }
        }
        Wait::TimedOut
    }

    fn notify(&self, tell: Tell<'_>) {
        let line = format!("{}\n", tell.text);
        match tell.told {
            Told::Working => self.say(&line),
            Told::Touch => self.say(&self.paint("yellow", &line)),
            Told::Refused => self.say(&self.paint("red", &line)),
            Told::Heading => self.heading(tell.text, tell.highlight.unwrap_or_default()),
        }
    }

    /// The warning on a clear screen, then the line typed after `> `:
    /// `NORESUME` or `DANGEROUS`, anything else refuses.
    fn resume(&self, request: &ResumeRequest<'_>) -> Resume {
        let console = self.system.console;
        console.screen(Screen::Clear);
        console.screen(Screen::ShowCursor);
        console.screen(Screen::MoveTo { row: 0, column: 0 });
        self.say(&resume_warning(request.pool, |color, text| {
            self.paint(color, text)
        }));
        // zfsbootmenu-input: `read -r -e -p "> "`, then `echo`
        self.say("> ");
        let line = console.read_line().ok().flatten().unwrap_or_default();
        match line.trim_matches([' ', '\t', '\n']) {
            "NORESUME" => Resume::Noresume,
            "DANGEROUS" => Resume::Dangerous,
            _ => Resume::Refuse,
        }
    }
}

#[cfg(test)]
mod tests;
