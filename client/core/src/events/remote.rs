use std::any::type_name;
use std::collections::HashMap;
use std::fmt;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::logging::log_error;

use super::channel::{Event, EventChannel};

// ── IPC error ─────────────────────────────────────────────────────────────────

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

// ── Unix domain socket transport (private) ────────────────────────────────────

struct IpcSender {
    writer: BufWriter<UnixStream>,
}

struct IpcReceiver {
    reader: BufReader<UnixStream>,
}

impl IpcSender {
    fn send_line(&mut self, line: &str) -> Result<(), IpcError> {
        self.writer.write_all(line.as_bytes())?;
        if !line.ends_with('\n') {
            self.writer.write_all(b"\n")?;
        }
        self.writer.flush()?;
        Ok(())
    }
}

impl IpcReceiver {
    fn recv_line(&mut self) -> Result<String, IpcError> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;
        if n == 0 {
            return Err(IpcError::Disconnected);
        }
        Ok(line.trim_end_matches('\n').to_string())
    }
}

/// Shuts the socket down when the last holder (the owning bus and any of its
/// senders) drops. `shutdown` acts on the whole socket regardless of how many
/// dup'd fds (from `try_clone`) reference it, so this reliably unblocks the
/// reader thread and makes the peer see EOF — closing `close()` alone cannot
/// guarantee with cloned fds.
struct ConnectionGuard {
    stream: UnixStream,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

fn make_pair(
    stream: UnixStream,
) -> Result<(IpcSender, IpcReceiver, Arc<ConnectionGuard>), IpcError> {
    let read_stream = stream.try_clone()?;
    let guard_stream = stream.try_clone()?;
    Ok((
        IpcSender {
            writer: BufWriter::new(stream),
        },
        IpcReceiver {
            reader: BufReader::new(read_stream),
        },
        Arc::new(ConnectionGuard {
            stream: guard_stream,
        }),
    ))
}

// ── IpcListener ───────────────────────────────────────────────────────────────

pub struct IpcListener {
    listener: UnixListener,
}

impl IpcListener {
    pub fn bind(path: &Path) -> Result<Self, IpcError> {
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path)?;
        Ok(Self { listener })
    }

    pub fn blocking_accept(&self) -> Result<RemoteEventBus, IpcError> {
        let (stream, _) = self.listener.accept()?;
        stream.set_nonblocking(false)?;
        let (sender, receiver, guard) = make_pair(stream)?;
        Ok(RemoteEventBus::from_pair(sender, receiver, guard))
    }
}

// ── RemoteEventBus ────────────────────────────────────────────────────────────

type RemoteHandler = Box<dyn Fn(&serde_json::Value) + Send + Sync>;

#[derive(Serialize, Deserialize)]
struct Envelope {
    kind: String,
    data: serde_json::Value,
}

/// A cheap cloneable send-only handle to a [`RemoteEventBus`].
///
/// Analogous to [`Emitter`] for the in-process bus — observers on the daemon
/// side capture a `RemoteSender` clone and use it to push events out to a
/// connected controller.
///
/// [`Emitter`]: super::bus::Emitter
#[derive(Clone)]
pub struct RemoteSender {
    writer: Arc<Mutex<IpcSender>>,
    // Cleared by the owning bus's reader thread when the peer disconnects, so
    // holders (e.g. the daemon's client list) can drop dead senders promptly
    // instead of leaking the socket fd until a write happens to fail.
    connected: Arc<AtomicBool>,
    // Shuts the socket down once this (and the owning bus) is dropped. The
    // daemon keeps a connection alive via its sender after dropping the bus, so
    // the guard must live on the sender too.
    _guard: Arc<ConnectionGuard>,
}

impl RemoteSender {
    /// Whether the peer is still connected (reader thread still running).
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn send<E: Event>(&self, event: E) -> CoreResult<()> {
        let envelope = Envelope {
            kind: type_name::<E>().to_string(),
            data: serde_json::to_value(&event)?,
        };
        let line = serde_json::to_string(&envelope)?;
        self.writer
            .lock()
            .unwrap()
            .send_line(&line)
            .map_err(CoreError::from)
    }
}

/// A stateless cross-process event bus backed by a Unix socket connection.
///
/// Inbound events are decoded on a background reader thread and dispatched to
/// registered handlers. There are no in-process observers or persisted state —
/// this only marshals typed events on and off the wire.
///
/// Implements [`EventChannel`] so it is interchangeable with an in-process
/// [`EventBus`] in `ClientController` and similar generic callers.
///
/// # Reader thread lifecycle
///
/// The reader thread is not started until [`start`] is called. On the server
/// (daemon) side this lets you register all inbound handlers before any
/// messages can be dispatched, eliminating the race where an early message
/// arrives before handlers are wired up. [`connect`] calls [`start`]
/// automatically so client callers need not worry about it.
///
/// [`start`]: RemoteEventBus::start
/// [`connect`]: RemoteEventBus::connect
/// [`EventBus`]: super::bus::EventBus
pub struct RemoteEventBus {
    writer: Arc<Mutex<IpcSender>>,
    handlers: Arc<Mutex<HashMap<String, Vec<RemoteHandler>>>>,
    // Receiver stored here until start() is called; None afterwards.
    pending_receiver: Option<IpcReceiver>,
    // Kept alive so the reader thread runs for the lifetime of this bus.
    _reader: Option<thread::JoinHandle<()>>,
    // True while the peer is connected; the reader thread clears it on EOF so
    // outstanding `RemoteSender`s can be pruned.
    connected: Arc<AtomicBool>,
    // Shuts the socket down when the last holder drops, terminating the reader
    // thread and signalling the peer. Cloned into senders via `sender()`.
    guard: Arc<ConnectionGuard>,
}

impl RemoteEventBus {
    fn from_pair(sender: IpcSender, receiver: IpcReceiver, guard: Arc<ConnectionGuard>) -> Self {
        let writer = Arc::new(Mutex::new(sender));
        let handlers: Arc<Mutex<HashMap<String, Vec<RemoteHandler>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        Self {
            writer,
            handlers,
            pending_receiver: Some(receiver),
            _reader: None,
            connected: Arc::new(AtomicBool::new(true)),
            guard,
        }
    }

    /// Whether the peer is still connected (reader thread still running).
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Start the background reader thread.
    ///
    /// Must be called after all inbound handlers have been registered with
    /// [`on`] / [`subscribe`]. Calling it a second time is a no-op.
    ///
    /// [`on`]: RemoteEventBus::subscribe
    /// [`subscribe`]: RemoteEventBus::subscribe
    pub fn start(&mut self) {
        let Some(mut receiver) = self.pending_receiver.take() else {
            return;
        };
        let reader_handlers = Arc::clone(&self.handlers);
        let connected = Arc::clone(&self.connected);
        self._reader = Some(thread::spawn(move || {
            while let Ok(line) = receiver.recv_line() {
                if line.is_empty() {
                    continue;
                }
                let Ok(envelope) = serde_json::from_str::<Envelope>(&line) else {
                    continue;
                };
                let guard = reader_handlers.lock().unwrap();
                if let Some(hs) = guard.get(&envelope.kind) {
                    for h in hs {
                        h(&envelope.data);
                    }
                }
            }
            // recv_line returned Err: the peer disconnected. Mark this
            // connection dead so holders of its sender can prune it.
            connected.store(false, Ordering::Relaxed);
        }));
    }

    /// Connect to a daemon at `path` and return a bus for that connection.
    pub fn connect(path: &Path) -> Result<Self, IpcError> {
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
        let (sender, receiver, guard) = make_pair(stream)?;
        let mut bus = Self::from_pair(sender, receiver, guard);
        bus.start();
        Ok(bus)
    }

    /// Serialize `event` and write it as one JSON line.
    pub fn send<E: Event>(&self, event: E) -> CoreResult<()> {
        let envelope = Envelope {
            kind: type_name::<E>().to_string(),
            data: serde_json::to_value(&event)?,
        };
        let line = serde_json::to_string(&envelope)?;
        self.writer
            .lock()
            .unwrap()
            .send_line(&line)
            .map_err(CoreError::from)
    }

    /// Register `f` to run for every inbound event of type `E`.
    pub fn subscribe<E: Event>(
        &mut self,
        f: impl Fn(&E) -> CoreResult<()> + Send + Sync + 'static,
    ) {
        let handler: RemoteHandler = Box::new(move |data: &serde_json::Value| {
            if let Ok(event) = serde_json::from_value::<E>(data.clone())
                && let Err(err) = f(&event)
            {
                log_error(
                    &format!("remote handler for {} failed", type_name::<E>()),
                    Some(&err),
                );
            }
        });
        self.handlers
            .lock()
            .unwrap()
            .entry(type_name::<E>().to_string())
            .or_default()
            .push(handler);
    }

    /// Return a cheap cloneable send-only handle.
    pub fn sender(&self) -> RemoteSender {
        RemoteSender {
            writer: Arc::clone(&self.writer),
            connected: Arc::clone(&self.connected),
            _guard: Arc::clone(&self.guard),
        }
    }
}

impl EventChannel for RemoteEventBus {
    fn publish<E: Event>(&self, event: E) -> CoreResult<()> {
        self.send(event)
    }

    fn on<E: Event>(&mut self, handler: impl Fn(&E) -> CoreResult<()> + Send + Sync + 'static) {
        self.subscribe(handler);
    }

    // Inbound events are driven by the background reader thread — nothing to pump.
    fn pump(&mut self) -> CoreResult<()> {
        Ok(())
    }

    fn request<Req: crate::events::Event, Resp: crate::events::Event + Clone>(
        &mut self,
        request: Req,
    ) -> CoreResult<Resp> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.on::<Resp>(move |resp| {
            let _ = tx.send(resp.clone());
            Ok(())
        });
        self.publish(request)?;
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|_| CoreError::InvalidState("daemon did not respond within 10 seconds"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Clone, Debug)]
    struct Ping {
        val: u32,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    struct Pong {
        val: u32,
    }

    fn make_bus_pair() -> (RemoteEventBus, RemoteEventBus) {
        // A pid+timestamp nonce isn't unique enough on its own: several tests in
        // this module call make_bus_pair() and the default test runner executes
        // them in parallel threads within the same process, so two calls landing
        // in the same clock tick (observed on macOS CI) previously collided on
        // the same socket path. The atomic counter guarantees each call gets a
        // distinct path regardless of clock resolution.
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        let sock = std::env::temp_dir().join(format!(
            "virtue-remote-test-{}-{}-{}.sock",
            std::process::id(),
            nonce,
            count
        ));
        let listener = IpcListener::bind(&sock).expect("bind");

        let sock2 = sock.clone();
        let client_handle =
            thread::spawn(move || RemoteEventBus::connect(&sock2).expect("connect"));

        let mut daemon_bus = listener.blocking_accept().expect("accept");
        let client_bus = client_handle.join().expect("connect thread");

        let _ = std::fs::remove_file(sock);

        // In tests handlers are registered after make_bus_pair returns, but
        // no messages are in flight yet, so starting the reader here is safe.
        daemon_bus.start();

        (daemon_bus, client_bus)
    }

    #[test]
    fn round_trip_typed_event() {
        let (bus_a, mut bus_b) = make_bus_pair();

        let (tx, rx) = std::sync::mpsc::channel();
        bus_b.subscribe(move |msg: &Ping| {
            tx.send(msg.val).ok();
            Ok(())
        });

        bus_a.send(Ping { val: 42 }).expect("send");

        let received = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("timed out waiting for message");
        assert_eq!(received, 42);
    }

    #[test]
    fn request_response_round_trip() {
        let (mut server_bus, mut client_bus) = make_bus_pair();

        // Server echoes Ping as Pong.
        let sender = server_bus.sender();
        server_bus.subscribe(move |ping: &Ping| sender.send(Pong { val: ping.val }));

        // Client uses EventChannel::request to do the round trip.
        let pong: Pong = client_bus.request(Ping { val: 7 }).expect("request");
        assert_eq!(pong.val, 7);
    }

    #[test]
    fn unix_socket_send_recv_round_trips() {
        let sock =
            std::env::temp_dir().join(format!("virtue-ipc-test-{}.sock", std::process::id()));
        let listener = IpcListener::bind(&sock).expect("bind");

        let sock2 = sock.clone();
        let client_handle =
            thread::spawn(move || RemoteEventBus::connect(&sock2).expect("connect"));

        let daemon_bus = listener.blocking_accept().expect("accept");
        let mut client_bus = client_handle.join().expect("connect thread");

        let (tx, rx) = std::sync::mpsc::channel();
        client_bus.subscribe(move |msg: &Ping| {
            tx.send(msg.val).ok();
            Ok(())
        });

        daemon_bus.send(Ping { val: 99 }).expect("send");

        let received = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("timed out");
        assert_eq!(received, 99);

        let _ = std::fs::remove_file(&sock);
    }

    #[test]
    fn sender_reports_disconnected_after_client_drops() {
        let (daemon_bus, client_bus) = make_bus_pair();
        let sender = daemon_bus.sender();
        assert!(sender.is_connected(), "should start connected");

        // Client goes away; the daemon's reader thread should observe EOF and
        // clear the connected flag so the sender can be pruned.
        drop(client_bus);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while sender.is_connected() && std::time::Instant::now() < deadline {
            thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            !sender.is_connected(),
            "sender should report disconnected after the client drops"
        );
    }
}
