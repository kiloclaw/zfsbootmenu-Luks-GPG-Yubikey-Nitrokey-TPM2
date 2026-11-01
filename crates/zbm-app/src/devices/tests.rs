#![allow(
    clippy::disallowed_methods,
    reason = "the broker opens real files and devices in its tests"
)]

use std::io::Read as _;

use pretty_assertions::assert_eq;
use zbm_platform::event::spawn_scoped_traced;
use zbm_testkit::TempDir;

use super::*;

/// The device number of `major`:`minor`, as glibc's `makedev`.
fn makedev(major: u64, minor: u64) -> u64 {
    ((major & 0xfff) << 8) | ((major & !0xfff) << 32) | (minor & 0xff) | ((minor & !0xff) << 12)
}

#[test]
fn requests_and_replies_go_and_come_back_whole() {
    let request = Request {
        kind: Kind::Input,
        path: PathBuf::from("/dev/input/event3"),
    };
    let bytes = request.encode();
    assert!(String::from_utf8_lossy(&bytes).contains("\"open\":\"input\""));
    assert_eq!(Request::decode(&bytes), Ok(request));
    for reply in [Reply::Opened, Reply::Refused("no".to_owned())] {
        assert_eq!(Reply::decode(&reply.encode()), Some(reply));
    }
    assert!(Request::decode(b"{}").is_err());
    assert!(
        Request::decode(br#"{"version": 2, "open": "card", "path": "/dev/dri/card0"}"#).is_err()
    );
    assert!(Request::decode(br#"{"version": 1, "open": "disk", "path": "/dev/sda"}"#).is_err());
    assert!(Request::decode(br#"{"version": 1, "open": "card"}"#).is_err());
    assert_eq!(Reply::decode(br#"{"version": 1, "reply": "maybe"}"#), None);
    assert_eq!(Reply::decode(br#"{"version": 2, "reply": "opened"}"#), None);
    assert_eq!(Reply::decode(b"not json"), None);
}

#[test]
fn device_numbers_come_apart_as_glibc_takes_them() {
    for (major, minor) in [
        (226_u32, 0_u32),
        (13, 300),
        (4, 64),
        (188, 1),
        (4095, 1_048_575),
    ] {
        let rdev = makedev(u64::from(major), u64::from(minor));
        assert_eq!(numbers(rdev), (major, minor));
    }
}

#[test]
fn only_a_name_of_the_kind_below_the_devices_is_opened() {
    let dev = Path::new("/dev");
    let named = |kind, path: &str| name_below(kind, Path::new(path), dev);
    assert_eq!(
        named(Kind::Card, "/dev/dri/card0").as_deref(),
        Some("dri/card0")
    );
    assert_eq!(named(Kind::Terminal, "/dev/tty3").as_deref(), Some("tty3"));
    assert_eq!(named(Kind::Card, "/dev/dri/../sda"), None);
    assert_eq!(
        named(Kind::Card, "/dev/./dri/card0"),
        Some("dri/card0".to_owned())
    );
    assert_eq!(named(Kind::Terminal, "/tmp/tty3"), None);
    assert_eq!(named(Kind::Terminal, "/dev/sda"), None);
    assert_eq!(named(Kind::Input, "/dev/input/by-id/kbd-event"), None);

    let error = open_checked(Kind::Card, Path::new("/dev/sda"), dev).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.to_string().contains("is not a card"), "{error}");
}

#[test]
fn a_file_or_a_link_where_a_device_should_be_is_refused() {
    let dir = TempDir::new("zbm-broker");
    dir.write("tty1", "not a terminal");
    std::os::unix::fs::symlink("/dev/null", dir.join("tty2")).unwrap();

    let file = open_checked(Kind::Terminal, &dir.join("tty1"), dir.path()).unwrap_err();
    let link = open_checked(Kind::Terminal, &dir.join("tty2"), dir.path()).unwrap_err();
    let missing = open_checked(Kind::Terminal, &dir.join("tty3"), dir.path()).unwrap_err();

    assert!(
        file.to_string().contains("not a character device"),
        "{file}"
    );
    assert!(
        link.to_string().contains("not a character device"),
        "{link}"
    );
    assert_eq!(missing.kind(), io::ErrorKind::NotFound);
    // A character device of another kind: the null device is 1:3.
    let null = std::fs::metadata("/dev/null").unwrap();
    assert!(not_of_kind(Kind::Terminal, &null).is_some_and(|why| why.contains("1:3")));
}

#[test]
fn the_broker_hands_on_what_the_supervisor_opened_or_why_it_did_not() {
    let dir = TempDir::new("zbm-broker-serve");
    let opened = dir.write("event3", "keys");
    let (console, supervisor) = Connection::pair().unwrap();
    let refusals = Mutex::new(Vec::new());
    let open = |kind: Kind, path: &Path| {
        if kind == Kind::Input && path == Path::new("/dev/input/event3") {
            File::open(&opened)
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "not that one",
            ))
        }
    };

    std::thread::scope(|scope| {
        let served = spawn_scoped_traced(scope, || {
            serve(&supervisor, open, |line| {
                refusals.lock().unwrap().push(line.to_owned());
            });
        });
        let broker = Broker::new(console);
        let mut file = broker
            .open(Kind::Input, Path::new("/dev/input/event3"))
            .unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        assert_eq!(text, "keys");
        let refused = broker
            .open(Kind::Card, Path::new("/dev/dri/card0"))
            .unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::Other);
        assert!(refused.to_string().contains("not that one"), "{refused}");
        // A request it cannot read is answered too.
        let connection = broker.connection.lock().unwrap();
        connection.send(b"{\"version\": 1}").unwrap();
        let (reply, fds) = connection.receive_with(Some(OPEN_WAIT)).unwrap().unwrap();
        assert!(fds.is_empty());
        assert!(matches!(Reply::decode(&reply), Some(Reply::Refused(_))));
        drop(connection);
        drop(broker);
        served.join().unwrap();
    });

    let refusals = refusals.into_inner().unwrap();
    assert_eq!(refusals.len(), 2, "{refusals:?}");
    assert!(refusals[0].contains("a card"), "{refusals:?}");
}

#[test]
fn a_supervisor_that_answers_wrongly_or_not_at_all_opens_nothing() {
    let (console, supervisor) = Connection::pair().unwrap();
    let broker = Broker::new(console);
    supervisor.send(&Reply::Opened.encode()).unwrap();
    let error = broker
        .open(Kind::Card, Path::new("/dev/dri/card0"))
        .unwrap_err();
    assert!(error.to_string().contains("no descriptor"), "{error}");
    supervisor.send(b"nonsense").unwrap();
    let error = broker
        .open(Kind::Card, Path::new("/dev/dri/card0"))
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    drop(supervisor);
    assert!(
        broker
            .open(Kind::Card, Path::new("/dev/dri/card0"))
            .is_err()
    );
}

#[test]
fn a_terminal_of_the_machine_is_opened_when_this_process_may() {
    let dev = Path::new("/dev");
    match open_checked(Kind::Terminal, Path::new("/dev/tty1"), dev) {
        Ok(file) => {
            let (major, minor) = numbers(file.metadata().unwrap().rdev());
            assert_eq!((major, minor), (4, 1));
        }
        // Not root, or no virtual terminal on this machine.
        Err(error) => assert!(
            matches!(
                error.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::NotFound
            ),
            "{error}"
        ),
    }
    let serial = open_checked(Kind::Serial, Path::new("/dev/tty1"), dev).unwrap_err();
    assert!(serial.to_string().contains("is not a serial"), "{serial}");
}

#[test]
fn a_console_gone_before_its_reply_ends_the_broker() {
    let (console, supervisor) = Connection::pair().unwrap();
    let request = Request {
        kind: Kind::Input,
        path: PathBuf::from("/dev/input/event0"),
    };
    console.send(&request.encode()).unwrap();
    drop(console);

    serve(&supervisor, |_, _| File::open("/dev/null"), |_| {});
}

#[test]
fn a_supervisor_that_does_not_answer_in_time_opens_nothing() {
    let (console, _supervisor) = Connection::pair().unwrap();
    let broker = Broker::new(console).with_wait(Duration::from_millis(10));
    let error = broker
        .open(Kind::Card, Path::new("/dev/dri/card0"))
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}
