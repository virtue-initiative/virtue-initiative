//! Helpers shared by the integration tests that drive the daemon through the
//! `Scenario` DSL. Each test file pulls in only what it needs.
#![allow(dead_code)]

use std::time::{Duration, Instant};

use serde::Deserialize;
use virtue_core::DaemonState;
use virtue_core::crypto::encode_batch_event;
use virtue_core::testing::Scenario;
use virtue_core::{Upload, UploadKind, load_state};

/// The time the first tick of most scenarios runs at. Large enough that
/// "N minutes before now" is still a positive time.
pub const T0: i64 = 1_000_000;

/// A `last_batch_at_ms` so far in the future that the batch interval never
/// elapses, so queued events stay visible in the pending lists.
pub const FAR_FUTURE: i64 = 1 << 50;

pub const DAY_MS: i64 = 24 * 60 * 60 * 1_000;

#[derive(Deserialize)]
struct WireEvent {
    #[serde(rename = "type")]
    kind: String,
    data: Option<WireData>,
}

#[derive(Deserialize)]
struct WireData {
    title: Option<String>,
}

/// An event waiting in the upload queues, reduced to what tests assert on.
#[derive(Debug, Clone, PartialEq)]
pub struct QueuedEvent {
    pub kind: String,
    pub title: Option<String>,
}

/// Every event still waiting in `state`'s upload queues, oldest first: the ones
/// already hashed and waiting for a batch, then the ones still waiting for the
/// hash server. Events in a batch that has already uploaded are gone.
pub fn queued_events_in(state: &DaemonState) -> Vec<QueuedEvent> {
    let mut out = Vec::new();
    for pending in &state.upload.pending_batch_events {
        out.push(decode(&pending.encoded));
    }
    for entry in &state.upload.pending_hash_events {
        let bytes = encode_batch_event(entry).expect("encode queued event");
        out.push(decode(&bytes));
    }
    out
}

fn decode(bytes: &[u8]) -> QueuedEvent {
    let event: WireEvent = rmp_serde::from_slice(bytes).expect("decode queued event");
    QueuedEvent {
        kind: event.kind,
        title: event.data.and_then(|d| d.title),
    }
}

pub fn queued_events(scenario: &Scenario) -> Vec<QueuedEvent> {
    queued_events_in(&scenario.state())
}

pub fn queued_count(scenario: &Scenario, kind: &str) -> usize {
    queued_events(scenario)
        .iter()
        .filter(|e| e.kind == kind)
        .count()
}

/// How many `screenshot_missed` alerts are waiting in the upload queues.
pub fn missed_alerts(scenario: &Scenario) -> usize {
    queued_count(scenario, "screenshot_missed")
}

/// Stops the batch interval from elapsing, so events queued from here on stay
/// in the pending lists (unless something forces a flush).
pub fn hold_batches(scenario: &mut Scenario) {
    scenario.with_state_mut(|s| s.upload.last_batch_at_ms = Some(FAR_FUTURE));
}

/// An authenticated scenario whose first tick has run at [`T0`] with batches
/// held back, so `next_wakeup_at_ms` is the next screenshot draw and every
/// event the daemon queues from here on stays visible.
pub fn held_scenario() -> Scenario {
    let mut scenario = Scenario::authenticated();
    hold_batches(&mut scenario);
    scenario.at_t(T0).tick();
    scenario
}

pub fn wakeup(scenario: &Scenario) -> i64 {
    scenario.state().next_wakeup_at_ms
}

/// Ticks once for each entry of `lateness_ms`, each at the scheduled wakeup
/// plus that lateness (negative means early).
pub fn tick_with_lateness(scenario: &mut Scenario, lateness_ms: &[i64]) {
    for late in lateness_ms {
        let expected = wakeup(scenario);
        scenario.at_t(expected + late).tick();
    }
}

/// Total hash and batch uploads the mock API has seen.
pub fn upload_total(scenario: &Scenario) -> usize {
    let api = scenario.api.state();
    api.hash_uploads.len() + api.batch_uploads.len()
}

/// A zero-risk event that exists only to be queued.
pub fn dev(title: &str) -> Upload {
    Upload {
        risk: 0.0,
        kind: UploadKind::Dev {
            title: title.into(),
            details: None,
        },
    }
}

/// An event with the given risk.
pub fn dev_with_risk(title: &str, risk: f32) -> Upload {
    Upload { risk, ..dev(title) }
}

/// Polls `check` for up to five seconds; for assertions about a loop thread.
pub fn eventually(mut check: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    check()
}

/// The state file as a reader outside the daemon sees it.
pub fn persisted(scenario: &Scenario) -> DaemonState {
    load_state::<DaemonState>(&scenario.state_dir_path().join("event_state.json"))
        .expect("read the state file")
}
