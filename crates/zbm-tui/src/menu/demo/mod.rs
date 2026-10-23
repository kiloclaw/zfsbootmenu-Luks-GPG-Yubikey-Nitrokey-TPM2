//! A fake machine for `zbm-tui --demo` and for the tests of the menu: it
//! runs start-up stage by stage and answers every [`Action`] by changing its
//! [`Snapshot`] and sending [`Event`]s, as `zbm-app` will.
//!
//! The demo's secrets: the passphrase `demo`, the TPM2 PIN `1234`; the
//! security key is never plugged in.

pub mod data;
mod session;
mod storage;

pub use session::Session;

use zbm_platform::secret::Secret;

use crate::menu::world::{Action, Event, Method, Outcome, Snapshot, StageState, VolumeState};

/// What the demo machine has.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemoOptions {
    /// The LUKS volumes start locked, and start-up asks for them.
    pub locked: bool,
    /// A pool of another host does not import at start-up.
    pub foreign: bool,
    /// The countdown in seconds; `None` waits.
    pub timeout: Option<u32>,
    /// The snapshots of the default environment.
    pub snapshots: usize,
}

impl Default for DemoOptions {
    fn default() -> DemoOptions {
        DemoOptions {
            locked: false,
            foreign: false,
            timeout: Some(10),
            snapshots: 400,
        }
    }
}

/// The passphrase of every demo volume.
pub const PASSPHRASE: &[u8] = b"demo";

/// The TPM2 PIN of the demo volume that has one.
pub const PIN: &[u8] = b"1234";

/// Wrong TPM2 PINs before the TPM locks out.
const PIN_TRIES: u32 = 3;

/// The fake machine.
#[derive(Clone, Debug)]
pub struct FakeMachine {
    snapshot: Snapshot,
    /// A dialog is open and start-up waits for its answer.
    waiting: bool,
    /// Wrong TPM2 PINs so far.
    pin_failures: u32,
    /// The import problem of the foreign pool was shown.
    import_asked: bool,
}

impl FakeMachine {
    /// The machine `options` describe, at power-on.
    pub fn new(options: &DemoOptions) -> FakeMachine {
        FakeMachine {
            snapshot: data::machine(options),
            waiting: false,
            pin_failures: 0,
            import_asked: false,
        }
    }

    /// The machine as it is now.
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// Start-up goes one step further, unless it waits for an answer.
    pub fn tick(&mut self) -> Vec<Event> {
        if self.waiting {
            return Vec::new();
        }
        let stages = &mut self.snapshot.startup;
        let Some(current) = stages
            .iter()
            .position(|stage| matches!(stage.state, StageState::Pending | StageState::Running))
        else {
            return Vec::new();
        };
        if let Some(stage) = stages.get_mut(current)
            && stage.state == StageState::Pending
        {
            stage.state = StageState::Running;
            return vec![self.changed()];
        }
        if let Some(event) = self.blocking(current) {
            self.waiting = true;
            return vec![event];
        }
        let skipped = current == data::UNLOCK_STAGE && self.were_all_skipped();
        if let Some(stage) = self.snapshot.startup.get_mut(current) {
            stage.state = if skipped {
                StageState::Skipped
            } else {
                StageState::Done
            };
        }
        vec![self.changed()]
    }

    /// What stops a stage: a locked volume, a pool that did not import.
    fn blocking(&mut self, stage: usize) -> Option<Event> {
        if stage == data::UNLOCK_STAGE {
            let volume = self
                .snapshot
                .volumes
                .iter()
                .find(|volume| volume.state == VolumeState::Locked)?;
            return Some(Event::UnlockRequested {
                volume: volume.name.clone(),
                method: volume
                    .methods
                    .first()
                    .copied()
                    .unwrap_or(Method::Passphrase),
                tries_left: None,
            });
        }
        if stage == data::IMPORT_STAGE && !self.import_asked {
            let pool = self.snapshot.pools.iter().find(|pool| !pool.imported)?;
            self.import_asked = true;
            return Some(Event::ImportProblem {
                pool: pool.name.clone(),
                reason: format!(
                    "{} was last imported by host {}, and this machine is host {}.",
                    pool.name,
                    data::FOREIGN_HOSTID,
                    data::HOSTID
                ),
            });
        }
        None
    }

    /// Asks again for what start-up waits for, after the user closed its
    /// dialog.
    pub fn reask(&mut self) {
        if self.waiting {
            self.waiting = false;
            self.import_asked = false;
        }
    }

    fn were_all_skipped(&self) -> bool {
        self.snapshot
            .volumes
            .iter()
            .all(|volume| volume.state == VolumeState::Skipped)
    }

    fn changed(&self) -> Event {
        Event::Changed(self.snapshot.clone())
    }

    /// Does what `action` asks, and says what happened.
    pub fn apply(&mut self, action: Action) -> Vec<Event> {
        match action {
            Action::Unlock {
                volume,
                method,
                secret,
            } => self.unlock(&volume, method, secret.as_ref().map(Secret::expose)),
            Action::SkipUnlock(volume) => {
                self.set_volume(&volume, VolumeState::Skipped);
                self.waiting = false;
                vec![self.changed()]
            }
            Action::Import(pool, policy) => self.import(&pool, policy),
            Action::Shell => leave("Opening the recovery shell"),
            Action::Reboot => leave("Rebooting"),
            Action::PowerOff => leave("Powering off"),
            Action::FirmwareSetup => leave("Rebooting into the firmware setup"),
            other => storage::apply(&mut self.snapshot, other),
        }
    }

    /// An unlock attempt.
    fn unlock(&mut self, volume: &str, method: Method, secret: Option<&[u8]>) -> Vec<Event> {
        let right = match method {
            Method::Passphrase => secret == Some(PASSPHRASE),
            Method::Tpm2Pin => secret == Some(PIN) && self.pin_failures < PIN_TRIES,
            _ => false,
        };
        if right {
            self.set_volume(volume, VolumeState::Open);
            self.waiting = false;
            if self
                .snapshot
                .volumes
                .iter()
                .all(|v| v.state != VolumeState::Locked)
            {
                for environment in &mut self.snapshot.environments {
                    environment.encryption = match environment.encryption {
                        zbm_app::Encryption::Locked => zbm_app::Encryption::Unlocked,
                        other => other,
                    };
                }
            }
            let done = Outcome::Done(format!("{volume} opened with its {}", method.as_str()));
            return vec![self.changed(), Event::Finished(done)];
        }
        self.wrong(volume, method)
    }

    /// A failed unlock attempt: the dialog asks again, with the reason.
    fn wrong(&mut self, volume: &str, method: Method) -> Vec<Event> {
        let (warning, method, tries_left) = match method {
            Method::Tpm2Pin => {
                self.pin_failures += 1;
                let left = PIN_TRIES.saturating_sub(self.pin_failures);
                if left == 0 {
                    let text = "the TPM is locked against PIN guessing: use the passphrase";
                    (text.to_owned(), Method::Passphrase, None)
                } else {
                    (
                        format!("wrong PIN, {left} tries left"),
                        Method::Tpm2Pin,
                        Some(left),
                    )
                }
            }
            Method::Fido2 | Method::YubiKey | Method::KeyDevice => (
                format!(
                    "no {} found: plug it in and press Enter, or choose another method",
                    method.as_str()
                ),
                method,
                None,
            ),
            _ => (format!("wrong {}", method.as_str()), method, None),
        };
        vec![
            Event::Finished(Outcome::Warning(warning)),
            Event::UnlockRequested {
                volume: volume.to_owned(),
                method,
                tries_left,
            },
        ]
    }

    fn set_volume(&mut self, name: &str, state: VolumeState) {
        if let Some(volume) = self.snapshot.volumes.iter_mut().find(|v| v.name == name) {
            volume.state = state;
        }
    }

    /// An import of a pool that did not import at start-up.
    fn import(
        &mut self,
        name: &zbm_types::PoolName,
        policy: zbm_types::ImportPolicy,
    ) -> Vec<Event> {
        let Some(pool) = self
            .snapshot
            .pools
            .iter_mut()
            .find(|pool| &pool.name == name)
        else {
            return Vec::new();
        };
        if pool.foreign_hostid.is_some() && policy == zbm_types::ImportPolicy::Strict {
            return vec![Event::Finished(Outcome::Failed {
                what: format!("{name} did not import"),
                why: vec![format!(
                    "it was last imported by host {}, and this machine is host {}",
                    data::FOREIGN_HOSTID,
                    data::HOSTID
                )],
                next: vec![
                    "import it with the host ID it was last imported with".to_owned(),
                    "force the import, if that host is not running".to_owned(),
                    "open the recovery shell".to_owned(),
                ],
            })];
        }
        pool.imported = true;
        self.waiting = false;
        storage::add_backup_environment(&mut self.snapshot, name);
        let done = Outcome::Done(format!("{name} imported ({policy})"));
        vec![self.changed(), Event::Finished(done)]
    }
}

/// The menu ends.
fn leave(why: &str) -> Vec<Event> {
    vec![Event::Leaving(why.to_owned())]
}

#[cfg(test)]
mod measures;
#[cfg(test)]
mod tests;
