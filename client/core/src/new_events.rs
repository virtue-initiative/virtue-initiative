use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::mpsc::{self, Receiver, Sender},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::error::{CoreError, CoreResult};
use crate::events::log_error;

/// Opaque, serializable blob used to persist and restore observer state.
type StateType = serde_json::Value;

/// A type-erased event as it travels through the in-process channel.
type AnyEvent = Box<dyn Any + Send + Sync>;

/// Anything that can be published on the bus.
///
/// The blanket impl means any `Serialize + DeserializeOwned` value that is
/// `Send + Sync + 'static` is automatically an `Event` — no derive needed.
pub trait Event: Serialize + DeserializeOwned + Send + Sync + 'static {}

impl<T> Event for T where T: Serialize + DeserializeOwned + Send + Sync + 'static {}

/// Emitted by the bus when a subscription handler returns `Err`.
///
/// Handlers are fallible (`Fn(&E) -> CoreResult<()>`); when one fails the bus
/// logs it and publishes this event so the failure can propagate — e.g. the IPC
/// bridge can forward it to a controller/UI. A failing handler **for `Error`
/// itself** is only logged, never re-emitted, so errors can't loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    /// The event type whose handler failed (`std::any::type_name`).
    pub source: String,
    pub message: String,
}

/// A stateful module that reacts to events.
///
/// During [`EventBus::new`] each observer's [`Observer::init`] is called with a
/// mutable handle to the bus (to register subscriptions and grab an
/// [`Emitter`]) and the state it previously returned from [`Observer::save`]
/// (or [`StateType::Null`] on a fresh start).
pub trait Observer {
    /// Register subscriptions and restore any previously-saved `state`.
    fn init(&mut self, bus: &mut EventBus, state: StateType) -> CoreResult<()>;
    /// Snapshot this observer's durable state so it can be restored later.
    fn save(&self) -> CoreResult<StateType>;
    /// Stable, unique key used to namespace this observer's saved state.
    fn name(&self) -> &'static str;
}

/// A boxed, type-erased subscription callback. The bus only ever invokes a
/// handler with an event whose concrete type matches the `TypeId` it was
/// registered under, so the inner downcast always succeeds.
type Handler = Box<dyn Fn(&dyn Any) + Send + Sync>;

/// A cheap, cloneable handle for publishing events.
///
/// Handlers must be `'static`, so they cannot borrow the [`EventBus`]. Instead
/// an observer grabs an `Emitter` in [`Observer::init`] and moves it into its
/// subscription closures to emit follow-up events.
#[derive(Clone)]
pub struct Emitter {
    tx: Sender<AnyEvent>,
}

impl Emitter {
    /// Queue `event` for delivery on the next drain pass.
    pub fn send<E: Event>(&self, event: E) -> CoreResult<()> {
        self.tx
            .send(Box::new(event))
            .map_err(|_| CoreError::InvalidState("event bus channel closed"))
    }
}

/// This is the main event loop.
///
/// The API looks like this:
/// ```ignore
/// #[derive(Serialize, Deserialize, Debug)]
/// struct MyEvent {
///     data: String,
/// }
///
/// #[derive(Serialize, Deserialize, Debug)]
/// struct MyOtherEvent {
///     data: String,
/// }
///
/// struct MyObserver {}
///
/// impl Observer for MyObserver {
///     fn init(&mut self, bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
///         // Grab an emitter so the handler can publish follow-up events.
///         let bus = bus.emitter();
///         bus.subscribe(move |event: &MyEvent| {
///             println!("Received MyEvent: {:?}", event);
///             bus.send(MyOtherEvent { data: "Hello".to_string() }).unwrap();
///         });
///         Ok(())
///     }
///     fn save(&self) -> CoreResult<StateType> {
///         Ok(serde_json::Value::Null)
///     }
///     fn name(&self) -> &'static str {
///         "my_observer"
///     }
/// }
///
/// let mut bus = EventBus::new(vec![Box::new(MyObserver {})], serde_json::Value::Null)?;
/// loop {
///     // Drains every pending event (and any they cascade into), then
///     // returns the merged state of all observers.
///     let state = bus.iter()?;
/// }
/// ```
pub struct EventBus {
    observers: Vec<Box<dyn Observer>>,
    handlers: HashMap<TypeId, Vec<Handler>>,
    tx: Sender<AnyEvent>,
    rx: Receiver<AnyEvent>,
}

impl EventBus {
    /// Build a bus, restoring each observer from `state`.
    ///
    /// `state` is the object previously produced by [`EventBus::iter`]/[`save`]:
    /// a JSON object keyed by [`Observer::name`]. Any observer missing from the
    /// object (e.g. on a fresh start, pass [`StateType::Null`]) is initialized
    /// with [`StateType::Null`].
    ///
    /// [`save`]: EventBus::save
    pub fn new(observers: Vec<Box<dyn Observer>>, state: StateType) -> CoreResult<Self> {
        let (tx, rx) = mpsc::channel();
        let mut bus = Self {
            observers: Vec::with_capacity(observers.len()),
            handlers: HashMap::new(),
            tx,
            rx,
        };

        // Initialize each observer against the (initially empty) bus. We iterate
        // the owned `observers` vec and push into `bus.observers` afterwards, so
        // there's no aliasing while `init` borrows the bus mutably.
        for mut observer in observers {
            let observer_state = state
                .get(observer.name())
                .cloned()
                .unwrap_or(StateType::Null);
            observer.init(&mut bus, observer_state)?;
            bus.observers.push(observer);
        }

        Ok(bus)
    }

    /// Register `f` to be called for every published event of type `E`.
    ///
    /// `f` is fallible: if it returns `Err`, the bus logs the error and publishes
    /// an [`Error`] event (so it can propagate, e.g. out to a controller). A
    /// failing handler for the `Error` event itself is only logged, never
    /// re-emitted, to avoid an error loop.
    pub fn subscribe<E: Event>(
        &mut self,
        f: impl Fn(&E) -> CoreResult<()> + Send + Sync + 'static,
    ) {
        let tx = self.tx.clone();
        let is_error_event = TypeId::of::<E>() == TypeId::of::<Error>();
        self.handlers
            .entry(TypeId::of::<E>())
            .or_default()
            .push(Box::new(move |event: &dyn Any| {
                // The bus only dispatches an event to handlers registered under
                // its own `TypeId`, so this downcast is infallible.
                let Some(event) = event.downcast_ref::<E>() else {
                    return;
                };
                if let Err(err) = f(event) {
                    log_error(
                        &format!("handler for {} failed", std::any::type_name::<E>()),
                        Some(&err),
                    );
                    if !is_error_event {
                        let _ = tx.send(Box::new(Error {
                            source: std::any::type_name::<E>().to_string(),
                            message: err.to_string(),
                        }));
                    }
                }
            }));
    }

    /// A cloneable handle for publishing events from within handlers.
    pub fn emitter(&self) -> Emitter {
        Emitter {
            tx: self.tx.clone(),
        }
    }

    /// Publish an event onto the queue. It is delivered on the next [`iter`].
    ///
    /// [`iter`]: EventBus::iter
    pub fn send<E: Event>(&self, event: E) -> CoreResult<()> {
        self.tx
            .send(Box::new(event))
            .map_err(|_| CoreError::InvalidState("event bus channel closed"))
    }

    /// Run one full processing pass.
    ///
    /// Drains every queued event, dispatching each to its subscribers. Because
    /// handlers publish through a cloned [`Sender`], events they emit land back
    /// on the same queue and are processed within this same call — so a single
    /// `iter` settles the entire cascade. Once the queue is empty, the state of
    /// every observer is collected and returned (see [`save`]).
    ///
    /// [`save`]: EventBus::save
    pub fn iter(&mut self) -> CoreResult<StateType> {
        while let Ok(event) = self.rx.try_recv() {
            self.dispatch(&event);
        }
        self.save()
    }

    /// Deliver a single event to every handler registered for its concrete type.
    fn dispatch(&self, event: &AnyEvent) {
        let event: &dyn Any = &**event;
        if let Some(handlers) = self.handlers.get(&event.type_id()) {
            for handler in handlers {
                handler(event);
            }
        }
    }

    /// Collect every observer's durable state into a JSON object keyed by name.
    pub fn save(&self) -> CoreResult<StateType> {
        let mut state = serde_json::Map::with_capacity(self.observers.len());
        for observer in &self.observers {
            state.insert(observer.name().to_string(), observer.save()?);
        }
        Ok(StateType::Object(state))
    }
}

/// A channel on which typed events can be published and observed.
///
/// Implemented by both the in-process [`EventBus`] and the cross-process
/// [`RemoteEventBus`]. This lets request/response helpers — most importantly
/// `ClientController` — be written **once** against `EventChannel` and work
/// regardless of whether the peer lives in this process or on the other end of
/// a socket, with no `if local { … } else { … }` branching at the call sites.
///
/// The only difference between the two implementations is captured by
/// [`pump`](EventChannel::pump): an in-process bus has to be driven explicitly
/// to process the queue, whereas a remote bus is driven by its own background
/// reader thread.
pub trait EventChannel {
    /// Publish `event` to the channel; does not wait for a reply.
    fn publish<E: Event>(&self, event: E) -> CoreResult<()>;

    /// Register `handler` to run for every observed event of type `E`.
    /// `handler` is fallible; see [`EventBus::subscribe`] for how errors surface.
    fn on<E: Event>(&mut self, handler: impl Fn(&E) -> CoreResult<()> + Send + Sync + 'static);

    /// Drive any pending in-process work. An [`EventBus`] runs one drain pass; a
    /// [`RemoteEventBus`] (whose reader thread does the work) is a no-op.
    fn pump(&mut self) -> CoreResult<()>;

    /// Publish `request` and block until a matching `Resp` is observed, then
    /// return it.
    ///
    /// Works uniformly across transports: an [`EventBus`] settles the response
    /// synchronously during [`pump`](EventChannel::pump); a [`RemoteEventBus`]
    /// receives it on its reader thread. Blocks until a response arrives,
    /// mirroring the previous controller's blocking semantics.
    fn request<Req: Event, Resp: Event + Clone>(&mut self, request: Req) -> CoreResult<Resp> {
        let (tx, rx) = mpsc::channel();
        self.on::<Resp>(move |resp| {
            // Ignore send errors: a prior, now-dropped request of the same type
            // may have left a stale handler registered.
            let _ = tx.send(resp.clone());
            Ok(())
        });
        self.publish(request)?;
        self.pump()?;
        rx.recv()
            .map_err(|_| CoreError::InvalidState("event channel closed before response"))
    }
}

impl EventChannel for EventBus {
    fn publish<E: Event>(&self, event: E) -> CoreResult<()> {
        self.send(event)
    }

    fn on<E: Event>(&mut self, handler: impl Fn(&E) -> CoreResult<()> + Send + Sync + 'static) {
        self.subscribe(handler);
    }

    fn pump(&mut self) -> CoreResult<()> {
        self.iter().map(|_state| ())
    }
}

/// This is used for sending events across IPC boundaries.
/// Other processes cannot have state or observers so this
/// is just for sending and receiving events directly.
///
/// This only supports macos and linux (other OSes aren't multi-process).
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod ipc {
    use super::*;
    use serde::Deserialize;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::thread;

    /// A subscription callback on the remote side: it deserializes the JSON
    /// payload into the concrete event type and invokes the user closure.
    type RemoteHandler = Box<dyn Fn(&serde_json::Value) + Send + Sync>;

    /// Line-delimited wire envelope. `kind` is `std::any::type_name::<E>()`,
    /// which is stable here because both ends are the same binary.
    #[derive(Serialize, Deserialize)]
    struct Envelope {
        kind: String,
        data: serde_json::Value,
    }

    /// A stateless event bus that talks to an [`EventBus`] in another process
    /// over a Unix domain socket. It has no observers and persists nothing — it
    /// only marshals typed events on and off the socket.
    pub struct RemoteEventBus {
        writer: UnixStream,
        handlers: Arc<Mutex<HashMap<&'static str, Vec<RemoteHandler>>>>,
        // Owns the background reader thread; named with a leading underscore so
        // it's kept alive for the lifetime of the bus without a dead-code warning.
        _reader: thread::JoinHandle<()>,
    }

    impl RemoteEventBus {
        /// Connect to the daemon listening on `socket` and start decoding the
        /// inbound event stream on a background thread.
        pub fn new(socket: &Path) -> CoreResult<Self> {
            let writer = UnixStream::connect(socket)?;
            let reader_stream = writer.try_clone()?;

            let handlers: Arc<Mutex<HashMap<&'static str, Vec<RemoteHandler>>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let reader_handlers = Arc::clone(&handlers);

            let _reader = thread::spawn(move || {
                let reader = BufReader::new(reader_stream);
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    if line.is_empty() {
                        continue;
                    }
                    let Ok(envelope) = serde_json::from_str::<Envelope>(&line) else {
                        continue;
                    };
                    let guard = reader_handlers.lock().unwrap();
                    if let Some(handlers) = guard.get(envelope.kind.as_str()) {
                        for handler in handlers {
                            handler(&envelope.data);
                        }
                    }
                }
            });

            Ok(Self {
                writer,
                handlers,
                _reader,
            })
        }

        /// Serialize `event` and write it to the socket as one JSON line.
        pub fn send<E: Event>(&self, event: E) -> CoreResult<()> {
            let envelope = Envelope {
                kind: std::any::type_name::<E>().to_string(),
                data: serde_json::to_value(&event)?,
            };
            let mut line = serde_json::to_string(&envelope)?;
            line.push('\n');
            // `&UnixStream: Write`, so this only needs `&self`.
            (&self.writer)
                .write_all(line.as_bytes())
                .map_err(CoreError::from)
        }

        /// Register `f` to run for every inbound event of type `E`.
        ///
        /// `f` is fallible; an `Err` is logged (there is no local module graph to
        /// emit an `Error` event into, unlike [`EventBus::subscribe`]).
        pub fn subscribe<E: Event>(
            &mut self,
            f: impl Fn(&E) -> CoreResult<()> + Send + Sync + 'static,
        ) {
            let handler: RemoteHandler = Box::new(move |data: &serde_json::Value| {
                if let Ok(event) = serde_json::from_value::<E>(data.clone())
                    && let Err(err) = f(&event)
                {
                    log_error(
                        &format!("remote handler for {} failed", std::any::type_name::<E>()),
                        Some(&err),
                    );
                }
            });
            self.handlers
                .lock()
                .unwrap()
                .entry(std::any::type_name::<E>())
                .or_default()
                .push(handler);
        }
    }

    impl EventChannel for RemoteEventBus {
        fn publish<E: Event>(&self, event: E) -> CoreResult<()> {
            self.send(event)
        }

        fn on<E: Event>(&mut self, handler: impl Fn(&E) -> CoreResult<()> + Send + Sync + 'static) {
            self.subscribe(handler);
        }

        // Inbound events are dispatched by the background reader thread, so there
        // is nothing to drive synchronously.
        fn pump(&mut self) -> CoreResult<()> {
            Ok(())
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use ipc::RemoteEventBus;

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::sync::{Arc, Mutex};

    #[derive(Serialize, Deserialize, Clone)]
    struct Tick {
        n: u32,
    }

    #[derive(Serialize, Deserialize, Clone)]
    struct Tock {
        n: u32,
    }

    /// Emits a `Tock` for every `Tick`, and counts how many `Tock`s it sees so
    /// we can assert that cascaded events are processed within a single `iter`.
    struct Counter {
        seen: Arc<Mutex<u32>>,
    }

    impl Observer for Counter {
        fn init(&mut self, bus: &mut EventBus, state: StateType) -> CoreResult<()> {
            // Restore the running count from saved state.
            if let Some(prev) = state.as_u64() {
                *self.seen.lock().unwrap() = prev as u32;
            }

            let emitter = bus.emitter();
            bus.subscribe(move |tick: &Tick| emitter.send(Tock { n: tick.n }));

            let seen = Arc::clone(&self.seen);
            bus.subscribe(move |_tock: &Tock| {
                *seen.lock().unwrap() += 1;
                Ok(())
            });

            Ok(())
        }

        fn save(&self) -> CoreResult<StateType> {
            Ok(StateType::from(*self.seen.lock().unwrap()))
        }

        fn name(&self) -> &'static str {
            "counter"
        }
    }

    #[test]
    fn cascades_within_single_iter_and_saves_state() {
        let seen = Arc::new(Mutex::new(0));
        let mut bus = EventBus::new(
            vec![Box::new(Counter {
                seen: Arc::clone(&seen),
            })],
            StateType::Null,
        )
        .unwrap();

        bus.send(Tick { n: 1 }).unwrap();
        bus.send(Tick { n: 2 }).unwrap();
        bus.send(Tick { n: 3 }).unwrap();

        // A single iter drains the three Ticks *and* the three Tocks they cascade.
        let state = bus.iter().unwrap();

        assert_eq!(*seen.lock().unwrap(), 3);
        assert_eq!(state["counter"].as_u64(), Some(3));
    }

    #[test]
    fn request_returns_response_over_event_channel() {
        // `Counter` emits a `Tock` for every `Tick`, so a request/response round
        // trip through the generic `EventChannel::request` should hand back the
        // cascaded `Tock`. This is the same code path `ClientController` uses.
        let seen = Arc::new(Mutex::new(0));
        let mut bus = EventBus::new(
            vec![Box::new(Counter {
                seen: Arc::clone(&seen),
            })],
            StateType::Null,
        )
        .unwrap();

        let reply: Tock = bus.request(Tick { n: 9 }).unwrap();
        assert_eq!(reply.n, 9);
    }

    #[test]
    fn restores_observer_state() {
        let saved = StateType::Object(
            [("counter".to_string(), StateType::from(7u32))]
                .into_iter()
                .collect(),
        );

        let seen = Arc::new(Mutex::new(0));
        let _bus = EventBus::new(
            vec![Box::new(Counter {
                seen: Arc::clone(&seen),
            })],
            saved,
        )
        .unwrap();

        assert_eq!(*seen.lock().unwrap(), 7);
    }

    /// A handler that fails on `Tick`, plus an observer that captures `Error`
    /// events, proving the bus turns a failed handler into an `Error` event.
    #[test]
    fn failing_handler_emits_error_event() {
        struct Failing;
        impl Observer for Failing {
            fn init(&mut self, bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
                bus.subscribe(|_tick: &Tick| Err(CoreError::InvalidState("boom")));
                Ok(())
            }
            fn save(&self) -> CoreResult<StateType> {
                Ok(StateType::Null)
            }
            fn name(&self) -> &'static str {
                "failing"
            }
        }

        struct Capture {
            errors: Arc<Mutex<Vec<Error>>>,
        }
        impl Observer for Capture {
            fn init(&mut self, bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
                let errors = Arc::clone(&self.errors);
                bus.subscribe(move |err: &Error| {
                    errors.lock().unwrap().push(err.clone());
                    Ok(())
                });
                Ok(())
            }
            fn save(&self) -> CoreResult<StateType> {
                Ok(StateType::Null)
            }
            fn name(&self) -> &'static str {
                "capture"
            }
        }

        let errors = Arc::new(Mutex::new(Vec::new()));
        let mut bus = EventBus::new(
            vec![
                Box::new(Failing),
                Box::new(Capture {
                    errors: Arc::clone(&errors),
                }),
            ],
            StateType::Null,
        )
        .unwrap();

        bus.send(Tick { n: 1 }).unwrap();
        // The failing Tick handler emits an Error, which is drained and captured
        // in the same pass.
        bus.iter().unwrap();

        let errors = errors.lock().unwrap();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("boom"));
        assert!(errors[0].source.contains("Tick"));
    }
}
