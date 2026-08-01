use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::Ping;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::model::UploadKind;
use crate::module::auth::{Login, Logout};
use crate::module::upload::Upload;
use crate::platform::ScreenshotHooks;

pub(crate) const HEARTBEAT_INTERVAL_MS: i64 = 24 * 60 * 60 * 1_000;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct HeartbeatObserverState {
    pub last_heartbeat_ms: i64,
    #[serde(default)]
    pub authenticated: bool,
}

pub struct HeartbeatModule {
    pub state: HeartbeatObserverState,
    platform: Box<dyn ScreenshotHooks>,
    authenticated: bool,
}

impl HeartbeatModule {
    pub fn new(platform: Box<dyn ScreenshotHooks>) -> Self {
        Self {
            state: HeartbeatObserverState::default(),
            platform,
            authenticated: false,
        }
    }
}

impl Observer for HeartbeatModule {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "heartbeat"
    }

    fn init(&mut self, _bus: &mut EventBus, state: StateType) -> CoreResult<()> {
        if !state.is_null() {
            self.state = serde_json::from_value(state)?;
            self.authenticated = self.state.authenticated;
        }
        Ok(())
    }

    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
        crate::dispatch_event!(event, {
            _: Login => {
                self.authenticated = true;
                self.state.authenticated = true;
                Ok(())
            },
            _: Logout => {
                self.authenticated = false;
                self.state.authenticated = false;
                Ok(())
            },
            _: Ping => {
                if !self.authenticated {
                    return Ok(());
                }
                let now_ms = self.platform.get_time_utc_ms()?;
                if now_ms - self.state.last_heartbeat_ms >= HEARTBEAT_INTERVAL_MS {
                    self.state.last_heartbeat_ms = now_ms;
                    let _ = emitter.send(Upload {
                        risk: 0.0,
                        kind: UploadKind::Heartbeat,
                    });
                }
                Ok(())
            },
        })
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.state)?)
    }
}

#[cfg(test)]
mod tests {
    use super::{HEARTBEAT_INTERVAL_MS, HeartbeatModule};
    use crate::events::Ping;
    use crate::model::UploadKind;
    use crate::model::{BatchRecipient, DeviceCredentials, DeviceSettings};
    use crate::module::auth::{Login, Logout};
    use crate::module::upload::Upload;
    use crate::testing::EventTester;

    fn login_event() -> Login {
        Login {
            credentials: DeviceCredentials {
                device_id: "test-device".into(),
                refresh_token: "test-refresh".into(),
            },
            settings: DeviceSettings {
                device_id: "test-device".into(),
                name: "test device".into(),
                platform: "test".into(),
                wrapping_keys: vec![BatchRecipient {
                    user_id: "test-user".into(),
                    pub_key_base64: "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
                }],
                hash_base_url: None,
            },
        }
    }

    #[test]
    fn heartbeat_not_emitted_when_unauthenticated() {
        let mut b = EventTester::builder();
        b.add(HeartbeatModule::new(Box::new(b.platform())));
        let mut t = b.build();
        // Ping without login: no heartbeat
        t.emit(1, Ping);
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::Heartbeat,
            ..
        }));
    }

    // The mock clock starts near 0, so a 24h interval (86_400_000 ms) is never
    // reached from small test timestamps. Use a start time of 1 day + 1 second
    // so `now_ms - 0 >= HEARTBEAT_INTERVAL_MS` on the very first ping.
    const DAY_SECS: f64 = (HEARTBEAT_INTERVAL_MS / 1_000 + 1) as f64;

    #[test]
    fn first_ping_after_login_emits_heartbeat() {
        let mut b = EventTester::builder();
        b.add(HeartbeatModule::new(Box::new(b.platform())));
        let mut t = b.build();
        // `last_heartbeat_ms` defaults to 0, so any now_ms >= 24h from epoch triggers.
        t.emit(DAY_SECS - 2.0, login_event());
        t.emit(DAY_SECS, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Heartbeat,
            ..
        }));
    }

    #[test]
    fn second_ping_within_24h_does_not_emit_heartbeat() {
        let mut b = EventTester::builder();
        b.add(HeartbeatModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(DAY_SECS - 2.0, login_event());
        t.emit(DAY_SECS, Ping); // fires first heartbeat (last_heartbeat_ms was 0)
        t.clear_captured();
        // 1 hour later — still within 24h window since the last heartbeat
        t.emit(DAY_SECS + 3600.0, Ping);
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::Heartbeat,
            ..
        }));
    }

    #[test]
    fn ping_after_24h_emits_next_heartbeat() {
        let mut b = EventTester::builder();
        b.add(HeartbeatModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(DAY_SECS - 2.0, login_event());
        t.emit(DAY_SECS, Ping); // first heartbeat
        t.clear_captured();

        t.emit(DAY_SECS + DAY_SECS, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Heartbeat,
            ..
        }));
    }

    #[test]
    fn logout_stops_heartbeats() {
        let mut b = EventTester::builder();
        b.add(HeartbeatModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(DAY_SECS - 2.0, login_event());
        t.emit(DAY_SECS, Ping); // first heartbeat
        t.emit(DAY_SECS + 1.0, Logout);
        t.clear_captured();

        t.emit(DAY_SECS + DAY_SECS, Ping);
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::Heartbeat,
            ..
        }));
    }

    #[test]
    fn heartbeat_timer_persists_across_restarts() {
        let mut b = EventTester::builder();
        b.add(HeartbeatModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(DAY_SECS - 2.0, login_event());
        t.emit(DAY_SECS, Ping); // heartbeat fires
        t.clear_captured();

        let saved = t.bus.save().unwrap();

        // Simulate daemon restart: restore state
        let mut b2 = EventTester::builder();
        b2.add(HeartbeatModule::new(Box::new(b2.platform())));
        b2.with_state(saved);
        let mut t2 = b2.build();

        // Only a few seconds after the last heartbeat: should NOT fire again
        t2.emit(DAY_SECS + 10.0, login_event());
        t2.emit(DAY_SECS + 20.0, Ping);
        t2.assert_not_like(crate::like!(Upload {
            kind: UploadKind::Heartbeat,
            ..
        }));

        // After another 24h: should fire
        t2.clear_captured();
        t2.emit(DAY_SECS + DAY_SECS + 20.0, Ping);
        t2.assert_like(crate::like!(Upload {
            kind: UploadKind::Heartbeat,
            ..
        }));
    }

    #[test]
    fn authenticated_flag_persists_across_restart_without_relogin() {
        let mut b = EventTester::builder();
        b.add(HeartbeatModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(DAY_SECS - 2.0, login_event());
        t.emit(DAY_SECS, Ping); // heartbeat fires

        let saved = t.bus.save().unwrap();

        // Simulate daemon restart while still logged in: no Login event this time.
        let mut b2 = EventTester::builder();
        b2.add(HeartbeatModule::new(Box::new(b2.platform())));
        b2.with_state(saved);
        let mut t2 = b2.build();
        t2.clear_captured();

        t2.emit(DAY_SECS + DAY_SECS, Ping);
        t2.assert_like(crate::like!(Upload {
            kind: UploadKind::Heartbeat,
            ..
        }));
    }

    #[test]
    fn heartbeat_upload_has_zero_risk() {
        let mut b = EventTester::builder();
        b.add(HeartbeatModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(DAY_SECS - 2.0, login_event());
        t.emit(DAY_SECS, Ping);
        let uploads = t.captured::<Upload>();
        let hb = uploads
            .iter()
            .find(|u| matches!(u.kind, UploadKind::Heartbeat))
            .expect("heartbeat upload not found");
        assert_eq!(hb.risk, 0.0, "heartbeat should have zero risk");
    }
}
