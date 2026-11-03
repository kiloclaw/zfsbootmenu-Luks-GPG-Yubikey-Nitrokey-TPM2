//! The boot of an environment whose key stays locked, as the legacy
//! function boots it.

use super::*;

/// The plain console, where the person types one key when asked, once:
/// `zfs` tried a key typed on its terminal once in the legacy boot, as its
/// test answers nothing more.
pub(super) struct OneKey<'a> {
    pub(super) console: ConsoleInteraction<'a>,
    pub(super) typed: std::sync::atomic::AtomicBool,
}

impl Interaction for OneKey<'_> {
    fn ask(&self, request: &PromptRequest) -> zbm_types::Answer {
        self.console.ask(request)
    }

    fn secret(&self, _: &SecretRequest<'_>) -> Asked {
        if self.typed.swap(true, std::sync::atomic::Ordering::SeqCst) {
            Asked::Ended
        } else {
            Asked::Secret(zbm_platform::Secret::copy_from(b"zfskey12"))
        }
    }

    fn wait(&self, request: &WaitRequest<'_>) -> Wait {
        self.console.wait(request)
    }

    fn notify(&self, tell: Tell<'_>) {
        self.console.notify(tell);
    }

    fn booting(&self, kernel: &str, environment: &str) {
        self.console.booting(kernel, environment);
    }
}

#[test]
fn boots_a_locked_environment_as_the_legacy_function_does() {
    let mut parity = parity!("boots_a_locked_environment_as_the_legacy_function_does");
    // The hook leaves it locked, and so does the passphrase prompt: the
    // mount refuses it.
    let locked = |keylocation| Machine {
        answers: vec![
            (0, "zpool list -H -o feature@encryption tank", "active\\n"),
            (0, "zfs get -H -o value encryptionroot *", "tank/ROOT\\n"),
            (0, "zfs get -H -o value keystatus *", "unavailable\\n"),
            (0, "zfs get -H -o value keylocation *", keylocation),
            (0, "zfs get -H -o value keyformat *", "hex\\n"),
            (1, "zfs load-key *", ""),
        ],
        files: vec![],
        hooks: true,
    };
    boot(&locked("prompt\\n"), &mut parity);
    // A key file that is not in the image, and a key that is not a
    // passphrase.
    boot(&locked("file:///zbm-test/missing.key\\n"), &mut parity);
    // A key source, locked with the same missing key: its own cache
    // attempt is not repeated.
    let mut with_source = locked("file:///zbm-test/missing.key\\n");
    with_source.answers.insert(
        0,
        (
            0,
            "zfs get -H -o value org.zfsbootmenu:keysource *",
            "tank/keys\\n",
        ),
    );
    boot(&with_source, &mut parity);
}
