#![allow(
    clippy::disallowed_methods,
    reason = "zbm-sys is tested against the kernel itself: real programs, files and time"
)]

use super::*;

/// A path for a socket, in a new directory of its own.
fn socket_path(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("zbm-sys-socket-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    directory.join("run").join("test.sock")
}

#[test]
fn packets_come_whole_and_in_order_from_a_peer_the_kernel_names() {
    let path = socket_path("whole");
    let listener = Listener::bind(&path).unwrap();
    assert_eq!(listener.path(), path);
    let client = Connection::connect(&path).unwrap();
    let server = listener
        .accept(Some(Duration::from_secs(5)))
        .unwrap()
        .unwrap();

    client.send(b"one").unwrap();
    client.send(b"").unwrap();
    client.send(b"three").unwrap();

    let wait = Some(Duration::from_secs(5));
    assert_eq!(server.receive(wait).unwrap().unwrap(), b"one");
    assert_eq!(server.receive(wait).unwrap().unwrap(), b"");
    assert_eq!(server.receive(wait).unwrap().unwrap(), b"three");
    assert_eq!(server.receive(Some(Duration::ZERO)).unwrap(), None);
    let peer = server.peer().unwrap();
    assert_eq!(peer.pid, std::process::id());
    assert_eq!(peer.uid, crate::credentials().uid);
    // The directory is the owner's alone.
    let mode = fs::metadata(path.parent().unwrap()).unwrap();
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o777,
        0o700
    );

    drop(client);
    assert_eq!(server.receive(wait).unwrap().unwrap(), b"");
    drop(listener);
    assert!(!path.exists());
}

#[test]
fn a_connection_passed_to_this_process_is_taken_once() {
    use std::os::fd::AsRawFd as _;
    let (one, other) = Connection::pair().unwrap();
    let number = one.fd.into_raw_fd();

    let taken = Connection::inherited(number).unwrap();
    assert_eq!(taken.as_fd().as_raw_fd(), number);
    let error = Connection::inherited(number).unwrap_err();
    assert!(error.to_string().contains("taken before"), "{error}");
    taken.set_nonblocking().unwrap();
    // A whole second, which the kernel's clock ticks keep as it is.
    other.set_send_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(
        sockopt::socket_timeout(&other.fd, sockopt::Timeout::Send).unwrap(),
        Some(Duration::from_secs(1))
    );
    other.send(b"hello").unwrap();
    assert_eq!(
        taken.receive(Some(Duration::ZERO)).unwrap().unwrap(),
        b"hello"
    );
    // Without room, at once.
    let error = loop {
        if let Err(error) = other.try_send(&[0; 1024]) {
            break error;
        }
    };
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert!(other.try_send(&vec![0; MAX_MESSAGE + 1]).is_err());
    // Closed on `exec`, as it was not when passed.
    let flags = rustix::io::fcntl_getfd(&taken).unwrap();
    assert!(flags.contains(rustix::io::FdFlags::CLOEXEC));

    // Neither a standard stream, a file, a closed number nor a socket
    // of another kind, which is left open.
    let file = fs::File::open("/dev/null").unwrap();
    let stream = rustix::net::socket(AddressFamily::UNIX, SocketType::STREAM, None).unwrap();
    for (number, why) in [
        (0, "standard stream"),
        (file.as_raw_fd(), "not a socket"),
        (stream.as_raw_fd(), "of packets"),
    ] {
        let error = Connection::inherited(number).unwrap_err();
        assert!(error.to_string().contains(why), "{number}: {error}");
    }
    assert!(Connection::inherited(i32::MAX).is_err());
    assert!(sockopt::socket_type(&stream).is_ok());
}

#[test]
fn a_socket_in_use_is_kept_and_one_left_behind_is_replaced() {
    let path = socket_path("in-use");
    let listener = Listener::bind(&path).unwrap();
    let error = Listener::bind(&path).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    // The attempt connected to see whether it was in use.
    assert!(listener.accept(Some(Duration::ZERO)).unwrap().is_some());
    drop(listener);

    // A socket nobody listens on, as a process that ended leaves.
    let left = packet_socket().unwrap();
    bind(&left, &SocketAddrUnix::new(&path).unwrap()).unwrap();
    drop(left);
    assert!(path.exists());
    let listener = Listener::bind(&path).unwrap();
    assert!(Connection::connect(&path).is_ok());
    drop(listener);
}

#[test]
fn nothing_waits_when_no_peer_comes_and_too_long_a_message_is_refused() {
    // A child forked meanwhile would hold the peer open until its exec.
    let _guard = crate::tests::PROCESSES.lock().unwrap();
    let path = socket_path("nothing");
    let listener = Listener::bind(&path).unwrap();
    assert!(listener.accept(Some(Duration::ZERO)).unwrap().is_none());
    assert!(Connection::connect(&path.with_file_name("none.sock")).is_err());

    let (one, other) = Connection::pair().unwrap();
    let long = vec![0; MAX_MESSAGE + 1];
    assert_eq!(
        one.send(&long).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    // Sent past the check, as another program could: refused on receipt.
    rustix::net::send(&other, &long, SendFlags::empty()).unwrap();
    let readable = wait_ready(&[Watch::read(one.as_fd())], Some(Duration::from_secs(5)));
    assert!(readable.unwrap().first().is_some_and(|ready| ready.read));
    let error = one.receive(Some(Duration::ZERO)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    drop(other);
    assert!(one.send(b"gone").is_err());
}

#[test]
fn descriptors_cross_with_a_packet_and_stay_usable() {
    let (one, other) = Connection::pair().unwrap();
    // A second pair stands for a terminal's input and output.
    let (input, output) = Connection::pair().unwrap();
    one.send_with(b"terminal", &[input.as_fd(), output.as_fd()])
        .unwrap();
    drop((input, output));

    let wait = Some(Duration::from_secs(5));
    let (message, fds) = other.receive_with(wait).unwrap().unwrap();
    assert_eq!(message, b"terminal");
    assert_eq!(fds.len(), 2);
    // Closed on `exec`, and the same pipe.
    let flags = rustix::io::fcntl_getfd(&fds[0]).unwrap();
    assert!(flags.contains(rustix::io::FdFlags::CLOEXEC));
    let [input, output] = <[OwnedFd; 2]>::try_from(fds).unwrap();
    let (input, output) = (Connection { fd: input }, Connection { fd: output });
    output.send(b"x").unwrap();
    assert_eq!(input.receive(wait).unwrap().unwrap(), b"x");

    // A packet without descriptors, either way.
    one.send(b"plain").unwrap();
    let (message, fds) = other.receive_with(wait).unwrap().unwrap();
    assert_eq!((message.as_slice(), fds.len()), (&b"plain"[..], 0));
    one.send_with(b"none", &[]).unwrap();
    assert_eq!(other.receive(wait).unwrap().unwrap(), b"none");
    assert!(other.receive_with(Some(Duration::ZERO)).unwrap().is_none());
}

#[test]
fn too_many_descriptors_or_bytes_are_refused() {
    let (one, other) = Connection::pair().unwrap();
    let fd = one.as_fd();
    let many = vec![fd; MAX_FDS + 1];
    let error = one.send_with(b"many", &many).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    let long = vec![0; MAX_MESSAGE + 1];
    let error = one.send_with(&long, &[]).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    // A peer that sends more than a packet holds is refused.
    let raw = rustix::net::socketpair(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::SEQPACKET,
        rustix::net::SocketFlags::CLOEXEC,
        None,
    )
    .unwrap();
    let ours = Connection { fd: raw.0 };
    rustix::net::send(&raw.1, &long, SendFlags::empty()).unwrap();
    let error = ours.receive_with(Some(Duration::from_secs(5))).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    drop(other);
}

#[test]
fn what_a_peer_sent_before_it_went_with_packets_unread_still_comes() {
    let wait = Some(Duration::from_secs(5));
    for with_fds in [false, true] {
        let (one, other) = Connection::pair().unwrap();
        // `one` sends a packet `other` never reads, `other` answers, and
        // goes: the kernel resets the connection.
        one.send(b"unread").unwrap();
        other.send(b"last words").unwrap();
        drop(other);
        let last = if with_fds {
            one.receive_with(wait).unwrap().unwrap().0
        } else {
            one.receive(wait).unwrap().unwrap()
        };
        assert_eq!(last, b"last words");
        // Then the peer is gone.
        let after = one.receive(wait);
        assert!(
            matches!(&after, Ok(Some(bytes)) if bytes.is_empty()) || after.is_err(),
            "{after:?}"
        );
    }
}
