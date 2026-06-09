use std::any::type_name;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::ipc::{IpcReceiver, IpcSender};

use super::bus::{Event, EventChannel, log_error};

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
}

impl RemoteSender {
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

/// A stateless cross-process event bus backed by an [`IpcSender`]/[`IpcReceiver`] pair.
///
/// Inbound events are decoded on a background reader thread and dispatched to
/// registered handlers. There are no in-process observers or persisted state —
/// this only marshals typed events on and off the wire.
///
/// Implements [`EventChannel`] so it is interchangeable with an in-process
/// [`EventBus`] in `ClientController` and similar generic callers.
///
/// [`EventBus`]: super::bus::EventBus
pub struct RemoteEventBus {
    writer: Arc<Mutex<IpcSender>>,
    handlers: Arc<Mutex<HashMap<String, Vec<RemoteHandler>>>>,
    // Kept alive so the reader thread runs for the lifetime of this bus.
    _reader: thread::JoinHandle<()>,
}

impl RemoteEventBus {
    /// Create a bus and start the background reader thread.
    pub fn new(sender: IpcSender, mut receiver: IpcReceiver) -> Self {
        let writer = Arc::new(Mutex::new(sender));
        let handlers: Arc<Mutex<HashMap<String, Vec<RemoteHandler>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let reader_handlers = Arc::clone(&handlers);

        let _reader = thread::spawn(move || {
            loop {
                match receiver.recv_line() {
                    Ok(line) => {
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
                    Err(_) => break,
                }
            }
        });

        Self {
            writer,
            handlers,
            _reader,
        }
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
            if let Ok(event) = serde_json::from_value::<E>(data.clone()) {
                if let Err(err) = f(&event) {
                    log_error(
                        &format!("remote handler for {} failed", type_name::<E>()),
                        Some(&err),
                    );
                }
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
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::time::{SystemTime, UNIX_EPOCH};
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .subsec_nanos();
            let sock = std::env::temp_dir().join(format!(
                "virtue-remote-test-{}-{}.sock",
                std::process::id(),
                nonce
            ));
            let listener = crate::ipc::IpcListener::bind(&sock).expect("bind");

            let sock2 = sock.clone();
            let client_handle =
                thread::spawn(move || crate::ipc::connect(&sock2).expect("connect"));

            let (d_sender, d_receiver) = listener.blocking_accept().expect("accept");
            let (c_sender, c_receiver) = client_handle.join().expect("connect thread");

            let _ = std::fs::remove_file(sock);

            (
                RemoteEventBus::new(d_sender, d_receiver),
                RemoteEventBus::new(c_sender, c_receiver),
            )
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            use std::path::Path;
            let listener = crate::ipc::IpcListener::bind(Path::new("/ignored")).expect("bind");
            let (c_sender, c_receiver) =
                crate::ipc::connect_in_process(&listener.connect_tx).expect("connect");
            let (d_sender, d_receiver) = listener.blocking_accept().expect("accept");
            (
                RemoteEventBus::new(d_sender, d_receiver),
                RemoteEventBus::new(c_sender, c_receiver),
            )
        }
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
}
