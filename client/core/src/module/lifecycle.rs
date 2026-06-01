use std::any::Any;
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::{Event, Observer, ProcessStoppedReason, StateType};
use crate::model::EventData;
use crate::platform::PlatformHooks;

pub(crate) const HIGH_RISK_LIFECYCLE_ALERT: f32 = 0.9;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct LifecycleObserverState {
    pub last_process_stopped_other: i64,
    pub last_process_stopped_shutdown: i64,
    pub last_process_stopped_user: i64,
    pub last_computer_suspend: i64,
    pub last_computer_resume: i64,
    pub last_ping: i64,
    pub last_process_started: i64,
}

pub struct LifecycleObserver {
    pub state: LifecycleObserverState,
    platform_hooks: Box<dyn PlatformHooks>,
    sender: Sender<Event>,
}

impl LifecycleObserver {
    pub fn new(platform_hooks: Box<dyn PlatformHooks>, sender: Sender<Event>) -> Self {
        Self {
            state: LifecycleObserverState::default(),
            platform_hooks,
            sender,
        }
    }

    fn compute_last_shutdown(&self) -> CoreResult<i64> {
        let stored = self.state.last_process_stopped_shutdown;
        if stored > 0 {
            return Ok(stored);
        }
        Ok(self
            .platform_hooks
            .get_last_shutdown_time_utc_ms()?
            .unwrap_or(0))
    }
}

impl Observer for LifecycleObserver {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "lifecycle"
    }

    fn save_state(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.state)?)
    }

    fn load_state(&mut self, state: StateType) -> CoreResult<()> {
        self.state = serde_json::from_value(state)?;
        Ok(())
    }

    fn on_event(&mut self, event: &Event) -> CoreResult<()> {
        let now_ms = self.platform_hooks.get_time_utc_ms()?;
        let last_shutdown = self.compute_last_shutdown()?;
        let old_last_ping = self.state.last_ping;

        // ALERT: process killed before shutdown (suspicious gap)
        if self.state.last_process_stopped_other > self.state.last_process_started
            && last_shutdown - self.state.last_process_stopped_other > 7000
        {
            self.sender
                .send(Event::Upload {
                    risk: 0.5,
                    kind: "lifecycle_alert".to_string(),
                    data: EventData::from_pairs([(
                        "alert_reason".to_string(),
                        "process_killed_before_shutdown".to_string(),
                    )]),
                })
                .ok();
            self.state.last_process_stopped_other = 0;
        }

        // Forward lifecycle events as batch log entries
        let lifecycle_kind = match event {
            Event::ProcessStarted => Some("process_started"),
            Event::ProcessStopped(_) => Some("process_stopped"),
            Event::ComputerSuspended => Some("computer_suspended"),
            Event::ComputerResumed => Some("computer_resumed"),
            Event::UserSessionChanged(_) => Some("user_session_changed"),
            _ => None,
        };
        if let Some(kind) = lifecycle_kind {
            self.sender
                .send(Event::Upload {
                    risk: 0.0,
                    kind: kind.to_string(),
                    data: EventData::default(),
                })
                .ok();
        }

        // Update state timestamps
        match event {
            Event::ProcessStopped(ProcessStoppedReason::Other) => {
                self.state.last_process_stopped_other = now_ms;
            }
            Event::ProcessStopped(ProcessStoppedReason::Shutdown) => {
                self.state.last_process_stopped_shutdown = now_ms;
            }
            Event::ProcessStopped(ProcessStoppedReason::User) => {
                self.state.last_process_stopped_user = now_ms;
            }
            Event::ProcessStarted => {
                self.state.last_process_started = now_ms;
            }
            Event::ComputerSuspended => {
                self.state.last_computer_suspend = now_ms;
            }
            Event::ComputerResumed => {
                self.state.last_computer_resume = now_ms;
            }
            Event::Ping => {
                self.state.last_ping = now_ms;
            }
            _ => {}
        }

        // ALERT: user explicitly stopped the process
        if matches!(event, Event::ProcessStopped(ProcessStoppedReason::User)) {
            self.sender
                .send(Event::Upload {
                    risk: HIGH_RISK_LIFECYCLE_ALERT,
                    kind: "lifecycle_alert".to_string(),
                    data: EventData::from_pairs([(
                        "alert_reason".to_string(),
                        "user_stopped_process".to_string(),
                    )]),
                })
                .ok();
        }

        // ALERT: ProcessStarted after suspicious gap (state.last_ping is old value here)
        if matches!(event, Event::ProcessStarted) {
            let boot_ms = self
                .platform_hooks
                .get_last_startup_time_utc_ms()?
                .unwrap_or(0);
            let ping_gap = if old_last_ping > 0 {
                now_ms - old_last_ping
            } else {
                i64::MAX
            };
            if ping_gap > 7000 && (now_ms - boot_ms) > 7000 {
                self.sender
                    .send(Event::Upload {
                        risk: HIGH_RISK_LIFECYCLE_ALERT,
                        kind: "lifecycle_alert".to_string(),
                        data: EventData::from_pairs([(
                            "alert_reason".to_string(),
                            "unexpected_process_start".to_string(),
                        )]),
                    })
                    .ok();
            }
        }

        // ALERT: Ping gap while computer was running (not sleeping)
        if matches!(event, Event::Ping) {
            let ping_gap = now_ms - old_last_ping;
            let resume_gap = now_ms - self.state.last_computer_resume;
            if old_last_ping > 0 && ping_gap > 7000 && resume_gap > 7000 {
                self.sender
                    .send(Event::Upload {
                        risk: HIGH_RISK_LIFECYCLE_ALERT,
                        kind: "lifecycle_alert".to_string(),
                        data: EventData::from_pairs([(
                            "alert_reason".to_string(),
                            "ping_gap_while_running".to_string(),
                        )]),
                    })
                    .ok();
            }
            // ALERT: last ping was after suspend started but no resume recorded
            if old_last_ping > self.state.last_computer_suspend + 1000
                && self.state.last_computer_resume < self.state.last_computer_suspend
            {
                self.sender
                    .send(Event::Upload {
                        risk: HIGH_RISK_LIFECYCLE_ALERT,
                        kind: "lifecycle_alert".to_string(),
                        data: EventData::from_pairs([(
                            "alert_reason".to_string(),
                            "ping_after_suspend".to_string(),
                        )]),
                    })
                    .ok();
            }
        }

        Ok(())
    }
}
