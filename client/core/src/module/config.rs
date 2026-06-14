use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::CoreResult;
use crate::events::Ping;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChanged {
    pub api_base_url: String,
    pub screenshot_interval_ms: u64,
    pub batch_interval_ms: u64,
}

pub struct ConfigModule {
    config: Config,
}

impl ConfigModule {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl Observer for ConfigModule {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "config"
    }

    fn init(&mut self, _bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
        Ok(())
    }

    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
        crate::dispatch_event!(event, {
            _: Ping => {
                let old_url = self.config.api_base_url.clone();
                let old_screenshot = self.config.screenshot_interval;
                let old_batch = self.config.batch_interval;

                self.config.refresh_from_runtime_file()?;

                let changed = self.config.api_base_url != old_url
                    || self.config.screenshot_interval != old_screenshot
                    || self.config.batch_interval != old_batch;

                if changed {
                    let _ = emitter.send(ConfigChanged {
                        api_base_url: self.config.api_base_url.clone(),
                        screenshot_interval_ms: self.config.screenshot_interval.as_millis() as u64,
                        batch_interval_ms: self.config.batch_interval.as_millis() as u64,
                    });
                }
                Ok(())
            },
        })
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(StateType::Null)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use super::{ConfigChanged, ConfigModule};
    use crate::config::Config;
    use crate::events::Ping;
    use crate::testing::EventTester;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn config_module_emits_config_changed_on_ping_when_override_changes_interval() {
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

        let mut b = EventTester::builder();
        b.capture::<ConfigChanged>();
        b.add(ConfigModule::new(config));
        let mut t = b.build();

        // No override yet: ping should not emit ConfigChanged
        t.emit(1, Ping);
        assert!(
            t.captured::<ConfigChanged>().is_empty(),
            "no change expected before override file exists"
        );

        // Write override with new screenshot interval
        fs::write(&override_file, r#"{"capture_interval_seconds": 15}"#).unwrap();
        t.emit(2, Ping);

        let events = t.captured::<ConfigChanged>();
        assert_eq!(events.len(), 1, "expected one ConfigChanged after override");
        assert_eq!(
            events[0].screenshot_interval_ms, 15_000,
            "screenshot interval should be updated to 15s"
        );

        let _ = fs::remove_dir_all(dir);
    }
}
