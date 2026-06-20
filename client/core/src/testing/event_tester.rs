use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::events::Ping;
use crate::events::bus::{Error, Event, EventBus, Observer, StateType};
use crate::model::PartialStatus;
use crate::module::upload::Upload;

use super::{MockApiClient, MockClock, TestPlatformHooks};

type CaptureStore = HashMap<TypeId, Box<dyn Any + Send + Sync>>;
type CaptureClearers = Vec<Box<dyn Fn() + Send + Sync>>;
type CaptureRegistrar =
    Box<dyn Fn(&mut EventBus, &mut CaptureStore, &mut CaptureClearers) + Send + Sync>;

fn register_capture<T: Event + Clone>(
    bus: &mut EventBus,
    store: &mut CaptureStore,
    clearers: &mut CaptureClearers,
) {
    if store.contains_key(&TypeId::of::<T>()) {
        return;
    }
    let vec: Arc<Mutex<Vec<T>>> = Arc::new(Mutex::new(Vec::new()));
    let vec_for_sub = Arc::clone(&vec);
    let vec_for_clear = Arc::clone(&vec);
    bus.subscribe(move |ev: &T| {
        vec_for_sub.lock().unwrap().push(ev.clone());
        Ok(())
    });
    clearers.push(Box::new(move || {
        vec_for_clear.lock().unwrap().clear();
    }));
    store.insert(TypeId::of::<T>(), Box::new(vec));
}

pub struct EventTesterBuilder {
    pub clock: MockClock,
    platform: TestPlatformHooks,
    api: MockApiClient,
    observers: Vec<Box<dyn Observer>>,
    names: Vec<(TypeId, &'static str)>,
    state: StateType,
    extra_captures: Vec<CaptureRegistrar>,
}

impl EventTesterBuilder {
    fn new() -> Self {
        let clock = MockClock::default();
        let platform = TestPlatformHooks::with_clock(clock.clone());
        let api = MockApiClient::new();
        Self {
            clock,
            platform,
            api,
            observers: Vec::new(),
            names: Vec::new(),
            state: StateType::Null,
            extra_captures: Vec::new(),
        }
    }

    /// Return a clone of the shared platform hooks (shares clock + screenshot state).
    pub fn platform(&self) -> TestPlatformHooks {
        self.platform.clone()
    }

    /// Return a clone of the shared mock API client (shares recording state).
    pub fn api(&self) -> MockApiClient {
        self.api.clone()
    }

    pub fn add<M: Observer>(&mut self, module: M) -> &mut Self {
        self.names.push((TypeId::of::<M>(), module.name()));
        self.observers.push(Box::new(module));
        self
    }

    pub fn with_state(&mut self, state: StateType) -> &mut Self {
        self.state = state;
        self
    }

    /// Register an extra event type for capture (in addition to the defaults:
    /// `Upload`, `PartialStatus`, `Error`). Must be called before `build()`.
    pub fn capture<T: Event + Clone>(&mut self) -> &mut Self {
        self.extra_captures.push(Box::new(|bus, store, clearers| {
            register_capture::<T>(bus, store, clearers);
        }));
        self
    }

    pub fn build(self) -> EventTester {
        // Inline spawner: background jobs run synchronously, so events they emit
        // cascade within the same `iter()` and the tester stays deterministic.
        let mut bus =
            EventBus::with_spawner(self.observers, self.state, Arc::new(super::InlineSpawner))
                .expect("EventBus construction failed");
        let mut captures = CaptureStore::new();
        let mut clearers = CaptureClearers::new();

        register_capture::<Upload>(&mut bus, &mut captures, &mut clearers);
        register_capture::<PartialStatus>(&mut bus, &mut captures, &mut clearers);
        register_capture::<Error>(&mut bus, &mut captures, &mut clearers);

        for registrar in self.extra_captures {
            registrar(&mut bus, &mut captures, &mut clearers);
        }

        let names: HashMap<TypeId, &'static str> = self.names.into_iter().collect();

        EventTester {
            bus,
            clock: self.clock,
            platform: self.platform,
            api: self.api,
            names,
            captures,
            capture_clearers: clearers,
            ping_enabled: false,
            next_ping_ms: 0,
            ping_interval_ms: 1000,
            disable_at_ms: None,
            cursor_ms: 0,
        }
    }
}

pub struct EventTester {
    pub bus: EventBus,
    pub clock: MockClock,
    pub platform: TestPlatformHooks,
    pub api: MockApiClient,
    names: HashMap<TypeId, &'static str>,
    captures: CaptureStore,
    capture_clearers: CaptureClearers,
    ping_enabled: bool,
    next_ping_ms: i64,
    ping_interval_ms: i64,
    disable_at_ms: Option<i64>,
    pub cursor_ms: i64,
}

impl EventTester {
    pub fn builder() -> EventTesterBuilder {
        EventTesterBuilder::new()
    }

    /// Advance the clock to `secs`, backfilling any pending pings, then send
    /// `event` and run one full bus drain.
    pub fn emit<E: Event>(&mut self, secs: impl Into<f64>, event: E) -> &mut Self {
        let ms = (secs.into() * 1000.0).round() as i64;
        self.backfill_to(ms);
        self.clock.set(ms);
        self.bus.send(event).expect("bus.send failed in emit");
        self.bus.iter().expect("bus.iter failed in emit");
        self.cursor_ms = ms;
        self
    }

    /// Advance the clock to `secs`, backfilling any pending pings, then flush
    /// the bus without injecting a new event.
    pub fn advance_to(&mut self, secs: impl Into<f64>) -> &mut Self {
        let ms = (secs.into() * 1000.0).round() as i64;
        self.backfill_to(ms);
        self.clock.set(ms);
        self.bus.iter().expect("bus.iter failed in advance_to");
        self.cursor_ms = ms;
        self
    }

    /// Enable automatic `Ping` backfilling every 1 s starting at `start_secs`.
    pub fn enable_pings(&mut self, start_secs: impl Into<f64>) -> &mut Self {
        let start_ms = (start_secs.into() * 1000.0).round() as i64;
        self.ping_enabled = true;
        self.next_ping_ms = start_ms;
        self.ping_interval_ms = 1000;
        self.disable_at_ms = None;
        self
    }

    /// Like `enable_pings` but with a custom interval.
    pub fn enable_pings_every(
        &mut self,
        start_secs: impl Into<f64>,
        interval_secs: impl Into<f64>,
    ) -> &mut Self {
        let start_ms = (start_secs.into() * 1000.0).round() as i64;
        self.ping_enabled = true;
        self.next_ping_ms = start_ms;
        self.ping_interval_ms = (interval_secs.into() * 1000.0).round() as i64;
        self.disable_at_ms = None;
        self
    }

    /// Stop automatic pings at `at_secs` (exclusive — a ping scheduled exactly
    /// at `at_secs` will NOT fire).
    pub fn disable_pings(&mut self, at_secs: impl Into<f64>) -> &mut Self {
        let at_ms = (at_secs.into() * 1000.0).round() as i64;
        self.disable_at_ms = Some(at_ms);
        self
    }

    /// Get a mutable reference to the observer added as type `M`.
    /// Panics if `M` was not added to the builder.
    pub fn observer<M: Observer>(&mut self) -> &mut M {
        let type_id = TypeId::of::<M>();
        let name = *self.names.get(&type_id).unwrap_or_else(|| {
            panic!(
                "module {} was not added to EventTester; call builder.add(module) before build()",
                std::any::type_name::<M>()
            )
        });
        self.bus
            .observer_mut(name)
            .unwrap_or_else(|| panic!("observer '{}' not found in bus", name))
            .as_any_mut()
            .downcast_mut::<M>()
            .unwrap_or_else(|| {
                panic!(
                    "downcast to {} failed for observer '{}'",
                    std::any::type_name::<M>(),
                    name
                )
            })
    }

    /// Clone out all captured events of type `T`.
    /// Panics if `T` was not registered (default: `Upload`, `PartialStatus`, `Error`).
    pub fn captured<T: Event + Clone>(&self) -> Vec<T> {
        let boxed = self.captures.get(&TypeId::of::<T>()).unwrap_or_else(|| {
            panic!(
                "type {} was not registered for capture; \
                 call builder.capture::<{}>() before build()",
                std::any::type_name::<T>(),
                std::any::type_name::<T>()
            )
        });
        boxed
            .downcast_ref::<Arc<Mutex<Vec<T>>>>()
            .expect("capture store downcast failed")
            .lock()
            .unwrap()
            .clone()
    }

    /// Pass if any captured `T` satisfies `pred`; otherwise panic, printing all
    /// captured values. Use with the [`like!`] macro for ergonomic matching.
    pub fn assert_like<T: Event + Clone + std::fmt::Debug>(
        &self,
        pred: impl Fn(&T) -> bool,
    ) -> &Self {
        let items = self.captured::<T>();
        if !items.iter().any(pred) {
            panic!(
                "assert_like failed: no {} matched the predicate\ncaptured:\n{:#?}",
                std::any::type_name::<T>(),
                items
            );
        }
        self
    }

    /// Fail if any captured `T` satisfies `pred`.
    pub fn assert_not_like<T: Event + Clone + std::fmt::Debug>(
        &self,
        pred: impl Fn(&T) -> bool,
    ) -> &Self {
        let items = self.captured::<T>();
        if let Some(found) = items.iter().find(|x| pred(x)) {
            panic!(
                "assert_not_like failed: unexpected {} matched the predicate\nfound:\n{:#?}",
                std::any::type_name::<T>(),
                found
            );
        }
        self
    }

    /// Clear all capture buffers (useful between phases of a single test).
    pub fn clear_captured(&mut self) -> &mut Self {
        for clearer in &self.capture_clearers {
            clearer();
        }
        self
    }

    fn backfill_to(&mut self, target_ms: i64) {
        while self.ping_enabled
            && self.next_ping_ms <= target_ms
            && self.disable_at_ms.is_none_or(|d| self.next_ping_ms < d)
        {
            self.clock.set(self.next_ping_ms);
            self.bus
                .send(Ping)
                .expect("bus.send(Ping) failed in backfill");
            self.bus.iter().expect("bus.iter failed in backfill");
            self.next_ping_ms += self.ping_interval_ms;
        }
        if self.disable_at_ms.is_some_and(|d| self.next_ping_ms >= d) {
            self.ping_enabled = false;
        }
    }
}

/// Build a typed predicate closure for use with [`EventTester::assert_like`].
///
/// The struct form infers the closure parameter type from the head identifier,
/// so no turbofish is needed:
/// ```rust,ignore
/// t.assert_like(like!(Upload { kind: UploadKind::LifecycleAlert { .. }, .. }));
/// ```
///
/// For enum-variant or path patterns, add a turbofish on `assert_like`:
/// ```rust,ignore
/// t.assert_like::<PartialStatus>(like!(PartialStatus::Lifecycle { is_running: true, .. }));
/// ```
#[macro_export]
macro_rules! like {
    ($t:ident { $($body:tt)* }) => {
        |__ev: &$t| ::std::matches!(__ev, $t { $($body)* })
    };
    ($($pat:tt)+) => {
        |__ev| ::std::matches!(__ev, $($pat)+)
    };
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::error::CoreResult;
    use crate::events::Ping;
    use crate::events::bus::{Emitter, EventBus, Observer, StateType};
    use crate::model::{AlertReason, LifecycleKind, UploadKind};
    use crate::module::lifecycle::{
        ComputerSuspended, LifecycleModule, LifecycleStatus, ProcessStarted,
    };

    /// Minimal observer that counts how many Ping events it receives.
    struct PingCounter {
        count: usize,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct _PingCountState;

    impl Observer for PingCounter {
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
        fn name(&self) -> &'static str {
            "ping_counter"
        }
        fn init(&mut self, _bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
            Ok(())
        }
        fn on_event(&mut self, event: &dyn Any, _emitter: &Emitter) -> CoreResult<()> {
            if event.downcast_ref::<Ping>().is_some() {
                self.count += 1;
            }
            Ok(())
        }
        fn save(&self) -> CoreResult<StateType> {
            Ok(StateType::Null)
        }
    }

    #[test]
    fn disable_pings_boundary_is_exclusive() {
        // Pings at 1s, 2s, 3s, 4s — but NOT at 5s (disable_at = 5s, exclusive).
        let mut b = EventTester::builder();
        b.add(PingCounter { count: 0 });
        let mut t = b.build();
        t.enable_pings(1);
        t.disable_pings(5);
        t.advance_to(10);
        assert_eq!(
            t.observer::<PingCounter>().count,
            4,
            "expected pings at 1s, 2s, 3s, 4s only (5s is the exclusive boundary)"
        );
    }

    #[test]
    fn ping_backfill_fires_at_correct_times() {
        // enable_pings(2) then emit(5.5, Ping):
        // backfill fires at 2000, 3000, 4000, 5000 (4 pings) then explicit Ping at 5500
        let mut b = EventTester::builder();
        b.add(PingCounter { count: 0 });
        let mut t = b.build();
        t.enable_pings(2);
        t.emit(5.5_f64, Ping);
        assert_eq!(
            t.observer::<PingCounter>().count,
            5,
            "expected 4 backfilled + 1 explicit ping"
        );
    }

    #[test]
    fn observer_lookup_returns_correct_module() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(1, ProcessStarted);
        assert_eq!(
            t.observer::<LifecycleModule>().state.last_process_started,
            1_000
        );
    }

    #[test]
    #[should_panic(expected = "assert_like failed")]
    fn assert_like_panics_when_no_match() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        // ComputerSuspended at 1s emits a Lifecycle upload but NOT a LifecycleAlert.
        t.emit(1, ComputerSuspended);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::PingGapWhileRunning
            },
            ..
        }));
    }

    #[test]
    fn assert_not_like_passes_when_absent() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(1, ComputerSuspended);
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::PingGapWhileRunning
            },
            ..
        }));
    }

    #[test]
    fn clear_captured_resets_buffers() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(1, ProcessStarted);
        assert!(!t.captured::<Upload>().is_empty());
        t.clear_captured();
        assert!(t.captured::<Upload>().is_empty());
    }

    #[test]
    fn assert_like_matches_lifecycle_kind() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(1, ComputerSuspended);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::ComputerSuspended
            },
            ..
        }));
    }

    #[test]
    fn missing_resume_fires_on_fourth_ping_with_enable_disable_pings() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();

        t.emit(1, ComputerSuspended);
        t.clear_captured();

        // Pings at 2s, 3s, 4s (3 pings) — NOT at 5s (exclusive boundary)
        t.enable_pings(2);
        t.disable_pings(5);
        t.advance_to(5);
        assert_eq!(
            t.observer::<LifecycleModule>().state.pings_while_suspended,
            3
        );
        t.clear_captured();

        // 4th ping crosses the >3 threshold
        t.emit(6, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::MissingResume
            },
            ..
        }));
        assert_eq!(
            t.observer::<LifecycleModule>().state.pings_while_suspended,
            0
        );
        assert!(matches!(
            t.observer::<LifecycleModule>().state.status,
            LifecycleStatus::Running
        ));
    }
}
