//! Installing a program's subscriber, which is global to the process: this
//! test has a process of its own.

#![allow(clippy::unwrap_used, reason = "a poisoned lock fails the test")]

use std::io::Write;
use std::sync::{Arc, Mutex};

use zbm_trace::{Config, LevelFilter, install};

/// A journal that tests can read back.
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_program_installs_its_subscriber_once() {
    let journal = Shared::default();
    let config = Config {
        stderr: LevelFilter::OFF,
        journal: Some(Box::new(journal.clone())),
    };

    install(config).unwrap();
    tracing::info!(pool = "tank", "imported");

    let text = String::from_utf8(journal.0.lock().unwrap().clone()).unwrap();
    assert!(text.contains("\"imported\""), "{text}");
    assert!(text.contains("\"tank\""), "{text}");
    let again = Config {
        stderr: LevelFilter::OFF,
        journal: None,
    };
    assert_eq!(
        install(again).unwrap_err().to_string(),
        "a tracing subscriber is already installed"
    );
}
