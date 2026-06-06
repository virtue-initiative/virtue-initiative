use std::fmt;

use crate::events::Event;

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum IpcError {
    DaemonNotRunning,
    Disconnected,
    Io(std::io::Error),
    Protocol(serde_json::Error),
    Remote(String),
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IpcError::DaemonNotRunning => write!(f, "daemon is not running"),
            IpcError::Disconnected => write!(f, "connection to daemon closed"),
            IpcError::Io(e) => write!(f, "I/O error: {e}"),
            IpcError::Protocol(e) => write!(f, "protocol error: {e}"),
            IpcError::Remote(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for IpcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IpcError::Io(e) => Some(e),
            IpcError::Protocol(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for IpcError {
    fn from(e: std::io::Error) -> Self {
        IpcError::Io(e)
    }
}

impl From<serde_json::Error> for IpcError {
    fn from(e: serde_json::Error) -> Self {
        IpcError::Protocol(e)
    }
}

// ── Inbound event filter ──────────────────────────────────────────────────────

/// Whether an event received from a controller is allowed to be forwarded to
/// the daemon's event loop.
pub fn is_allowed_inbound(event: &Event) -> bool {
    matches!(
        event,
        Event::UserSessionLogin
            | Event::UserSessionLogout
            | Event::ComputerSuspended
            | Event::ComputerResumed
            | Event::ProcessStopped(_)
            | Event::LoginRequested { .. }
            | Event::LogoutRequested
            | Event::UserStopRequested { .. }
            | Event::StatusRequest
    )
}

// ── Unix domain socket transport (Linux / macOS) ──────────────────────────────

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_impl {
    use std::io::{BufRead, BufReader, BufWriter, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;

    use super::{Event, IpcError};

    pub struct IpcSender {
        writer: BufWriter<UnixStream>,
    }

    pub struct IpcReceiver {
        reader: BufReader<UnixStream>,
    }

    pub struct IpcListener {
        listener: UnixListener,
    }

    impl IpcSender {
        pub fn send(&mut self, event: &Event) -> Result<(), IpcError> {
            let json = serde_json::to_string(event)?;
            self.writer.write_all(json.as_bytes())?;
            self.writer.write_all(b"\n")?;
            self.writer.flush()?;
            Ok(())
        }
    }

    impl IpcReceiver {
        pub fn recv_event(&mut self) -> Result<Event, IpcError> {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line)?;
            if n == 0 {
                return Err(IpcError::Disconnected);
            }
            Ok(serde_json::from_str(line.trim())?)
        }

        pub fn try_recv_event(&mut self) -> Result<Option<Event>, IpcError> {
            // If BufReader's buffer already holds a complete line, parse it without
            // touching the socket — avoids spurious WouldBlock on a buffered newline.
            if self.reader.buffer().contains(&b'\n') {
                return self.recv_event().map(Some);
            }
            self.reader.get_ref().set_nonblocking(true)?;
            let result = self.recv_event();
            let _ = self.reader.get_ref().set_nonblocking(false);
            match result {
                Ok(ev) => Ok(Some(ev)),
                Err(IpcError::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
                Err(e) => Err(e),
            }
        }
    }

    impl IpcListener {
        pub fn bind(path: &Path) -> Result<Self, IpcError> {
            let _ = std::fs::remove_file(path);
            let listener = UnixListener::bind(path)?;
            Ok(Self { listener })
        }

        pub fn blocking_accept(&self) -> Result<(IpcSender, IpcReceiver), IpcError> {
            let (stream, _) = self.listener.accept()?;
            stream.set_nonblocking(false)?;
            let read_stream = stream.try_clone()?;
            Ok((
                IpcSender {
                    writer: BufWriter::new(stream),
                },
                IpcReceiver {
                    reader: BufReader::new(read_stream),
                },
            ))
        }
    }

    pub fn connect(path: &Path) -> Result<(IpcSender, IpcReceiver), IpcError> {
        let stream = UnixStream::connect(path).map_err(|e| {
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) {
                IpcError::DaemonNotRunning
            } else {
                IpcError::Io(e)
            }
        })?;
        let read_stream = stream.try_clone()?;
        Ok((
            IpcSender {
                writer: BufWriter::new(stream),
            },
            IpcReceiver {
                reader: BufReader::new(read_stream),
            },
        ))
    }
}

// ── In-process mpsc transport (Windows / Android / iOS) ──────────────────────

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod mpsc_impl {
    use std::path::Path;
    use std::sync::{OnceLock, mpsc};

    use super::{Event, IpcError};

    // Set explicitly by the platform after IpcListener::bind() via register_connect_tx().
    static CONNECT_TX: OnceLock<mpsc::SyncSender<(IpcSender, IpcReceiver)>> = OnceLock::new();

    pub struct IpcSender {
        tx: mpsc::SyncSender<String>,
    }

    pub struct IpcReceiver {
        rx: mpsc::Receiver<String>,
    }

    pub struct IpcListener {
        accept_rx: mpsc::Receiver<(IpcSender, IpcReceiver)>,
        pub connect_tx: mpsc::SyncSender<(IpcSender, IpcReceiver)>,
    }

    impl IpcSender {
        pub fn send(&mut self, event: &Event) -> Result<(), IpcError> {
            let json = serde_json::to_string(event)?;
            self.tx.send(json).map_err(|_| IpcError::Disconnected)
        }
    }

    impl IpcReceiver {
        pub fn recv_event(&mut self) -> Result<Event, IpcError> {
            let json = self.rx.recv().map_err(|_| IpcError::Disconnected)?;
            Ok(serde_json::from_str(&json)?)
        }

        pub fn try_recv_event(&mut self) -> Result<Option<Event>, IpcError> {
            match self.rx.try_recv() {
                Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
                Err(mpsc::TryRecvError::Empty) => Ok(None),
                Err(mpsc::TryRecvError::Disconnected) => Err(IpcError::Disconnected),
            }
        }
    }

    impl IpcListener {
        pub fn bind(_path: &Path) -> Result<Self, IpcError> {
            let (tx, rx) = mpsc::sync_channel(8);
            Ok(Self {
                accept_rx: rx,
                connect_tx: tx,
            })
        }

        pub fn blocking_accept(&self) -> Result<(IpcSender, IpcReceiver), IpcError> {
            self.accept_rx.recv().map_err(|_| IpcError::Disconnected)
        }
    }

    /// Register the listener's connect sender so that `connect()` works.
    /// Must be called by the platform after `IpcListener::bind()`.
    pub fn register_connect_tx(tx: mpsc::SyncSender<(IpcSender, IpcReceiver)>) {
        CONNECT_TX.set(tx).ok();
    }

    /// Connect using an explicit sender — avoids any global state.
    pub fn connect_in_process(
        tx: &mpsc::SyncSender<(IpcSender, IpcReceiver)>,
    ) -> Result<(IpcSender, IpcReceiver), IpcError> {
        let (c2d_tx, c2d_rx) = mpsc::sync_channel::<String>(100);
        let (d2c_tx, d2c_rx) = mpsc::sync_channel::<String>(100);

        let daemon_pair = (IpcSender { tx: d2c_tx }, IpcReceiver { rx: c2d_rx });
        let ctrl_pair = (IpcSender { tx: c2d_tx }, IpcReceiver { rx: d2c_rx });

        tx.send(daemon_pair).map_err(|_| IpcError::Disconnected)?;
        Ok(ctrl_pair)
    }

    pub fn connect(_path: &Path) -> Result<(IpcSender, IpcReceiver), IpcError> {
        let tx = CONNECT_TX.get().ok_or(IpcError::DaemonNotRunning)?;
        connect_in_process(tx)
    }
}

// ── Public re-exports ─────────────────────────────────────────────────────────

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use unix_impl::{IpcListener, IpcReceiver, IpcSender, connect};

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use mpsc_impl::{
    IpcListener, IpcReceiver, IpcSender, connect, connect_in_process, register_connect_tx,
};

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{Event, Redacted};

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn unix_socket_send_recv_round_trips() {
        let sock =
            std::env::temp_dir().join(format!("virtue-ipc-test-{}.sock", std::process::id()));
        let listener = IpcListener::bind(&sock).expect("bind");

        let sock2 = sock.clone();
        let (_ctrl_sender, mut ctrl_receiver) =
            std::thread::spawn(move || connect(&sock2).expect("connect"))
                .join()
                .expect("connect thread");

        let (mut daemon_sender, _daemon_receiver) = listener.blocking_accept().expect("accept");

        let event = Event::UserSessionLogin;
        daemon_sender.send(&event).expect("send");

        let received = ctrl_receiver.recv_event().expect("recv");
        assert!(matches!(received, Event::UserSessionLogin));

        let _ = std::fs::remove_file(&sock);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn unix_socket_login_request_response_round_trips() {
        let sock =
            std::env::temp_dir().join(format!("virtue-ipc-test-login-{}.sock", std::process::id()));
        let listener = IpcListener::bind(&sock).expect("bind");

        let sock2 = sock.clone();
        let ctrl = std::thread::spawn(move || connect(&sock2).expect("connect"))
            .join()
            .expect("connect thread");
        let (mut ctrl_sender, mut ctrl_receiver) = ctrl;

        let (mut daemon_sender, mut daemon_receiver) = listener.blocking_accept().expect("accept");

        // Controller sends LoginRequested
        ctrl_sender
            .send(&Event::LoginRequested {
                email: "test@example.com".into(),
                password: Redacted("secret".into()),
            })
            .expect("send login request");

        // Daemon receives it
        let req = daemon_receiver.recv_event().expect("recv request");
        assert!(matches!(req, Event::LoginRequested { .. }));

        // Daemon responds with LoginResult
        daemon_sender
            .send(&Event::LoginResult {
                success: true,
                error: None,
                device_id: Some("device-123".into()),
            })
            .expect("send login result");

        // Controller receives it
        let result = ctrl_receiver.recv_event().expect("recv result");
        assert!(matches!(result, Event::LoginResult { success: true, .. }));

        let _ = std::fs::remove_file(&sock);
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    #[test]
    fn mpsc_send_recv_round_trips() {
        use std::path::Path;
        let path = Path::new("/ignored");
        let listener = IpcListener::bind(path).expect("bind");

        let (mut ctrl_sender, mut ctrl_receiver) =
            connect_in_process(&listener.connect_tx).expect("connect");
        let (mut daemon_sender, mut daemon_receiver) = listener.blocking_accept().expect("accept");

        daemon_sender.send(&Event::UserSessionLogin).expect("send");
        let received = ctrl_receiver.recv_event().expect("recv");
        assert!(matches!(received, Event::UserSessionLogin));

        ctrl_sender
            .send(&Event::LoginRequested {
                email: "a@b.com".into(),
                password: Redacted("pw".into()),
            })
            .expect("send request");
        let req = daemon_receiver.recv_event().expect("recv request");
        assert!(matches!(req, Event::LoginRequested { .. }));
    }
}
