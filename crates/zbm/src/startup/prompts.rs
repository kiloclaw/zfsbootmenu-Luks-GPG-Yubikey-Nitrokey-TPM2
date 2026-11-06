//! What unlocking asks, answered with the start-up view's dialogs
//! (`zbm_app::service::asker`'s, which the menu shares): a secret in the
//! unlock dialog, a device waited for in it with the time left, and the
//! timed prompts as questions. What booting asks (a ZFS key, the resume
//! question, a failed boot) has no dialog yet (ADR 0015): the view steps
//! aside, and the plain console asks it.

use zbm_app::model::{Action, Event, Method, Volume, VolumeState};
use zbm_app::prompt::ConsoleInteraction;
use zbm_app::service::asker::{Asks, Dialogs};
use zbm_platform::Clock;
use zbm_platform::interaction::{
    Asked, Interaction, PromptRequest, Resume, ResumeRequest, SecretRequest, SecretSubject, Tell,
    VolumeRef, Wait, WaitRequest,
};
use zbm_types::{Answer, Untrusted};

use super::MenuFront;

impl MenuFront<'_> {
    /// What `ask` gets from the plain console, while the view steps aside;
    /// `refused` if it did not run.
    fn plainly<T>(&self, refused: T, ask: impl FnOnce(&ConsoleInteraction<'_>) -> T) -> T {
        let mut answer = None;
        self.hand_over(|| answer = Some(ask(&ConsoleInteraction::new(self.system))));
        answer.unwrap_or(refused)
    }

    /// Lists `volume`, asked for with `method`, among the view's volumes,
    /// so that its dialog names its device.
    fn list_volume(&self, volume: VolumeRef<'_>, method: Method) {
        let snapshot = {
            let mut snapshot = super::lock(&self.known).clone();
            let known = Volume {
                name: Untrusted::new(volume.name.to_owned()),
                device: Untrusted::new(volume.device.to_owned()),
                state: VolumeState::Locked,
                methods: vec![method],
                pool: None,
                tpm2: None,
            };
            match snapshot
                .volumes
                .iter_mut()
                .find(|v| v.name.get() == volume.name)
            {
                Some(listed) => *listed = known,
                None => snapshot.volumes.push(known),
            }
            snapshot
        };
        self.show(Event::Changed(snapshot));
    }
}

impl Asks for MenuFront<'_> {
    fn show(&self, event: Event) {
        MenuFront::show(self, event);
    }

    fn answer(
        &self,
        event: Event,
        check: Option<&mut dyn FnMut() -> Option<Action>>,
    ) -> Option<Action> {
        MenuFront::answer(self, event, check)
    }

    fn list(&self, volume: VolumeRef<'_>, method: Method) -> String {
        self.list_volume(volume, method);
        volume.name.to_owned()
    }

    fn clock(&self) -> &dyn Clock {
        self.system.clock
    }
}

impl Interaction for MenuFront<'_> {
    fn ask(&self, request: &PromptRequest) -> Answer {
        Dialogs::ask(self, request)
            .unwrap_or_else(|| self.plainly(Answer::Escape, |console| console.ask(request)))
    }

    /// The unlock dialog: what was typed with Enter; empty with Esc, or when
    /// another method was chosen; nothing when another program opened the
    /// volume meanwhile, or the view is gone.
    fn secret(&self, request: &SecretRequest<'_>) -> Asked {
        match request.subject {
            SecretSubject::Volume(volume) => self.dialogs.secret(self, volume, request),
            SecretSubject::EncryptionRoot(_) => {
                self.plainly(Asked::Ended, |console| console.secret(request))
            }
        }
    }

    /// The unlock dialog, waiting for the device with the time left: Esc
    /// stops waiting, Enter looks again at once.
    fn wait(&self, request: &WaitRequest<'_>) -> Wait {
        self.dialogs.wait(self, request)
    }

    /// The view steps aside while `run`'s program has the console, with the
    /// runner of the whole system rather than the quiet one of the stages.
    fn lease_console(&self, run: &mut dyn FnMut(Option<&dyn zbm_platform::Runner>)) {
        let runner = self.system.runner;
        self.hand_over(|| run(Some(runner)));
    }

    /// The resume question, on the plain console.
    fn resume(&self, request: &ResumeRequest<'_>) -> Resume {
        self.plainly(Resume::Refuse, |console| console.resume(request))
    }

    /// What goes wrong is said by the next dialog; what takes a while
    /// shows until the next event.
    fn notify(&self, tell: Tell<'_>) {
        self.dialogs.notify(self, tell);
    }
}
