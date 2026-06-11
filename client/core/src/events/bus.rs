use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::mpsc::{self, Receiver, Sender},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::error::{CoreError, CoreResult};

/// Private trait that bundles `Any + Debug` so the channel can hold type-erased
/// events that are still printable in debug builds.
trait AnyDebugEvent: Any + std::fmt::Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
}
impl<T: Any + std::fmt::Debug + Send + Sync> AnyDebugEvent for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Match a `&dyn Any` event against typed arms, like a `match` statement.
///
/// Each arm has the form `$pat: $Type => $expr`. The pattern `_` discards the
/// typed reference; any identifier binds it for use in the expression body.
/// Falls through to `Ok(())` when no arm matches.
///
/// ```rust,ignore
/// fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
///     dispatch_event!(event, {
///         _: Ping          => self.handle_ping(emitter),
///         ev: ProcessStopped => self.handle_stopped(&ev.0, emitter),
///     })
/// }
/// ```
#[macro_export]
macro_rules! dispatch_event {
    ($event:expr, { $($binding:tt : $ty:ty => $body:expr),* $(,)? }) => {{
        $(
            if let Some($binding) = ($event as &dyn ::std::any::Any).downcast_ref::<$ty>() {
                return $body;
            }
        )*
        Ok(())
    }};
}

pub fn log_error(msg: &str, err: Option<&dyn std::fmt::Display>) {
    match err {
        Some(e) => eprintln!("[core error] {msg}: {e}"),
        None => eprintln!("[core error] {msg}"),
    }
}

/// Opaque, serializable blob used to persist and restore observer state.
pub type StateType = serde_json::Value;

/// A type-erased event as it travels through the in-process channel.
type AnyEvent = Box<dyn AnyDebugEvent>;

/// Anything that can be published on the bus.
///
/// The blanket impl means any `Serialize + DeserializeOwned` value that is
/// `Send + Sync + 'static` is automatically an `Event` — no derive needed.
pub trait Event: Serialize + DeserializeOwned + std::fmt::Debug + Send + Sync + 'static {}

impl<T> Event for T where T: Serialize + DeserializeOwned + std::fmt::Debug + Send + Sync + 'static {}

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
pub trait Observer: 'static {
    /// Register subscriptions and restore any previously-saved `state`.
    fn init(&mut self, bus: &mut EventBus, state: StateType) -> CoreResult<()>;
    /// Handle a single on_evented event. Called for every event during `iter`.
    /// Modules that use this instead of closure subscriptions implement their
    /// logic here with `&mut self`, eliminating the need for `Arc<Mutex<Inner>>`.
    fn on_event(&mut self, _event: &dyn Any, _emitter: &Emitter) -> CoreResult<()> {
        Ok(())
    }
    /// Snapshot this observer's durable state so it can be restored later.
    fn save(&self) -> CoreResult<StateType>;
    /// Stable, unique key used to namespace this observer's saved state.
    fn name(&self) -> &'static str;
    /// Downcast to a concrete type. Required by test helpers to inspect state.
    fn as_any_mut(&mut self) -> &mut dyn Any;
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
    /// an [`Error`] event so it can propagate. A failing handler for `Error`
    /// itself is only logged, never re-emitted.
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
    /// Drains every queued event, on_eventing each to its subscribers. Because
    /// handlers publish through a cloned [`Sender`], events they emit land back
    /// on the same queue and are processed within this same call — so a single
    /// `iter` settles the entire cascade. Once the queue is empty, the state of
    /// every observer is collected and returned.
    pub fn iter(&mut self) -> CoreResult<StateType> {
        while let Ok(event) = self.rx.try_recv() {
            if cfg!(debug_assertions) {
                println!("Event: {:?}", &*event);
            }
            // Subscription-based on_event (existing modules).
            {
                let event_any: &dyn Any = event.as_any();
                if let Some(handlers) = self.handlers.get(&event_any.type_id()) {
                    for handler in handlers {
                        handler(event_any);
                    }
                }
            }
            // Direct on_event (modules using the new &mut self pattern).
            let emitter = self.emitter();
            for observer in &mut self.observers {
                observer.on_event(event.as_any(), &emitter)?;
            }
        }
        self.save()
    }

    /// Get a mutable reference to the observer with the given name.
    /// Used by test helpers to inspect or seed module state.
    pub fn observer_mut(&mut self, name: &str) -> Option<&mut dyn Observer> {
        self.observers
            .iter_mut()
            .find(|o| o.name() == name)
            .map(|o| o.as_mut())
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
/// [`RemoteEventBus`]. This lets `ClientController` be written once against
/// `EventChannel` and work regardless of whether the peer lives in this process
/// or on the other end of a socket.
pub trait EventChannel {
    /// Publish `event` to the channel; does not wait for a reply.
    fn publish<E: Event>(&self, event: E) -> CoreResult<()>;

    /// Register `handler` to run for every observed event of type `E`.
    fn on<E: Event>(&mut self, handler: impl Fn(&E) -> CoreResult<()> + Send + Sync + 'static);

    /// Drive any pending in-process work. An [`EventBus`] runs one drain pass; a
    /// [`RemoteEventBus`] (whose reader thread does the work) is a no-op.
    fn pump(&mut self) -> CoreResult<()>;

    /// Publish `request` and block until a matching `Resp` is observed, or 5
    /// seconds elapse.
    fn request<Req: Event, Resp: Event + Clone>(&mut self, request: Req) -> CoreResult<Resp> {
        let (tx, rx) = mpsc::channel();
        self.on::<Resp>(move |resp| {
            // Ignore send errors: a stale handler from a prior request of the
            // same type may already have been dropped.
            let _ = tx.send(resp.clone());
            Ok(())
        });
        self.publish(request)?;
        self.pump()?;
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| match e {
                mpsc::RecvTimeoutError::Timeout => {
                    CoreError::CommandFailed("timed out waiting for daemon response".into())
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    CoreError::InvalidState("event channel closed before response")
                }
            })
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

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
        seen: u32,
    }

    impl Observer for Counter {
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }

        fn init(&mut self, _bus: &mut EventBus, state: StateType) -> CoreResult<()> {
            if let Some(prev) = state.as_u64() {
                self.seen = prev as u32;
            }
            Ok(())
        }

        fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
            crate::dispatch_event!(event, {
                tick: Tick => emitter.send(Tock { n: tick.n }),
                _: Tock => {
                    self.seen += 1;
                    Ok(())
                },
            })
        }

        fn save(&self) -> CoreResult<StateType> {
            Ok(StateType::from(self.seen))
        }

        fn name(&self) -> &'static str {
            "counter"
        }
    }

    #[test]
    fn cascades_within_single_iter_and_saves_state() {
        let mut bus = EventBus::new(vec![Box::new(Counter { seen: 0 })], StateType::Null).unwrap();

        bus.send(Tick { n: 1 }).unwrap();
        bus.send(Tick { n: 2 }).unwrap();
        bus.send(Tick { n: 3 }).unwrap();

        let state = bus.iter().unwrap();

        let seen = bus
            .observer_mut("counter")
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Counter>()
            .unwrap()
            .seen;
        assert_eq!(seen, 3);
        assert_eq!(state["counter"].as_u64(), Some(3));
    }

    #[test]
    fn request_returns_response_over_event_channel() {
        let mut bus = EventBus::new(vec![Box::new(Counter { seen: 0 })], StateType::Null).unwrap();

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

        let mut bus = EventBus::new(vec![Box::new(Counter { seen: 0 })], saved).unwrap();

        let seen = bus
            .observer_mut("counter")
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Counter>()
            .unwrap()
            .seen;
        assert_eq!(seen, 7);
    }

    #[test]
    fn failing_subscription_emits_error_event() {
        // Subscription closures that return Err are caught by the bus and
        // re-emitted as Error events — this tests that contract.
        struct Failing;
        impl Observer for Failing {
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
            fn init(&mut self, bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
                bus.subscribe(|_: &Tick| Err(CoreError::InvalidState("boom")));
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
            errors: Vec<Error>,
        }
        impl Observer for Capture {
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
            fn init(&mut self, _bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
                Ok(())
            }
            fn on_event(&mut self, event: &dyn Any, _emitter: &Emitter) -> CoreResult<()> {
                crate::dispatch_event!(event, {
                    err: Error => {
                        self.errors.push(err.clone());
                        Ok(())
                    },
                })
            }
            fn save(&self) -> CoreResult<StateType> {
                Ok(StateType::Null)
            }
            fn name(&self) -> &'static str {
                "capture"
            }
        }

        let mut bus = EventBus::new(
            vec![Box::new(Failing), Box::new(Capture { errors: Vec::new() })],
            StateType::Null,
        )
        .unwrap();

        bus.send(Tick { n: 1 }).unwrap();
        bus.iter().unwrap();

        let errors = &bus
            .observer_mut("capture")
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Capture>()
            .unwrap()
            .errors;
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("boom"));
        assert!(errors[0].source.contains("Tick"));
    }
}
