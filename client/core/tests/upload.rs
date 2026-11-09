//! Upload and retry rules (CORE-005), driven through the `Scenario` DSL.
//!
//! Run with:
//!   cargo test -p virtue-core --features testing --test upload
#![cfg(feature = "testing")]

mod common;

use common::*;
use virtue_core::CoreError;
use virtue_core::api::{DeviceState, UploadedBatchResponse};
use virtue_core::model::{BatchRecipient, DeviceSettings};
use virtue_core::testing::Scenario;

/// An authenticated scenario with the next screenshot far away, so the only
/// events are the ones a test queues.
fn quiet_scenario() -> Scenario {
    let mut scenario = Scenario::authenticated();
    scenario.with_state_mut(|s| s.screenshot.next_screenshot_at_ms = Some(FAR_FUTURE));
    scenario
}

fn down() -> CoreError {
    CoreError::CommandFailed("server down".into())
}

// ── Nothing is queued without credentials ────────────────────────────────────

// CORE-005: while no device credentials are stored, events are not queued.
#[test]
fn events_are_not_queued_while_no_device_credentials_are_stored() {
    let mut scenario = Scenario::new();
    scenario
        .at_t(T0)
        .queue_upload(dev("ignored"))
        .note_user_stop("test")
        .force_capture_now()
        .flush_batch_now()
        .tick_n(3);

    assert!(queued_events(&scenario).is_empty());
    let status = scenario.status();
    assert_eq!(status.pending_hash_count, 0);
    assert_eq!(status.pending_batch_count, 0);
    let api = scenario.api.state();
    assert!(api.hash_uploads.is_empty() && api.batch_uploads.is_empty());
}

// ── Retries with exponential backoff, kept in the state object ───────────────

/// Drives `attempts` failed upload attempts, each at the moment the previous
/// backoff ended, checking via `count` that nothing is retried a moment
/// before. Returns how long each failure's backoff was.
fn backoff_delays(
    scenario: &mut Scenario,
    attempts: usize,
    count: impl Fn(&Scenario) -> usize,
    retry_at: impl Fn(&Scenario) -> i64,
) -> Vec<i64> {
    let mut delays = Vec::new();
    let mut now = T0;
    for attempt in 1..=attempts {
        scenario.at_t(now).tick();
        assert_eq!(
            count(scenario),
            attempt,
            "attempt {attempt} should have been made"
        );

        let retry = retry_at(scenario);
        delays.push(retry - now);
        scenario.at_t(retry - 1).tick();
        assert_eq!(
            count(scenario),
            attempt,
            "attempt {attempt} must not be retried before its backoff ends"
        );
        now = retry;
    }
    scenario.at_t(now);
    delays
}

fn assert_delays_grow(delays: &[i64]) {
    assert!(delays[0] > 0);
    for pair in delays.windows(2) {
        // Each wait doubles, give or take a few hundred ms of jitter.
        assert!(
            pair[1] >= 2 * pair[0] - 500,
            "backoff should grow exponentially: {delays:?}"
        );
    }
}

// CORE-005: a failed hash upload is kept in state and retried with exponential backoff.
#[test]
fn hash_uploads_are_retried_with_exponential_backoff() {
    let mut scenario = quiet_scenario();
    hold_batches(&mut scenario);
    for _ in 0..3 {
        scenario.api.program_hash(Err(down()));
    }
    scenario.queue_upload(dev("retry me"));

    let delays = backoff_delays(
        &mut scenario,
        3,
        |s| s.api.state().hash_uploads.len(),
        |s| s.state().upload.hash_backoff.next_attempt_at_ms,
    );
    assert_delays_grow(&delays);
    assert_eq!(scenario.state().upload.pending_hash_events.len(), 1);

    // The fourth attempt succeeds and the event moves on to its batch.
    scenario.tick();
    assert_eq!(scenario.api.state().hash_uploads.len(), 4);
    assert!(scenario.state().upload.pending_hash_events.is_empty());
    assert_eq!(scenario.state().upload.pending_batch_events.len(), 1);
}

// CORE-005: a failed batch upload is kept in state and retried with exponential backoff.
#[test]
fn batch_uploads_are_retried_with_exponential_backoff() {
    let mut scenario = quiet_scenario();
    for _ in 0..3 {
        scenario.api.program_batch(Err(down()));
    }
    scenario.queue_upload(dev("retry me"));

    let delays = backoff_delays(
        &mut scenario,
        3,
        |s| s.api.state().batch_uploads.len(),
        |s| s.state().upload.batch_backoff.next_attempt_at_ms,
    );
    assert_delays_grow(&delays);
    assert_eq!(scenario.state().upload.pending_batch_events.len(), 1);

    scenario.tick();
    assert_eq!(scenario.api.state().batch_uploads.len(), 4);
    assert!(scenario.state().upload.pending_batch_events.is_empty());
}

// CORE-005: the retry backoff is stored in the state object, so a restart does not retry early.
#[test]
fn a_restart_does_not_retry_before_the_stored_backoff_ends() {
    let mut first = quiet_scenario();
    hold_batches(&mut first);
    first.api.program_hash(Err(down()));
    first.queue_upload(dev("retry me"));
    first.at_t(T0).tick();
    let retry_at = first.state().upload.hash_backoff.next_attempt_at_ms;
    assert!(retry_at > T0 + 500);

    let mut second =
        Scenario::authenticated_with_state_dir_at(first.state_dir_path().to_path_buf(), T0 + 500);
    second.tick();
    assert!(
        second.api.state().hash_uploads.is_empty(),
        "still backing off"
    );
    assert_eq!(second.state().upload.pending_hash_events.len(), 1);

    second.at_t(retry_at).tick();
    assert_eq!(second.api.state().hash_uploads.len(), 1);
}

// CORE-005: a batch that cannot be built backs off rather than being retried at once.
#[test]
fn a_batch_that_cannot_be_built_backs_off_like_a_failed_upload() {
    let mut scenario = quiet_scenario();
    scenario.with_state_mut(|s| {
        // A recipient key that is not valid base64 makes the batch unbuildable.
        s.upload.settings.as_mut().unwrap().wrapping_keys[0].pub_key_base64 = "not base64!".into();
    });
    scenario.queue_upload(dev("cannot be batched"));

    let build_errors = |s: &mut Scenario| {
        s.status()
            .recent_errors
            .iter()
            .filter(|e| e.context == "batch_upload")
            .count()
    };

    scenario.at_t(T0).tick();
    assert_eq!(build_errors(&mut scenario), 1);
    let retry_at = scenario.state().upload.batch_backoff.next_attempt_at_ms;
    assert!(retry_at > T0, "must not be due again at once");
    assert_eq!(scenario.state().next_wakeup_at_ms, retry_at);

    // Inside the backoff nothing is attempted.
    scenario.at_t(retry_at - 1).tick();
    assert_eq!(build_errors(&mut scenario), 1);

    // Once it ends the build is attempted again, and again backs off for longer.
    scenario.at_t(retry_at).tick();
    assert_eq!(build_errors(&mut scenario), 2);
    let second_retry = scenario.state().upload.batch_backoff.next_attempt_at_ms;
    assert!(second_retry - retry_at >= 2 * (retry_at - T0) - 500);

    // Nothing reached the API, and a fixed key lets the batch through.
    assert!(scenario.api.state().batch_uploads.is_empty());
    scenario.with_state_mut(|s| {
        s.upload.settings.as_mut().unwrap().wrapping_keys[0].pub_key_base64 =
            "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into();
    });
    scenario.at_t(second_retry).tick();
    assert_eq!(scenario.api.state().batch_uploads.len(), 1);
}

// ── The screen lock gate ─────────────────────────────────────────────────────

// CORE-005: a batch is not uploaded while locked unless forced by a heartbeat or flush_batch_now.
#[test]
fn a_locked_screen_holds_the_batch_unless_a_heartbeat_or_flush_forces_it() {
    struct Case {
        name: &'static str,
        act: fn(&mut Scenario),
        uploads_while_locked: usize,
    }
    let cases = [
        Case {
            name: "a due batch waits while the screen is locked",
            act: |s| {
                s.at_t(T0 + 70_000);
            },
            uploads_while_locked: 0,
        },
        Case {
            name: "flush_batch_now uploads while the screen is locked",
            act: |s| {
                s.at_t(T0 + 2_000).flush_batch_now();
            },
            uploads_while_locked: 1,
        },
        Case {
            name: "a heartbeat uploads while the screen is locked",
            act: |s| {
                s.with_state_mut(|st| st.heartbeat.last_heartbeat_ms = T0 - DAY_MS + 1_000);
                s.at_t(T0 + 2_000);
            },
            uploads_while_locked: 1,
        },
        Case {
            name: "an event over 90% forces the batch but not past the lock",
            act: |s| {
                s.at_t(T0 + 2_000)
                    .queue_upload(dev_with_risk("alert", 0.95));
            },
            uploads_while_locked: 0,
        },
    ];

    for case in &cases {
        let mut scenario = quiet_scenario();
        scenario.with_state_mut(|s| s.upload.last_batch_at_ms = Some(T0));
        scenario.queue_upload(dev("waiting for the batch"));
        scenario.at_t(T0 + 1_000).tick();
        assert_eq!(scenario.state().upload.pending_batch_events.len(), 1);
        assert!(scenario.api.state().batch_uploads.is_empty());

        scenario.platform.set_locked_or_screensaver(true);
        (case.act)(&mut scenario);
        scenario.tick();
        assert_eq!(
            scenario.api.state().batch_uploads.len(),
            case.uploads_while_locked,
            "{}",
            case.name
        );

        // Held, not lost: unlocking lets it through.
        scenario.platform.set_locked_or_screensaver(false);
        let later = scenario.clock.now_ms() + 1_000;
        scenario.at_t(later).tick();
        assert!(
            !scenario.api.state().batch_uploads.is_empty(),
            "{}: the batch should upload once unlocked",
            case.name
        );
    }
}

// ── Risk forcing and the event counts ────────────────────────────────────────

// CORE-005: a risk of 90% or more, rounded to a whole percent, forces the next batch.
#[test]
fn an_event_at_ninety_percent_rounded_forces_the_batch_without_waiting_for_the_interval() {
    for (risk, forced) in [
        (0.5, false),
        (0.8949, false),
        (0.8951, true),
        (0.9, true),
        (1.0, true),
    ] {
        let mut scenario = quiet_scenario();
        scenario.with_state_mut(|s| s.upload.last_batch_at_ms = Some(T0));
        scenario.queue_upload(dev_with_risk("risky", risk));
        scenario.at_t(T0 + 1_000).tick();

        assert_eq!(
            scenario.api.state().batch_uploads.len(),
            usize::from(forced),
            "risk {risk}"
        );
    }
}

// CORE-005: a heartbeat forces the next batch without waiting for the interval.
#[test]
fn a_heartbeat_forces_the_batch_without_waiting_for_the_interval() {
    let mut scenario = quiet_scenario();
    scenario.with_state_mut(|s| {
        s.upload.last_batch_at_ms = Some(T0);
        s.heartbeat.last_heartbeat_ms = T0 - DAY_MS + 500;
    });
    scenario.at_t(T0 + 1_000).tick();

    assert_eq!(
        queued_count(&scenario, "heartbeat"),
        0,
        "uploaded, not waiting"
    );
    assert_eq!(scenario.api.state().batch_uploads.len(), 1);
}

// CORE-005: events count as high from 70% and medium from 40% to below 70%, by rounded percentage.
#[test]
fn batch_event_counts_use_the_rounded_percentage_bands() {
    let mut scenario = quiet_scenario();
    // 39%: neither. 40%, 69%: medium. 70% (rounded up from 69.51), 70%, 89%, 90%: high.
    for (i, risk) in [0.3949, 0.4, 0.6949, 0.6951, 0.7, 0.8949, 0.9]
        .into_iter()
        .enumerate()
    {
        scenario.queue_upload(dev_with_risk(&format!("risk {i}"), risk));
    }
    scenario.at_t(T0).flush_batch_now().tick();

    let api = scenario.api.state();
    assert_eq!(api.batch_uploads.len(), 1);
    let batch = &api.batch_uploads[0].batch;
    assert_eq!(batch.total_count, 7);
    assert_eq!(batch.medium_risk_count, 2);
    assert_eq!(batch.high_risk_count, 4);
}

// ── What a batch holds ───────────────────────────────────────────────────────

// CORE-005: a batch holds every hashed event in order, and no more than 200 hashes wait for it.
#[test]
fn hashing_stops_at_200_waiting_events_until_their_batch_uploads() {
    let mut scenario = quiet_scenario();
    // A full batch is sent at once, so make its first upload fail to keep the
    // 200 events waiting while the rest are queued behind them.
    scenario.api.program_batch(Err(down()));
    for i in 0..250 {
        scenario.queue_upload(dev(&format!("event {i:03}")));
    }

    // A tick hashes at most 8 events, so this is more than enough ticks.
    scenario.at_t(T0).tick_n(40);
    assert_eq!(
        scenario.api.state().batch_uploads.len(),
        1,
        "the full batch was tried"
    );
    assert_eq!(scenario.api.state().hash_uploads.len(), 200);
    let state = scenario.state();
    assert_eq!(state.upload.pending_batch_events.len(), 200);
    assert_eq!(state.upload.pending_hash_events.len(), 50);
    let titles: Vec<String> = queued_events(&scenario)
        .into_iter()
        .filter_map(|e| e.title)
        .collect();
    let expected: Vec<String> = (0..250).map(|i| format!("event {i:03}")).collect();
    assert_eq!(
        titles, expected,
        "events keep the order they were queued in"
    );

    // Once the full batch uploads, the remaining 50 are hashed for the next one.
    let retry_at = state.upload.batch_backoff.next_attempt_at_ms;
    scenario.at_t(retry_at).tick();
    assert_eq!(scenario.api.state().batch_uploads.len(), 2);
    assert_eq!(scenario.api.state().batch_uploads[1].batch.total_count, 200);
    scenario.tick_n(10);
    assert_eq!(scenario.api.state().hash_uploads.len(), 250);
    assert_eq!(scenario.state().upload.pending_batch_events.len(), 50);

    scenario.flush_batch_now().tick();
    let api = scenario.api.state();
    assert_eq!(api.batch_uploads.len(), 3);
    assert_eq!(api.batch_uploads[2].batch.total_count, 50);
}

// ── Device settings and the hash token ───────────────────────────────────────

fn settings_with(flags: &[&str], recipients: usize) -> DeviceSettings {
    DeviceSettings {
        device_id: "scenario-device".into(),
        name: "refreshed device".into(),
        platform: "test-platform".into(),
        wrapping_keys: (0..recipients)
            .map(|i| BatchRecipient {
                user_id: format!("user-{i}"),
                pub_key_base64: "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            })
            .collect(),
        hash_base_url: None,
        feature_flags: flags.iter().map(|f| f.to_string()).collect(),
    }
}

// CORE-005: device settings are refreshed on startup, with their feature flags saved to state.
#[test]
fn settings_and_feature_flags_are_fetched_on_startup_and_saved() {
    let first = Scenario::authenticated();
    assert_eq!(first.api.state().get_device_settings_calls.len(), 1);
    assert!(first.state().upload.settings.is_some());

    let second = Scenario::authenticated_with_state_dir_configured(
        first.state_dir_path().to_path_buf(),
        T0,
        |_, api| api.state().default_device_settings.feature_flags = vec!["beta".into()],
    );
    for (name, settings) in [
        ("in memory", second.state().upload.settings),
        ("on disk", persisted(&second).upload.settings),
    ] {
        let settings = settings.expect("settings saved");
        assert!(settings.has_feature_flag("beta"), "{name}");
        assert!(!settings.has_feature_flag("other"), "{name}");
    }
}

// CORE-005: device settings are refreshed on batch upload and saved.
#[test]
fn a_batch_upload_refreshes_the_saved_settings_and_hash_token() {
    let mut scenario = quiet_scenario();
    scenario.api.program_batch(Ok(UploadedBatchResponse {
        id: "batch-1".into(),
        settings: settings_with(&["gamma"], 2),
        hash_token: "token-from-the-batch".into(),
    }));
    scenario.queue_upload(dev("batch me"));
    scenario.at_t(T0).tick();

    let state = scenario.state();
    let settings = state.upload.settings.expect("settings kept");
    assert!(settings.has_feature_flag("gamma"));
    assert_eq!(
        state.upload.hash_token_cache.map(|(token, _)| token),
        Some("token-from-the-batch".to_string())
    );
    assert_eq!(scenario.status().partner_count, Some(1));
}

// CORE-005: every feature flag is off until settings have been fetched.
#[test]
fn feature_flags_are_off_until_settings_have_been_fetched() {
    let mut first = quiet_scenario();
    // The settings are not known yet, as on an install that has never fetched them.
    first.with_state_mut(|s| s.upload.settings = None);
    first.at_t(T0).tick();

    let mut second = Scenario::authenticated_with_state_dir_configured(
        first.state_dir_path().to_path_buf(),
        T0 + 1_000,
        |_, api| api.program_get_device_settings(Err(down())),
    );
    let settings = second.state().upload.settings;
    assert!(settings.is_none());
    assert!(!settings.is_some_and(|s| s.has_feature_flag("beta")));
    assert_eq!(second.status().partner_count, None);
}

const MINUTE_MS: i64 = 60_000;

// CORE-005: the hash token is kept until shortly before it expires, not refreshed on a short schedule.
#[test]
fn the_hash_token_is_kept_until_shortly_before_it_expires() {
    let mut scenario = quiet_scenario();
    hold_batches(&mut scenario);
    assert_eq!(
        scenario.api.state().get_device_settings_calls.len(),
        1,
        "startup fetch"
    );

    // Well past the one-minute batch interval, and most of the token's 2 hours.
    for minutes in [2, 30, 60, 100] {
        scenario.queue_upload(dev("hash me"));
        scenario.at_t(minutes * MINUTE_MS).tick();
    }
    {
        let api = scenario.api.state();
        assert_eq!(api.get_device_settings_calls.len(), 1, "no refresh yet");
        assert_eq!(api.hash_uploads.len(), 4);
        assert!(
            api.hash_uploads
                .iter()
                .all(|h| h.hash_jwt == "mock-hash-token")
        );
    }

    // Shortly before the 2 hours are up a new one is fetched.
    let default_settings = scenario.api.state().default_device_settings.clone();
    scenario.api.program_get_device_settings(Ok(DeviceState {
        settings: default_settings,
        hash_token: "renewed-token".into(),
    }));
    scenario.queue_upload(dev("hash me"));
    scenario.at_t(115 * MINUTE_MS).tick();
    let api = scenario.api.state();
    assert_eq!(api.get_device_settings_calls.len(), 2);
    assert_eq!(api.hash_uploads.last().unwrap().hash_jwt, "renewed-token");
}

// CORE-005: a hash server 401 means a new token is fetched before the next hash upload.
#[test]
fn an_unauthorized_hash_upload_fetches_a_new_token_before_the_next_one() {
    let mut scenario = quiet_scenario();
    hold_batches(&mut scenario);
    scenario.api.program_hash(Err(CoreError::HttpStatus {
        status: 401,
        message: "token rejected".into(),
    }));
    let default_settings = scenario.api.state().default_device_settings.clone();
    scenario.api.program_get_device_settings(Ok(DeviceState {
        settings: default_settings,
        hash_token: "fresh-token".into(),
    }));
    scenario.queue_upload(dev("hash me"));

    scenario.at_t(T0).tick();
    {
        let api = scenario.api.state();
        assert_eq!(api.hash_uploads.len(), 1);
        assert_eq!(api.hash_uploads[0].hash_jwt, "mock-hash-token");
        assert_eq!(
            api.get_device_settings_calls.len(),
            1,
            "only the startup fetch so far"
        );
    }

    let retry_at = scenario.state().upload.hash_backoff.next_attempt_at_ms;
    scenario.at_t(retry_at).tick();
    let api = scenario.api.state();
    assert_eq!(
        api.get_device_settings_calls.len(),
        2,
        "a new token was fetched"
    );
    assert_eq!(api.hash_uploads.len(), 2);
    assert_eq!(api.hash_uploads[1].hash_jwt, "fresh-token");
}
