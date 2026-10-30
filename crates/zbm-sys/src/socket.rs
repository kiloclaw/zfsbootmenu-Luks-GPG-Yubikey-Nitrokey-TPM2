//! Unix sockets of packets (`SOCK_SEQPACKET`), which the zones of the
//! runtime talk over (ADR 0002): each send is one message, kept whole and
//! in order, and each end can tell who the other is (`SO_PEERCRED`), so a
//! socket refuses the peers it does not trust. The standard library has
//! none of this yet.

use std::fs::{self, DirBuilder};
use std::io;
use std::os::fd::{AsFd, BorrowedFd, FromRawFd as _, IntoRawFd as _, OwnedFd};
use std::os::unix::fs::{DirBuilderExt as _, FileTypeExt as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use rustix::net::{
    AddressFamily, RecvFlags, SendFlags, SocketAddrUnix, SocketFlags, SocketType, accept_with,
    bind, connect, listen, recv, send, socket_with, sockopt,
};

use crate::console::{Watch, wait_ready};

/// The longest message a socket receives; a longer one is refused.
pub const MAX_MESSAGE: usize = 64 * 1024;

/// How many peers may wait to be accepted.
const BACKLOG: i32 = 4;

/// Who is at the other end of a socket, as the kernel says.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Peer {
    /// Its process.
    pub pid: u32,
    /// Its user.
    pub uid: u32,
}

/// A new socket of packets, closed on `exec`.
fn packet_socket() -> io::Result<OwnedFd> {
    let fd = socket_with(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::CLOEXEC,
        None,
    )?;
    Ok(fd)
}

/// Waits until `fd` is readable, at most `timeout` (without limit when
/// `None`); whether it is.
fn is_readable(fd: BorrowedFd<'_>, timeout: Option<Duration>) -> io::Result<bool> {
    let ready = wait_ready(&[Watch::read(fd)], timeout)?;
    Ok(ready
        .first()
        .is_some_and(|ready| ready.read || ready.closed))
}

/// A socket that peers connect to at a path, which it removes when it is
/// dropped.
#[derive(Debug)]
pub struct Listener {
    fd: OwnedFd,
    path: PathBuf,
}

impl Listener {
    /// Listens at `path`, making its directory, only its owner's, when it is
    /// missing. A socket left there by a process that ended is replaced; one
    /// that a process still listens on is not.
    ///
    /// # Errors
    ///
    /// When the directory or the socket cannot be made, or a process still
    /// listens at `path` (`AddrInUse`).
    #[allow(
        clippy::disallowed_methods,
        reason = "a socket is no file the Files interface reads or writes"
    )]
    pub fn bind(path: &Path) -> io::Result<Listener> {
        if let Some(directory) = path.parent() {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(directory)?;
        }
        let address = SocketAddrUnix::new(path)?;
        if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket()) {
            if Connection::connect(path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("a process listens at {}", path.display()),
                ));
            }
            fs::remove_file(path)?;
        }
        let fd = packet_socket()?;
        bind(&fd, &address)?;
        let listener = Listener {
            fd,
            path: path.to_owned(),
        };
        listen(&listener.fd, BACKLOG)?;
        Ok(listener)
    }

    /// The next peer that connects, waiting at most `timeout` (without limit
    /// when `None`): `None` when none came.
    ///
    /// # Errors
    ///
    /// When the socket cannot be waited on, or the peer accepted.
    pub fn accept(&self, timeout: Option<Duration>) -> io::Result<Option<Connection>> {
        if !is_readable(self.fd.as_fd(), timeout)? {
            return Ok(None);
        }
        match accept_with(&self.fd, SocketFlags::CLOEXEC) {
            Ok(fd) => Ok(Some(Connection { fd })),
            // The peer went before it was accepted.
            Err(rustix::io::Errno::AGAIN | rustix::io::Errno::CONNABORTED) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Where it listens.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl AsFd for Listener {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

impl Drop for Listener {
    #[allow(
        clippy::disallowed_methods,
        reason = "a socket is no file the Files interface reads or writes"
    )]
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// One end of a connection of packets.
#[derive(Debug)]
pub struct Connection {
    fd: OwnedFd,
}

impl Connection {
    /// Connects to the socket at `path`.
    ///
    /// # Errors
    ///
    /// When none listens there.
    pub fn connect(path: &Path) -> io::Result<Connection> {
        let address = SocketAddrUnix::new(path)?;
        let fd = packet_socket()?;
        connect(&fd, &address)?;
        Ok(Connection { fd })
    }

    /// A pair of connected ends, as for tests.
    ///
    /// # Errors
    ///
    /// When the sockets cannot be made.
    pub fn pair() -> io::Result<(Connection, Connection)> {
        let (one, other) = rustix::net::socketpair(
            AddressFamily::UNIX,
            SocketType::SEQPACKET,
            SocketFlags::CLOEXEC,
            None,
        )?;
        Ok((Connection { fd: one }, Connection { fd: other }))
    }

    /// The end of a connection this process was started with, at the
    /// descriptor `number` that its parent passed it
    /// ([`pass_fd`](crate::pass_fd)), closed on `exec` from now on. Each
    /// number is taken once.
    ///
    /// # Errors
    ///
    /// When `number` is one of the standard streams, was taken before, or
    /// is not open on a Unix socket of packets.
    pub fn inherited(number: i32) -> io::Result<Connection> {
        static TAKEN: Mutex<Vec<i32>> = Mutex::new(Vec::new());
        let refused =
            |why: &str| io::Error::new(io::ErrorKind::InvalidInput, format!("{why}: {number}"));
        if number <= 2 {
            return Err(refused("a standard stream"));
        }
        let mut taken = TAKEN.lock().unwrap_or_else(PoisonError::into_inner);
        if taken.contains(&number) {
            return Err(refused("a descriptor taken before"));
        }
        // What the descriptor is, without touching it: `socket:[inode]`
        // for a socket; nothing for a number that is not open.
        let link = fs::read_link(format!("/proc/self/fd/{number}"))?;
        if !link.as_os_str().as_encoded_bytes().starts_with(b"socket:") {
            return Err(refused("not a socket"));
        }
        taken.push(number);
        // SAFETY: the kernel says `number` is open on a socket, and this
        // process takes it once: nothing else in it owns a socket it was
        // started with.
        let fd = unsafe { OwnedFd::from_raw_fd(number) };
        let is_packets = sockopt::socket_type(&fd).is_ok_and(|kind| kind == SocketType::SEQPACKET)
            && sockopt::socket_domain(&fd).is_ok_and(|family| family == AddressFamily::UNIX);
        if !is_packets {
            // Not ours to close.
            let _ = fd.into_raw_fd();
            return Err(refused("not a Unix socket of packets"));
        }
        rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC)?;
        Ok(Connection { fd })
    }

    /// Has [`Connection::send`] fail with [`io::ErrorKind::WouldBlock`]
    /// rather than wait while the other end reads nothing.
    ///
    /// # Errors
    ///
    /// When the socket cannot be changed.
    pub fn set_nonblocking(&self) -> io::Result<()> {
        rustix::io::ioctl_fionbio(&self.fd, true)?;
        Ok(())
    }

    /// Has [`Connection::send`] wait at most `wait` for room, then fail with
    /// [`io::ErrorKind::WouldBlock`], while the other end reads nothing.
    ///
    /// # Errors
    ///
    /// When the socket cannot be changed.
    pub fn set_send_timeout(&self, wait: Duration) -> io::Result<()> {
        sockopt::set_socket_timeout(&self.fd, sockopt::Timeout::Send, Some(wait))?;
        Ok(())
    }

    /// Who is at the other end.
    ///
    /// # Errors
    ///
    /// When the kernel does not say.
    pub fn peer(&self) -> io::Result<Peer> {
        let credentials = sockopt::socket_peercred(&self.fd)?;
        Ok(Peer {
            pid: credentials.pid.as_raw_nonzero().get().unsigned_abs(),
            uid: credentials.uid.as_raw(),
        })
    }

    /// Sends `message`, whole, as one packet.
    ///
    /// # Errors
    ///
    /// When it is longer than [`MAX_MESSAGE`], or the other end is gone.
    pub fn send(&self, message: &[u8]) -> io::Result<()> {
        if message.len() > MAX_MESSAGE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("a message of {} bytes", message.len()),
            ));
        }
        // No `SIGPIPE` when the other end is gone: an error instead.
        let sent = send(&self.fd, message, SendFlags::NOSIGNAL)?;
        if sent == message.len() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::WriteZero,
                format!("{sent} bytes of {} sent", message.len()),
            ))
        }
    }

    /// Sends `message` as one packet if there is room for it now, whatever
    /// the socket's mode, which other descriptors of it share.
    ///
    /// # Errors
    ///
    /// When it is longer than [`MAX_MESSAGE`], there is no room, or the
    /// other end is gone.
    pub fn try_send(&self, message: &[u8]) -> io::Result<()> {
        if message.len() > MAX_MESSAGE {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        send(&self.fd, message, SendFlags::NOSIGNAL | SendFlags::DONTWAIT)?;
        Ok(())
    }

    /// The next packet, waiting at most `timeout` (without limit when
    /// `None`): `None` when none came in time, an empty one when the other
    /// end is gone or sent an empty one.
    ///
    /// # Errors
    ///
    /// When the socket cannot be read, or the packet is longer than
    /// [`MAX_MESSAGE`].
    pub fn receive(&self, timeout: Option<Duration>) -> io::Result<Option<Vec<u8>>> {
        if !is_readable(self.fd.as_fd(), timeout)? {
            return Ok(None);
        }
        let mut buffer = vec![0; MAX_MESSAGE];
        let (_, length) = recv(&self.fd, &mut buffer[..], RecvFlags::TRUNC)?;
        if length > MAX_MESSAGE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("a message of {length} bytes"),
            ));
        }
        buffer.truncate(length);
        Ok(Some(buffer))
    }
}

impl AsFd for Connection {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "zbm-sys is tested against the kernel itself: real programs, files and time"
)]
mod tests {
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
}
