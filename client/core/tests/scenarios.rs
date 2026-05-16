//! Behavioral scenario tests for `MonitorService`.
//!
//! These run on every PR via `cargo test -p virtue-core --features testing
//! --test scenarios`. Each scenario uses the `Scenario` DSL from
//! `virtue_core::testing::scenario` to drive the service through a sequence
//! of events without real HTTP, real screenshots, or wall-clock time.
//!
//! Add new scenarios here; do not put them in the in-crate `#[cfg(test)]`
//! modules. The integration-test boundary forces the DSL to be reachable
//! through `virtue_core`'s public API only, which is the same surface
//! platform crates would use.

use virtue_core::lifecycle::{LifecycleObservation, ServiceRole};
use virtue_core::testing::Scenario;

#[test]
fn fresh_unauthenticated_service_loops_cleanly_with_no_uploads() {
    let mut scenario = Scenario::new();

    scenario
        .assert_is_running(true)
        .assert_is_authenticated(false);

    scenario
        .at_t(0)
        .loop_iteration()
        .at_t(60_000)
        .loop_iteration()
        .at_t(120_000)
        .loop_iteration();

    scenario
        .assert_is_running(true)
        .assert_is_authenticated(false)
        .assert_batch_upload_count(0)
        .assert_log_upload_count(0)
        .assert_errors_log_empty();
}

#[test]
fn service_stop_observation_records_to_lifecycle_observations_jsonl() {
    let mut scenario = Scenario::authenticated();

    scenario
        .assert_is_running(true)
        .assert_is_authenticated(true);

    scenario.at_t(180_000).observe(LifecycleObservation::ServiceStopObserved {
        role: ServiceRole::PrimaryService,
        raw_reason: "sigterm".into(),
        shutdown_in_progress: true,
        explicit_user_stop: false,
        detected_by: "scenario-test".into(),
    });

    scenario
        .assert_lifecycle_observations_contain("service_stop_observed")
        .assert_lifecycle_observations_contain("sigterm")
        .assert_lifecycle_observations_contain("scenario-test");

    scenario.shutdown().assert_is_running(false);
}
