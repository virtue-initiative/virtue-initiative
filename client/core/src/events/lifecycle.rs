use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{Event, HIGH_RISK_LIFECYCLE_ALERT, SERVICE_PING_GRACE_MS, SERVICE_PING_INTERVAL_MS};
use crate::crypto::prepare_log_batch_event;
use crate::error::CoreResult;
use crate::lifecycle::{
    CapturePermissionState, LifecycleObservation, LifecycleOrigin, LifecycleStatus,
    LifecycleTransition, ServicePingLog, ServiceStopMarker, UserSessionState, apply_observation,
};
use crate::model::{EventData, LogEntry};

const STOP_ALERT_THRESHOLD_MS: i64 = 10_000;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct LifecycleObserverState {
    pub lifecycle: LifecycleStatus,
    /// Transitions produced by the most recently processed observation, so the
    /// service can read them back after `iter()` for platform callers.
    #[serde(default)]
    pub last_transitions: Vec<LifecycleTransition>,
    #[serde(default)]
    pub service_stop_markers: HashMap<String, ServiceStopMarker>,
    #[serde(default)]
    pub service_last_pings: HashMap<String, ServicePingLog>,
}

pub struct LifecycleObserver {
    pub state: LifecycleObserverState,
}

impl LifecycleObserver {
    pub fn new(state: LifecycleObserverState) -> Self {
        Self { state }
    }

    pub(super) fn on_event(&mut self, event: &Event, _now_ms: i64) -> CoreResult<Vec<Event>> {
        match event {
            Event::LifecycleObserved {
                observation,
                now_ms,
                is_authenticated,
            } => self.process_observation(observation, *now_ms, *is_authenticated),
            _ => Ok(vec![]),
        }
    }

    /// Fold a lifecycle observation into state, storing the resulting
    /// transitions in `state.last_transitions` and returning the upload events
    /// that should be queued (ImmediateUpload / BatchUpload).
    fn process_observation(
        &mut self,
        observation: &LifecycleObservation,
        now_ms: i64,
        is_authenticated: bool,
    ) -> CoreResult<Vec<Event>> {
        let result = apply_observation(&self.state.lifecycle, observation);
        self.state.lifecycle = result.status;
        let base_ts = match observation {
            LifecycleObservation::BootObserved {
                booted_at_ms: Some(ts),
                ..
            } => *ts,
            _ => now_ms,
        };
        let mut events = Vec::new();
        for (index, transition) in result.transitions.iter().enumerate() {
            let ts = base_ts.saturating_add(index as i64);
            events.push(lifecycle_transition_to_batch_event(transition, ts)?);
        }
        let effect_events = self.handle_observation_effects(
            observation,
            &result.transitions,
            now_ms,
            is_authenticated,
        )?;
        events.extend(effect_events);
        self.state.last_transitions = result.transitions;
        Ok(events)
    }

    fn handle_observation_effects(
        &mut self,
        observation: &LifecycleObservation,
        transitions: &[LifecycleTransition],
        now_ms: i64,
        is_authenticated: bool,
    ) -> CoreResult<Vec<Event>> {
        let mut events = Vec::new();
        match observation {
            LifecycleObservation::ServiceStopObserved { role, .. } => {
                let Some(service_transition) = transitions
                    .iter()
                    .find(|t| t.service_role == Some(*role) && t.to == "stopped")
                else {
                    return Ok(events);
                };
                let marker = ServiceStopMarker {
                    role: *role,
                    origin: service_transition.origin,
                    stopped_at_ms: now_ms,
                };
                self.state
                    .service_stop_markers
                    .insert(role.as_str().to_string(), marker);
                if is_authenticated && service_transition.origin == LifecycleOrigin::UserRequested {
                    let mut data = EventData::default();
                    data.insert(
                        "alert_reason",
                        serde_json::Value::String("user_initiated_stop".to_string()),
                    );
                    data.insert(
                        "service_role",
                        serde_json::Value::String(role.as_str().to_string()),
                    );
                    data.insert(
                        "origin",
                        serde_json::Value::String(service_transition.origin.as_str().to_string()),
                    );
                    data.insert(
                        "detected_by",
                        serde_json::Value::String(service_transition.detected_by.clone()),
                    );
                    events.push(lifecycle_alert_to_event(
                        now_ms,
                        HIGH_RISK_LIFECYCLE_ALERT,
                        data,
                    )?);
                }
            }
            LifecycleObservation::ServiceStarted { role, detected_by } => {
                let Some(_) = transitions
                    .iter()
                    .find(|t| t.service_role == Some(*role) && t.to == "running")
                else {
                    return Ok(events);
                };
                let marker = self.state.service_stop_markers.remove(role.as_str());
                if !is_authenticated {
                    return Ok(events);
                }
                match marker {
                    None => match self.state.service_last_pings.get(role.as_str()) {
                        Some(last_ping)
                            if now_ms.saturating_sub(last_ping.pinged_at_ms)
                                > SERVICE_PING_INTERVAL_MS + SERVICE_PING_GRACE_MS =>
                        {
                            let ping_gap_ms = now_ms.saturating_sub(last_ping.pinged_at_ms);
                            let mut data = EventData::default();
                            data.insert(
                                "alert_reason",
                                serde_json::Value::String(
                                    "missing_stop_marker_after_ping_gap".to_string(),
                                ),
                            );
                            data.insert(
                                "service_role",
                                serde_json::Value::String(role.as_str().to_string()),
                            );
                            data.insert(
                                "detected_by",
                                serde_json::Value::String(detected_by.clone()),
                            );
                            data.insert("ping_gap_ms", serde_json::Value::from(ping_gap_ms));
                            data.insert(
                                "ping_threshold_ms",
                                serde_json::Value::from(
                                    SERVICE_PING_INTERVAL_MS + SERVICE_PING_GRACE_MS,
                                ),
                            );
                            events.push(lifecycle_alert_to_event(
                                now_ms,
                                HIGH_RISK_LIFECYCLE_ALERT,
                                data,
                            )?);
                        }
                        Some(_) => {}
                        None => {
                            let mut data = EventData::default();
                            data.insert(
                                "alert_reason",
                                serde_json::Value::String("missing_stop_marker".to_string()),
                            );
                            data.insert(
                                "service_role",
                                serde_json::Value::String(role.as_str().to_string()),
                            );
                            data.insert(
                                "detected_by",
                                serde_json::Value::String(detected_by.clone()),
                            );
                            events.push(lifecycle_alert_to_event(
                                now_ms,
                                HIGH_RISK_LIFECYCLE_ALERT,
                                data,
                            )?);
                        }
                    },
                    Some(marker)
                        if marker.origin == LifecycleOrigin::UserRequested
                            || marker.origin == LifecycleOrigin::SystemShutdown => {}
                    Some(marker) => {
                        let downtime_ms = now_ms.saturating_sub(marker.stopped_at_ms);
                        if downtime_ms > STOP_ALERT_THRESHOLD_MS {
                            let mut data = EventData::default();
                            data.insert(
                                "alert_reason",
                                serde_json::Value::String("extended_service_stop".to_string()),
                            );
                            data.insert(
                                "service_role",
                                serde_json::Value::String(role.as_str().to_string()),
                            );
                            data.insert(
                                "origin",
                                serde_json::Value::String(marker.origin.as_str().to_string()),
                            );
                            data.insert("downtime_ms", serde_json::Value::from(downtime_ms));
                            data.insert(
                                "threshold_ms",
                                serde_json::Value::from(STOP_ALERT_THRESHOLD_MS),
                            );
                            data.insert(
                                "detected_by",
                                serde_json::Value::String(detected_by.clone()),
                            );
                            events.push(lifecycle_alert_to_event(
                                now_ms,
                                HIGH_RISK_LIFECYCLE_ALERT,
                                data,
                            )?);
                        }
                    }
                }
            }
            LifecycleObservation::UserSessionChanged {
                state,
                origin,
                detected_by,
            } => {
                let Some(session_transition) = transitions
                    .iter()
                    .find(|t| t.domain.as_str() == "user_session" && t.to == state.as_str())
                else {
                    return Ok(events);
                };
                if is_authenticated && *state == UserSessionState::LoggedOut {
                    let mut data = EventData::default();
                    data.insert(
                        "alert_reason",
                        serde_json::Value::String("user_session_logout".to_string()),
                    );
                    data.insert(
                        "origin",
                        serde_json::Value::String(origin.as_str().to_string()),
                    );
                    data.insert(
                        "detected_by",
                        serde_json::Value::String(detected_by.clone()),
                    );
                    data.insert(
                        "from",
                        serde_json::Value::String(session_transition.from.clone()),
                    );
                    data.insert(
                        "to",
                        serde_json::Value::String(session_transition.to.clone()),
                    );
                    events.push(lifecycle_alert_to_event(
                        now_ms,
                        HIGH_RISK_LIFECYCLE_ALERT,
                        data,
                    )?);
                }
            }
            LifecycleObservation::CapturePermissionChanged { state, detected_by } => {
                let Some(permission_transition) = transitions
                    .iter()
                    .find(|t| t.domain.as_str() == "capture_permission" && t.to == state.as_str())
                else {
                    return Ok(events);
                };
                if !is_authenticated {
                    return Ok(events);
                }
                let mut data = EventData::default();
                data.insert(
                    "alert_reason",
                    serde_json::Value::String("capture_permission_changed".to_string()),
                );
                data.insert(
                    "detected_by",
                    serde_json::Value::String(detected_by.clone()),
                );
                data.insert(
                    "from",
                    serde_json::Value::String(permission_transition.from.clone()),
                );
                data.insert(
                    "to",
                    serde_json::Value::String(permission_transition.to.clone()),
                );
                events.push(lifecycle_alert_to_event(
                    now_ms,
                    capture_permission_alert_risk(&permission_transition.from, *state),
                    data,
                )?);
            }
            _ => {}
        }
        Ok(events)
    }
}

fn lifecycle_transition_to_batch_event(
    transition: &LifecycleTransition,
    ts: i64,
) -> CoreResult<Event> {
    let log = lifecycle_transition_log(transition, ts);
    let data = prepare_log_batch_event(log.ts, &log.kind, log.risk, log.data)?;
    Ok(Event::BatchUpload { data })
}

fn lifecycle_transition_log(transition: &LifecycleTransition, ts: i64) -> LogEntry {
    let mut data = EventData::default();
    data.insert(
        "domain",
        serde_json::Value::String(transition.domain.as_str().to_string()),
    );
    data.insert("from", serde_json::Value::String(transition.from.clone()));
    data.insert("to", serde_json::Value::String(transition.to.clone()));
    data.insert(
        "origin",
        serde_json::Value::String(transition.origin.as_str().to_string()),
    );
    data.insert(
        "detected_by",
        serde_json::Value::String(transition.detected_by.clone()),
    );
    data.insert(
        "confidence",
        serde_json::Value::String(transition.confidence.as_str().to_string()),
    );
    if let Some(role) = transition.service_role {
        data.insert(
            "service_role",
            serde_json::Value::String(role.as_str().to_string()),
        );
    }
    LogEntry {
        ts,
        kind: "lifecycle_transition".to_string(),
        risk: Some(transition.risk),
        data,
    }
}

fn lifecycle_alert_to_event(ts: i64, risk: f32, data: EventData) -> CoreResult<Event> {
    let log = LogEntry {
        ts,
        kind: "lifecycle_alert".to_string(),
        risk: Some(risk),
        data,
    };
    if risk >= HIGH_RISK_LIFECYCLE_ALERT {
        Ok(Event::ImmediateUpload { entry: log })
    } else {
        let buffered = prepare_log_batch_event(log.ts, &log.kind, log.risk, log.data)?;
        Ok(Event::BatchUpload { data: buffered })
    }
}

fn capture_permission_alert_risk(from: &str, to: CapturePermissionState) -> f32 {
    if from == CapturePermissionState::Granted.as_str() && to != CapturePermissionState::Granted {
        HIGH_RISK_LIFECYCLE_ALERT
    } else {
        0.2
    }
}
