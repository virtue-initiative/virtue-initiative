//! The daemon's own clock (CORE-027): the server's time, carried forward on
//! the platform's boot clock, so setting the system clock doesn't move it.

/// A response's time closer than this to the clock is left alone: `Date` is
/// only accurate to about a second, so following every response would just
/// make the clock jitter.
const SYNC_TOLERANCE_MS: i64 = 2_000;

/// A correction bigger than this is reported (CORE-027).
pub(crate) const CORRECTION_ALERT_MS: i64 = 5 * 60_000;

#[derive(Clone, Copy, Debug)]
pub(crate) struct DaemonClock {
    /// The time at `boot_ms` on the boot clock.
    utc_ms: i64,
    boot_ms: i64,
    /// Whether `utc_ms` came from a server response, not the system clock.
    synced: bool,
}

impl DaemonClock {
    /// Starts from the system clock, but no earlier than the last tick's
    /// time, `last_tick_ms`, so a system clock set back can't move the
    /// daemon's time before one it has already recorded. A clock set back
    /// further is corrected, and reported, by the first response.
    pub fn start(system_ms: i64, boot_ms: i64, last_tick_ms: Option<i64>) -> Self {
        Self {
            utc_ms: last_tick_ms.map_or(system_ms, |tick_ms| system_ms.max(tick_ms)),
            boot_ms,
            synced: false,
        }
    }

    pub fn now_ms(&self, boot_ms: i64) -> i64 {
        self.utc_ms + (boot_ms - self.boot_ms)
    }

    /// Moves the clock to `server_ms`, read when the boot clock was at
    /// `boot_ms`: always on the first response, after that only when it's
    /// more than [`SYNC_TOLERANCE_MS`] off. Returns how far it moved.
    pub fn sync(&mut self, server_ms: i64, boot_ms: i64) -> i64 {
        let correction_ms = server_ms - self.now_ms(boot_ms);
        if self.synced && correction_ms.abs() <= SYNC_TOLERANCE_MS {
            return 0;
        }
        *self = Self {
            utc_ms: server_ms,
            boot_ms,
            synced: true,
        };
        correction_ms
    }
}

/// The `alert` message for a correction of `correction_ms` (CORE-027).
pub(crate) fn correction_message(correction_ms: i64) -> String {
    let direction = if correction_ms > 0 { "behind" } else { "ahead" };
    format!(
        "This device's clock was {} {direction} until it reached the server, so times it recorded before then may be off by that much.",
        format_duration(correction_ms.abs())
    )
}

/// The two largest whole units of `ms`, e.g. "2 hours 5 minutes".
fn format_duration(ms: i64) -> String {
    let minutes = ms / 60_000;
    let units = [
        (minutes / 1_440, "day"),
        (minutes / 60 % 24, "hour"),
        (minutes % 60, "minute"),
    ];
    let parts: Vec<String> = units
        .iter()
        .skip_while(|(count, _)| *count == 0)
        .take(2)
        .filter(|(count, _)| *count > 0)
        .map(|(count, unit)| {
            let plural = if *count == 1 { "" } else { "s" };
            format!("{count} {unit}{plural}")
        })
        .collect();
    if parts.is_empty() {
        "less than a minute".to_string()
    } else {
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // CORE-027: with no tick yet, the clock starts from the system clock
    // and follows the boot clock.
    #[test]
    fn starts_from_the_system_clock_and_follows_the_boot_clock() {
        let clock = DaemonClock::start(1_000_000, 50, None);
        assert_eq!(clock.now_ms(50), 1_000_000);
        assert_eq!(clock.now_ms(10_050), 1_010_000);
    }

    // CORE-027: after a tick, the clock starts from the system clock, but no
    // earlier than the last tick.
    #[test]
    fn starts_no_earlier_than_the_last_tick() {
        let last_tick = Some(5_000_000);
        assert_eq!(
            DaemonClock::start(1_000_000, 0, last_tick).now_ms(0),
            5_000_000
        );
        assert_eq!(
            DaemonClock::start(9_000_000, 0, last_tick).now_ms(0),
            9_000_000
        );
    }

    // CORE-027: the first response always sets the clock, and a later one
    // only when it's more than 2 seconds off.
    #[test]
    fn follows_responses_that_are_more_than_two_seconds_off() {
        let mut clock = DaemonClock::start(1_000_000, 0, None);
        assert_eq!(clock.sync(1_000_500, 0), 500);
        assert_eq!(clock.now_ms(1_000), 1_001_500);

        assert_eq!(clock.sync(1_003_000, 1_000), 0);
        assert_eq!(clock.now_ms(1_000), 1_001_500);

        assert_eq!(clock.sync(998_000, 1_000), -3_500);
        assert_eq!(clock.now_ms(2_000), 999_000);
    }

    // CORE-027: the alert says how far off the clock was, and which way.
    #[test]
    fn correction_message_names_the_size_and_direction() {
        assert_eq!(
            correction_message(2 * 3_600_000 + 5 * 60_000 + 30_000),
            "This device's clock was 2 hours 5 minutes behind until it reached the server, so times it recorded before then may be off by that much."
        );
        assert!(correction_message(-6 * 60_000).contains("was 6 minutes ahead"));
        assert!(correction_message(-3 * 86_400_000 - 60_000).contains("was 3 days ahead"));
        assert!(correction_message(86_400_000 + 3_600_000).contains("was 1 day 1 hour behind"));
    }
}
