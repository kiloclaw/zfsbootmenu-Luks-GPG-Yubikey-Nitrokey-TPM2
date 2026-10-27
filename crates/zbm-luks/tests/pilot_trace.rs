//! The trace of the key methods, in a test program of its own: tracing
//! remembers which call sites a subscriber wants, and tests on other
//! threads, which run the same code without a subscriber, made it forget
//! some of this test's events.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test fails loudly"
)]

use std::path::PathBuf;

use pretty_assertions::assert_eq;
use tracing_subscriber::layer::SubscriberExt;
use zbm_crypt::{Closed, Cryptsetup, Mapping, OpenOptions};
use zbm_luks::pilot::{Attempt, KeyChain, KeyFile, KeyMethod};
use zbm_platform::{Context, Secret};
use zbm_testkit::fakes::FakeSystem;
use zbm_trace::JournalLayer;

/// A method that opens the volume.
struct Opens;

impl KeyMethod for Opens {
    fn name(&self) -> &'static str {
        "prompt"
    }

    fn try_open(
        &self,
        context: &Context<'_>,
        mapping: Mapping<Closed>,
        options: OpenOptions,
    ) -> Attempt {
        let key = Secret::copy_from(b"scripted");
        let open = Cryptsetup::new(context.runner()).open(mapping, &key, options);
        Attempt::Opened(open.unwrap())
    }
}

#[test]
fn each_attempt_is_traced_inside_the_unlock_span() {
    let fake = FakeSystem::new();
    let missing = KeyFile::new(PathBuf::from("/etc/zfsbootmenu/keys/root.key"));
    let unlocker = KeyChain::new(vec![Box::new(missing), Box::new(Opens)]);
    let layer = JournalLayer::new(std::io::sink()).with_copy();
    let lines = layer.lines();
    let subscriber = tracing_subscriber::registry().with(layer);
    let volume = Mapping::new("/dev/sda2".parse().unwrap(), "cryptroot".parse().unwrap());

    tracing::subscriber::with_default(subscriber, || {
        let _ = unlocker.unlock(&Context::new(fake.system()), volume, OpenOptions::default());
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
