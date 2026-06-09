use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::error::CoreResult;
use crate::events::bus::{EventBus, Observer, StateType};
use crate::events::types::{ConfigChanged, Ping};

struct ConfigInner {
    config: Config,
}

pub struct ConfigModule {
    inner: Arc<Mutex<ConfigInner>>,
}

impl ConfigModule {
    pub fn new(config: Config) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ConfigInner { config })),
        }
    }
}

impl Observer for ConfigModule {
    fn name(&self) -> &'static str {
        "config"
    }

    fn init(&mut self, bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
        let emitter = bus.emitter();

        let inner = Arc::clone(&self.inner);
        bus.subscribe(move |_: &Ping| {
            let mut g = inner.lock().unwrap();
            let old_url = g.config.api_base_url.clone();
            let old_screenshot = g.config.screenshot_interval;
            let old_batch = g.config.batch_interval;

            g.config.refresh_from_runtime_file()?;

            let changed = g.config.api_base_url != old_url
                || g.config.screenshot_interval != old_screenshot
                || g.config.batch_interval != old_batch;

            if changed {
                let _ = emitter.send(ConfigChanged {
                    api_base_url: g.config.api_base_url.clone(),
                    screenshot_interval_ms: g.config.screenshot_interval.as_millis() as u64,
                    batch_interval_ms: g.config.batch_interval.as_millis() as u64,
                });
            }
            Ok(())
        });

        Ok(())
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(StateType::Null)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::events::bus::EventBus;
    use crate::events::types::ConfigChanged;

    #[test]
    fn config_module_emits_config_changed_on_ping_when_override_changes_interval() {
        use std::fs;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "virtue-config-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let override_file = dir.join("config_override.json");

        let config = Config::new(
            "https://example.invalid",
            "test-device",
            "test-platform",
            dir.clone(),
            Some(override_file.clone()),
            Duration::from_secs(300),
            Duration::from_secs(3600),
        );

        let config_module = ConfigModule::new(config);

        let received = Arc::new(Mutex::new(Vec::<ConfigChanged>::new()));
        struct Capture {
            received: Arc<Mutex<Vec<ConfigChanged>>>,
        }
        impl Observer for Capture {
            fn init(&mut self, bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
                let r = Arc::clone(&self.received);
                bus.subscribe(move |ev: &ConfigChanged| {
                    r.lock().unwrap().push(ev.clone());
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

        let mut bus = EventBus::new(
            vec![
                Box::new(config_module),
                Box::new(Capture {
                    received: Arc::clone(&received),
                }),
            ],
            StateType::Null,
        )
        .unwrap();

        // No override yet: ping should not emit ConfigChanged
        bus.send(Ping).unwrap();
        bus.iter().unwrap();
        assert!(
            received.lock().unwrap().is_empty(),
            "no change expected before override file exists"
        );

        // Write override with new screenshot interval
        fs::write(&override_file, r#"{"capture_interval_seconds": 15}"#).unwrap();
        bus.send(Ping).unwrap();
        bus.iter().unwrap();

        let events = received.lock().unwrap();
        assert_eq!(events.len(), 1, "expected one ConfigChanged after override");
        assert_eq!(
            events[0].screenshot_interval_ms, 15_000,
            "screenshot interval should be updated to 15s"
        );

        let _ = fs::remove_dir_all(dir);
    }
}
