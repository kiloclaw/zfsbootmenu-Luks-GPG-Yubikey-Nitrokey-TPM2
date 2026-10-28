//! The questions of start-up and the waits for a device: what the keys and
//! the time running out answer, and what the alerts say.

use pretty_assertions::assert_eq;
use zbm_display::keys::Key;
use zbm_types::Answer;

use super::{Message, update};
use crate::menu::caps::Capabilities;
use crate::menu::keymap::{Context, Modifiers, press};
use crate::menu::model::{Dialog, Model};
use crate::menu::theme::Theme;
use crate::menu::view::tests::screen;
use crate::menu::world::{Action, Event, Method, Question, Snapshot};

fn model() -> Model {
    Model::new(Snapshot::default(), Theme::new(Capabilities::FULL))
}

/// `model` after `message`, and the actions it asked for.
fn after(model: Model, message: Message) -> (Model, Vec<Action>) {
    update(model, message)
}

fn event(model: Model, event: Event) -> Model {
    after(model, Message::Event(Box::new(event))).0
}

fn key(model: Model, code: Key) -> (Model, Vec<Action>) {
    after(model, Message::Key(press(code, Modifiers::NONE)))
}

fn asked(question: Question, seconds: Option<u64>) -> Model {
    event(model(), Event::Asked { question, seconds })
}

#[test]
fn enter_goes_on_and_escape_stops() {
    let question = Question::ImportFailed {
        pool: Some("tank".into()),
    };
    let model = asked(question.clone(), Some(5));
    assert_eq!(model.context(), Context::Choose);

    let (model, actions) = key(model, Key::Char('x'));
    assert!(actions.is_empty() && model.dialog.is_some());
    let (model, actions) = key(model, Key::Enter);
    assert_eq!(actions, [Action::Answer(Answer::Continue)]);
    assert!(model.dialog.is_none());

    let model = asked(question, None);
    let (_, actions) = key(model, Key::Escape);
    assert_eq!(actions, [Action::Answer(Answer::Escape)]);
}

#[test]
fn a_question_goes_on_when_its_time_runs_out() {
    let model = asked(Question::MissingDevices, Some(2));
    let (model, actions) = after(model, Message::Tick(1500));
    assert!(actions.is_empty());
    let shown = screen(&model, 80, 25);
    assert!(shown.contains("Devices Are Missing"), "{shown}");
    assert!(shown.contains("Looking again in 1 second."), "{shown}");
    assert!(shown.contains("Stop Waiting") && shown.contains("Look Again"));

    let (model, actions) = after(model, Message::Tick(500));
    assert_eq!(actions, [Action::Answer(Answer::Continue)]);
    assert!(model.dialog.is_none());

    // Without a limit it waits.
    let model = asked(Question::MissingDevices, None);
    let (model, actions) = after(model, Message::Tick(60_000));
    assert!(actions.is_empty() && model.dialog.is_some());
}

#[test]
fn every_question_has_its_words() {
    let questions = [
        (
            Question::RetryUnlock {
                volume: "cryptroot".into(),
            },
            ["cryptroot Did Not Open", "Try Again", "Skip"],
        ),
        (
            Question::MissingVolume {
                device: "UUID=1234".into(),
            },
            ["UUID=1234 Is Missing", "Look Again", "Skip"],
        ),
        (
            Question::ImportFailed { pool: None },
            ["No Pool Imported", "Try Again", "Open the Recovery Shell"],
        ),
        (
            Question::Unsupported {
                advice: "Check for a new binary release of ZFSBootMenu".into(),
            },
            ["Unsupported Features", "new binary release", "Continue"],
        ),
    ];
    for (question, words) in questions {
        let shown = screen(&asked(question, Some(30)), 80, 25);
        for word in words {
            assert!(shown.contains(word), "{word} in {shown}");
        }
    }
}

#[test]
fn a_wait_for_a_device_shows_the_time_left_and_gives_up() {
    let model = event(
        model(),
        Event::UnlockRequested {
            volume: "cryptroot".into(),
            method: Method::Fido2,
            tries_left: None,
            notice: None,
            wait: Some(10),
        },
    );
    let shown = screen(&model, 80, 25);
    assert!(shown.contains("Waiting for the security key"), "{shown}");
    assert!(shown.contains("10 seconds left"), "{shown}");

    let (model, actions) = after(model, Message::Tick(9_000));
    assert!(actions.is_empty());
    assert!(screen(&model, 80, 25).contains("1 second left"));
    let (model, actions) = after(model, Message::Tick(1_000));
    assert_eq!(actions, [Action::SkipUnlock("cryptroot".into())]);
    assert!(!matches!(model.dialog, Some(Dialog::Unlock(_))));
}

#[test]
fn typed_methods_of_security_keys_have_a_field() {
    for (method, label) in [
        (Method::Fido2Pin, "Security key PIN:"),
        (Method::YubiKeyPassphrase, "Passphrase:"),
        (Method::OpenPgpCardPin, "Card PIN:"),
    ] {
        let model = event(
            model(),
            Event::UnlockRequested {
                volume: "cryptroot".into(),
                method,
                tries_left: Some(2),
                notice: None,
                wait: None,
            },
        );
        let shown = screen(&model, 80, 25);
        assert!(shown.contains(label), "{label} in {shown}");
        assert!(shown.contains("2 tries left"), "{shown}");
    }
    assert!(!Method::OpenPgpCard.is_typed());
}

#[test]
fn progress_shows_until_the_next_event() {
    let model = event(model(), Event::Progress("Unlocking /dev/sda2 ...".into()));
    let shown = screen(&model, 80, 25);
    assert!(shown.contains("Unlocking /dev/sda2 ..."), "{shown}");
    let model = event(model, Event::StageStarted("Importing pools".into()));
    assert_eq!(model.progress, None);
}
