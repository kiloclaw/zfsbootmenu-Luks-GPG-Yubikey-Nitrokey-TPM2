//! The menu's Chroot, which [`open`] performs on the console.

use pretty_assertions::assert_eq;
use zbm_platform::Signal;

use super::*;
use crate::application::tests::NoShell;
use crate::model::{Action, Event, Outcome};
use crate::{App, Application as _, Runtime, Through};

/// Applies the chroot of `target` on `app`'s console, that of `fake`.
fn on_console(app: &App<'_>, fake: &FakeSystem, target: &str) -> Vec<Event> {
    let action = Action::Chroot(target.parse().unwrap());
    app.apply(&action, Through::Console(fake.system(), &NoShell))
}

#[test]
fn the_menu_s_chroot_opens_the_shell_through_the_gate_on_every_image() {
    let fake = machine("off");
    let runtime = Runtime::default();
    let app = App::new(Context::new(fake.system()), &runtime);

    let environment = on_console(&app, &fake, "tank/ROOT/be");
    let snapshot = on_console(&app, &fake, "tank/ROOT/be@old");

    assert_eq!(
        environment,
        [Event::Finished(Outcome::Done(
            "the shell in tank/ROOT/be ended".to_owned()
        ))]
    );
    assert!(matches!(
        snapshot.last(),
        Some(Event::Finished(Outcome::Done(_)))
    ));
    let calls = fake.runner.calls();
    let shell = calls
        .iter()
        .find(|call| call.display() == "/bin/sh")
        .expect("the shell ran");
    assert_eq!(shell.get_default_signals(), [Signal::Interrupt]);
    assert_eq!(shell.get_root(), Some(std::path::Path::new(MP)));

    // A shell that cannot start says why.
    fake.runner.first(|command| {
        (command.display() == "/bin/sh")
            .then(|| Err(io::Error::other("cannot change root directory")))
    });
    let failed = on_console(&app, &fake, "tank/ROOT/be");
    assert!(
        matches!(
            failed.as_slice(),
            [Event::Finished(Outcome::Failed { what, why, .. })]
                if what == "unable to open a shell in tank/ROOT/be"
                    && why == &[
                        "unable to start a shell in the chroot",
                        "cannot change root directory",
                    ]
        ),
        "{failed:?}"
    );
}

#[test]
fn the_locked_menu_runs_the_chroot_only_after_the_pin_and_locks_again() {
    let fake = machine("on");
    let runtime = Runtime::default();
    let authority = Authority::new(Lock::On("hash".to_owned()));
    let app = App::new(Context::new(fake.system()), &runtime).with_authority(&authority);
    let chroot = Action::Chroot("tank/ROOT/be".parse().unwrap());
    let ran_chroot = || {
        fake.runner
            .lines()
            .iter()
            .filter(|line| line.as_str() == "/bin/sh")
            .count()
    };

    // Quietly, and on the console with no keyholder to check a PIN:
    // nothing is asked, imported or mounted.
    assert_eq!(
        app.apply(&chroot, Through::Quietly(fake.system())),
        crate::auth::refused()
    );
    assert_eq!(
        app.apply(&chroot, Through::Console(fake.system(), &NoShell)),
        crate::auth::refused()
    );
    assert_eq!(ran(&fake), Vec::<String>::new());

    // The right PIN: the chroot runs, and the menu is locked again after.
    let (client, keyholder) = zbm_sys::socket::Connection::pair().unwrap();
    fake.console.secret(b"correct horse");
    let runner = &fake.runner;
    let events = std::thread::scope(|scope| {
        zbm_platform::event::spawn_scoped_traced(scope, move || {
            let _ = crate::keyholder::serve(
                &keyholder,
                runner,
                |_, _| Vec::new(),
                |pin| pin.expose() == b"correct horse",
            );
        });
        let keyholders = crate::keyholder::tests::Given::new(client);
        let events = app
            .with_keyholders(&keyholders)
            .apply(&chroot, Through::Console(fake.system(), &NoShell));
        drop(keyholders);
        events
    });
    assert!(
        matches!(events.last(), Some(Event::Finished(Outcome::Done(_)))),
        "{events:?}"
    );
    assert_eq!(ran_chroot(), 1, "{:?}", fake.runner.lines());
    assert!(app.snapshot().unwrap().menu_locked);
}

#[test]
fn the_menu_s_chroot_binds_the_image_s_resolv_conf() {
    let fake = machine("off");
    fake.files
        .file("/etc/resolv.conf", "nameserver 10.0.2.3\n")
        .file(format!("{MP}/etc/resolv.conf"), "");
    let runtime = Runtime::default();
    let app = App::new(Context::new(fake.system()), &runtime);
    let events = on_console(&app, &fake, "tank/ROOT/be");
    assert!(
        matches!(events.last(), Some(Event::Finished(Outcome::Done(_)))),
        "{events:?}"
    );
    let lines = ran(&fake);
    let target = format!("{MP}/etc/resolv.conf");
    assert_eq!(
        lines[1..3],
        [
            format!("mount -B /etc/resolv.conf {target}"),
            format!("mount -o remount,bind,ro {target}"),
        ]
    );
    assert!(lines.contains(&format!("umount {target}")), "{lines:#?}");
}

#[test]
fn the_menu_s_chroot_refuses_a_tmp_that_links_to_etc() {
    let fake = machine("off");
    fake.files.symlink(format!("{MP}/tmp"), "/etc");
    let runtime = Runtime::default();
    let app = App::new(Context::new(fake.system()), &runtime);
    let events = on_console(&app, &fake, "tank/ROOT/be");
    assert!(
        matches!(events.last(), Some(Event::Finished(Outcome::Done(_)))),
        "{events:?}"
    );
    let lines = ran(&fake);
    assert!(
        !lines.iter().any(|line| line.contains("/tmp")),
        "{lines:#?}"
    );
    assert!(lines.contains(&format!("mount -t proc proc {MP}/proc")));
    let shown = decolorize(&fake.console.output_text());
    assert!(
        shown.contains("* /tmp is not mounted: it is a link to /etc\n"),
        "{shown}"
    );
    assert!(!shown.contains("/tmp is mounted"), "{shown}");
}
