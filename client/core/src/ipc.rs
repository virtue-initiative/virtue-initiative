use std::fmt;

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

// ── Unix domain socket transport (Linux / macOS) ──────────────────────────────

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_impl {
    use std::io::{BufRead, BufReader, BufWriter, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;

    use super::IpcError;

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
        pub fn send_line(&mut self, line: &str) -> Result<(), IpcError> {
            self.writer.write_all(line.as_bytes())?;
            if !line.ends_with('\n') {
                self.writer.write_all(b"\n")?;
            }
            self.writer.flush()?;
            Ok(())
        }
    }

    impl IpcReceiver {
        pub fn recv_line(&mut self) -> Result<String, IpcError> {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line)?;
            if n == 0 {
                return Err(IpcError::Disconnected);
            }
            Ok(line.trim_end_matches('\n').to_string())
        }

        pub fn try_recv_line(&mut self) -> Result<Option<String>, IpcError> {
            if self.reader.buffer().contains(&b'\n') {
                return self.recv_line().map(Some);
            }
            self.reader.get_ref().set_nonblocking(true)?;
            let result = self.recv_line();
            let _ = self.reader.get_ref().set_nonblocking(false);
            match result {
                Ok(line) => Ok(Some(line)),
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

    use super::IpcError;

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
        pub fn send_line(&mut self, line: &str) -> Result<(), IpcError> {
            self.tx
                .send(line.trim_end_matches('\n').to_string())
                .map_err(|_| IpcError::Disconnected)
        }
    }

    impl IpcReceiver {
        pub fn recv_line(&mut self) -> Result<String, IpcError> {
            self.rx.recv().map_err(|_| IpcError::Disconnected)
        }

        pub fn try_recv_line(&mut self) -> Result<Option<String>, IpcError> {
            match self.rx.try_recv() {
                Ok(line) => Ok(Some(line)),
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

    pub fn register_connect_tx(tx: mpsc::SyncSender<(IpcSender, IpcReceiver)>) {
        CONNECT_TX.set(tx).ok();
    }

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

        daemon_sender.send_line("hello world").expect("send");

        let received = ctrl_receiver.recv_line().expect("recv");
        assert_eq!(received, "hello world");

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
        let (mut daemon_sender, _daemon_receiver) = listener.blocking_accept().expect("accept");

        daemon_sender.send_line("test message").expect("send");
        let received = ctrl_receiver.recv_line().expect("recv");
        assert_eq!(received, "test message");

        ctrl_sender.send_line("request").expect("send request");
        let req = _daemon_receiver.recv_line().expect("recv request");
        assert_eq!(req, "request");
    }
}
