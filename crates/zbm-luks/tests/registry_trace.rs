//! The trace of the key methods, in a test program of its own: tracing
//! remembers which call sites a subscriber wants, and tests on other
//! threads, which run the same code without a subscriber, made it forget
//! some of this test's events.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test fails loudly"
)]

use pretty_assertions::assert_eq;
use tracing_subscriber::layer::SubscriberExt;
use zbm_luks::mappings::Origin;
use zbm_luks::unlock::methods::Registry;
use zbm_luks::unlocker::{Luks, Settings, Unlocker};
use zbm_platform::interaction::NoInteraction;
use zbm_testkit::fakes::FakeSystem;
use zbm_trace::JournalLayer;

#[test]
fn each_attempt_is_traced_inside_the_unlock_span() {
    let fake = FakeSystem::new();
    // The second volume's key file is there.
    fake.files.file("/keys/b.key", "secret");
    let settings = Settings::new("/zfsbootmenu");
    let interaction = NoInteraction;
    let layer = JournalLayer::new(std::io::sink()).with_copy();
    let lines = layer.lines();
    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let luks = Luks {
            context: fake.context(),
            settings: &settings,
            interaction: &interaction,
        };
        let mut unlocker = Unlocker::new(luks).with_registry(Registry::key_file());
        let volumes = [
            ("/dev/sda2", "va", "/keys/a.key"),
            ("/dev/sdb2", "vb", "/keys/b.key"),
        ];
        for (device, name, key) in volumes {
            let (device, name) = (device.parse().unwrap(), name.parse().unwrap());
            let _ = unlocker.open_device(Some(&name), &device, key, "", Origin::Configured);
        }
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
        .filter(|(_, message)| ["refused", "opened"].contains(&message.as_str()))
        .collect();
    assert_eq!(
        events,
        [
            ("KeyFile".to_owned(), "refused".to_owned()),
            ("KeyFile".to_owned(), "opened".to_owned()),
        ]
    );
}
