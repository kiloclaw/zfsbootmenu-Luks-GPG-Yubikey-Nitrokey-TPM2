//! Tests of the unlocker and the key file method.
//!
//! Behaviours covered: the key file's contents reach cryptsetup on standard
//! input only; a missing key file makes the method unavailable without
//! running cryptsetup; methods are tried in order until one opens the volume;
//! a refused key leads to the next method; when every method fails, each
//! attempt is reported in order; each attempt is traced.

use std::cell::Cell;
use std::path::PathBuf;

use pretty_assertions::assert_eq;
use tracing_subscriber::layer::SubscriberExt;
use zbm_crypt::{Closed, CryptError, Mapping, OpenOptions};
use zbm_platform::{Context, Input, Secret};
use zbm_testkit::fakes::FakeSystem;
use zbm_trace::JournalLayer;

use super::*;

const KEY_FILE: &str = "/etc/zfsbootmenu/keys/root.key";

fn volume() -> Mapping<Closed> {
    Mapping::new("/dev/sda2".parse().unwrap(), "cryptroot".parse().unwrap())
}

fn key_file() -> Box<dyn KeyMethod> {
    Box::new(KeyFile::new(PathBuf::from(KEY_FILE)))
}

/// A method that does what it is told and counts how often it was asked.
struct Scripted {
    name: &'static str,
    opens: bool,
    calls: Cell<usize>,
}

impl Scripted {
    fn boxed(name: &'static str, opens: bool) -> Box<dyn KeyMethod> {
        Box::new(Scripted {
            name,
            opens,
            calls: Cell::new(0),
        })
    }
}

impl KeyMethod for Scripted {
    fn name(&self) -> &'static str {
        self.name
    }

    fn try_open(
        &self,
        context: &Context<'_>,
        mapping: Mapping<Closed>,
        options: OpenOptions,
    ) -> Attempt {
        self.calls.set(self.calls.get() + 1);
        if self.opens {
            let key = Secret::copy_from(b"scripted");
            let open = zbm_crypt::Cryptsetup::new(context.runner()).open(mapping, &key, options);
            return Attempt::Opened(open.unwrap());
        }
        Attempt::Unavailable {
            mapping,
            reason: Unavailable::KeyFileMissing {
                path: PathBuf::from("/nowhere"),
            },
        }
    }
}

#[test]
fn a_key_file_reaches_cryptsetup_on_standard_input_only() {
    let fake = FakeSystem::new();
    fake.files.file(KEY_FILE, "correct horse battery staple");
    let unlocker = KeyChain::new(vec![key_file()]);

    let opened = unlocker
        .unlock(
            &Context::new(fake.system()),
            volume(),
            OpenOptions::default(),
        )
        .unwrap();

    assert_eq!(opened.method, "key file");
    let calls = fake.runner.calls();
    assert_eq!(
        calls[0].display(),
        "cryptsetup open --type luks --key-file=- /dev/sda2 cryptroot"
    );
    assert_eq!(
        calls[0].get_stdin(),
        &Input::Secret(Secret::copy_from(b"correct horse battery staple"))
    );
    fake.assert_no_leak(b"correct horse battery staple");
}

#[test]
fn a_missing_key_file_is_unavailable_and_runs_nothing() {
    let fake = FakeSystem::new();
    let unlocker = KeyChain::new(vec![key_file()]);

    let failed = unlocker
        .unlock(
            &Context::new(fake.system()),
            volume(),
            OpenOptions::default(),
        )
        .unwrap_err();

    assert_eq!(fake.runner.lines(), Vec::<String>::new());
    assert_eq!(failed.mapping, volume());
    assert_eq!(failed.attempts.len(), 1);
    assert_eq!(
        failed.attempts[0].1.to_string(),
        "the key file /etc/zfsbootmenu/keys/root.key does not exist"
    );
}

#[test]
fn methods_are_tried_in_order_until_one_opens_the_volume() {
    let fake = FakeSystem::new();
    let unlocker = KeyChain::new(vec![
        Scripted::boxed("first", false),
        Scripted::boxed("second", true),
        Scripted::boxed("third", true),
    ]);

    let opened = unlocker
        .unlock(
            &Context::new(fake.system()),
            volume(),
            OpenOptions::default(),
        )
        .unwrap();

    assert_eq!(opened.method, "second");
    assert_eq!(fake.runner.lines().len(), 1);
}

#[test]
fn a_refused_key_file_leads_to_the_next_method() {
    let fake = FakeSystem::new();
    fake.files.file(KEY_FILE, "wrong");
    fake.runner
        .respond_program("cryptsetup", 2, "No key available with this passphrase.\n");
    let unlocker = KeyChain::new(vec![key_file(), Scripted::boxed("prompt", false)]);

    let failed = unlocker
        .unlock(
            &Context::new(fake.system()),
            volume(),
            OpenOptions::default(),
        )
        .unwrap_err();

    let names: Vec<&str> = failed.attempts.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["key file", "prompt"]);
    assert!(matches!(
        failed.attempts[0].1,
        Refusal::Refused(CryptError::WrongKey { .. })
    ));
    assert_eq!(failed.to_string(), "no key method opened /dev/sda2");
}

#[test]
fn each_attempt_is_traced_inside_the_unlock_span() {
    let fake = FakeSystem::new();
    let unlocker = KeyChain::new(vec![key_file(), Scripted::boxed("prompt", true)]);
    let layer = JournalLayer::new(std::io::sink());
    let lines = layer.lines();
    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let _ = unlocker.unlock(
            &Context::new(fake.system()),
            volume(),
            OpenOptions::default(),
        );
    });

    let events: Vec<(String, String)> = lines
        .take()
        .iter()
        .filter(|line| line["spans"][1]["span"] == "key_method")
        .map(|line| {
            let method = line["spans"][1]["method"].as_str().unwrap().to_owned();
            let message = line["message"].as_str().unwrap().to_owned();
            (method, message)
        })
        .collect();
    assert_eq!(
        events,
        [
            ("key file".to_owned(), "not available".to_owned()),
            ("prompt".to_owned(), "command ended".to_owned()),
            ("prompt".to_owned(), "opened".to_owned()),
        ]
    );
}
