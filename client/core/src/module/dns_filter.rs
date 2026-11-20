//! DNS filter check (CORE-027): resolves the owner's check domain through the
//! OS resolver and reports when their DNS filter stops blocking it. Report
//! only; nothing here blocks or changes DNS traffic.

use std::net::{IpAddr, ToSocketAddrs};

use serde::{Deserialize, Serialize};

use crate::model::UploadKind;
use crate::module::lifecycle::EXTRA_HIGH_RISK;
use crate::module::upload::{self, UploadState};

/// A domain no DNS filter should block. When it does not resolve, the network
/// is down or broken, and the check domain's answer says nothing.
pub const CONTROL_DOMAIN: &str = "example.com";
/// Mean, shortest and longest delay between checks (CORE-027).
const CHECK_DELAY_MEAN_MS: f64 = 20.0 * 60.0 * 1_000.0;
pub(crate) const CHECK_DELAY_MIN_MS: i64 = 60 * 1_000;
pub(crate) const CHECK_DELAY_MAX_MS: i64 = 60 * 60 * 1_000;
pub(crate) const UNKNOWN_RETRY_MS: i64 = 10 * 60 * 1_000;

/// The delay until the next check for a uniform draw `u` in `[0, 1)`
/// (CORE-027): exponentially distributed, so the chance of a check in the
/// next minute does not depend on how long ago the last one was, then
/// clamped so checks are neither back to back nor more than an hour apart.
pub(crate) fn check_delay_ms(u: f64) -> i64 {
    let u = u.clamp(0.0, 1.0 - f64::EPSILON);
    let delay = -(1.0 - u).ln() * CHECK_DELAY_MEAN_MS;
    (delay as i64).clamp(CHECK_DELAY_MIN_MS, CHECK_DELAY_MAX_MS)
}

/// A uniform draw in `[0, 1)` from the OS RNG, so earlier delays do not
/// predict later ones.
fn uniform_draw() -> f64 {
    use rand_core::{OsRng, TryRngCore};
    let bits = OsRng.try_next_u64().expect("OS RNG unavailable") >> 11;
    bits as f64 / (1u64 << 53) as f64
}

/// Resolves a host name to its addresses. Injectable so tests can script
/// answers; production uses [`SystemDnsResolver`].
pub trait DnsResolver: Send + Sync {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String>;
}

/// Resolves through the operating system's resolver (`getaddrinfo` and its
/// equivalents), which is the path a DNS filter set on the device controls.
pub struct SystemDnsResolver;

impl DnsResolver for SystemDnsResolver {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        (host, 443)
            .to_socket_addrs()
            .map(|addrs| addrs.map(|addr| addr.ip()).collect())
            .map_err(|err| err.to_string())
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DnsFilterResult {
    /// The check domain is blocked: the filter is in use.
    Filtered,
    /// The check domain resolves normally: the filter is not in use.
    Unfiltered,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct DnsFilterState {
    /// The check domain `last_result` and `next_check_at_ms` belong to.
    pub domain: Option<String>,
    pub last_result: Option<DnsFilterResult>,
    pub next_check_at_ms: i64,
}

/// A check that is due, handed to [`execute`].
pub struct DnsCheckPlan {
    domain: String,
}

/// What [`execute`] found; `result` is `None` when the control domain did not
/// resolve, so the answer is unknown.
pub struct DnsCheckOutcome {
    domain: String,
    result: Option<DnsFilterResult>,
}

/// The check domain the settings name, or `None` when they name none.
/// Unchanged (`Err`) while settings have never been fetched, since then
/// nothing is known about the domain.
fn configured_domain(upload: &UploadState) -> Result<Option<&str>, ()> {
    let settings = upload.settings.as_ref().ok_or(())?;
    Ok(settings.dns_filter_domain.as_deref())
}

/// Brings `state` in line with the settings' domain: a removed domain sends
/// `dns_filter_check_removed`, a new or changed one forgets the last result
/// and makes a check due now. Returns a plan when a check is due.
pub fn plan(
    state: &mut DnsFilterState,
    upload: &mut UploadState,
    now_ms: i64,
) -> Option<DnsCheckPlan> {
    upload.device_credentials.as_ref()?;
    let configured = configured_domain(upload).ok()?.map(str::to_string);
    if configured != state.domain {
        if configured.is_none()
            && let Some(previous) = state.domain.take()
        {
            upload::enqueue(
                upload,
                now_ms,
                EXTRA_HIGH_RISK,
                UploadKind::DnsFilterCheckRemoved { domain: previous },
            );
        }
        *state = DnsFilterState {
            domain: configured,
            last_result: None,
            next_check_at_ms: now_ms,
        };
    }
    let domain = state.domain.clone()?;
    (now_ms >= state.next_check_at_ms).then_some(DnsCheckPlan { domain })
}

/// Resolves the control domain and then the check domain. The only I/O here.
pub fn execute(plan: DnsCheckPlan, resolver: &dyn DnsResolver) -> DnsCheckOutcome {
    let result = if is_blocked(resolver.resolve(CONTROL_DOMAIN)) {
        tracing::warn!("dns filter check: control domain did not resolve, result unknown");
        None
    } else if is_blocked(resolver.resolve(&plan.domain)) {
        Some(DnsFilterResult::Filtered)
    } else {
        Some(DnsFilterResult::Unfiltered)
    };
    DnsCheckOutcome {
        domain: plan.domain,
        result,
    }
}

/// A failed lookup, or an answer holding only unspecified or loopback
/// addresses, which is how DNS filters answer a blocked name.
fn is_blocked(answer: Result<Vec<IpAddr>, String>) -> bool {
    match answer {
        Ok(addrs) => addrs
            .iter()
            .all(|addr| addr.is_unspecified() || addr.is_loopback()),
        Err(_) => true,
    }
}

/// Records the outcome, sends an event when the result changed, and schedules
/// the next check.
pub fn commit(
    state: &mut DnsFilterState,
    upload: &mut UploadState,
    outcome: DnsCheckOutcome,
    now_ms: i64,
) {
    if state.domain.as_deref() != Some(outcome.domain.as_str()) {
        return;
    }
    let Some(result) = outcome.result else {
        state.next_check_at_ms = now_ms + UNKNOWN_RETRY_MS;
        return;
    };
    let event = match (state.last_result, result) {
        (Some(DnsFilterResult::Unfiltered), DnsFilterResult::Filtered) => Some((
            0.0,
            UploadKind::DnsFilterActive {
                domain: outcome.domain,
            },
        )),
        (None | Some(DnsFilterResult::Filtered), DnsFilterResult::Unfiltered) => Some((
            EXTRA_HIGH_RISK,
            UploadKind::DnsFilterInactive {
                domain: outcome.domain,
            },
        )),
        _ => None,
    };
    if let Some((risk, kind)) = event {
        upload::enqueue(upload, now_ms, risk, kind);
    }
    state.last_result = Some(result);
    state.next_check_at_ms = now_ms + check_delay_ms(uniform_draw());
}

/// When a check can next make progress (CORE-020): now when the settings'
/// domain differs from the one checked, else the scheduled check. `None`
/// while unauthenticated, before settings are fetched, or with no domain.
pub fn next_check_at_ms(state: &DnsFilterState, upload: &UploadState) -> Option<i64> {
    upload.device_credentials.as_ref()?;
    let configured = configured_domain(upload).ok()?;
    if configured != state.domain.as_deref() {
        return Some(i64::MIN);
    }
    state.domain.as_ref()?;
    Some(state.next_check_at_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE_MS: i64 = 60 * 1_000;

    // CORE-027: the delay is exponential with a mean of 20 minutes, clamped
    // to between 1 and 60 minutes.
    #[test]
    fn the_check_delay_is_exponential_and_clamped() {
        let cases: [(f64, i64); 6] = [
            (0.0, MINUTE_MS),
            // 1 - e^(-1/20) is the draw at which the delay reaches 1 minute.
            (0.03, MINUTE_MS),
            (1.0 - (-1.0f64).exp(), 20 * MINUTE_MS),
            (1.0 - (-2.0f64).exp(), 40 * MINUTE_MS),
            (0.99, 60 * MINUTE_MS),
            (1.0, 60 * MINUTE_MS),
        ];
        for (u, expected) in cases {
            let delay = check_delay_ms(u);
            assert!(
                (delay - expected).abs() <= 1,
                "u = {u}: expected {expected} ms, got {delay} ms"
            );
        }
    }

    // CORE-027: each delay is drawn anew, so delays differ between checks and
    // stay within the clamp.
    #[test]
    fn each_check_delay_is_drawn_anew() {
        let delays: Vec<i64> = (0..200).map(|_| check_delay_ms(uniform_draw())).collect();
        assert!(
            delays
                .iter()
                .all(|d| (CHECK_DELAY_MIN_MS..=CHECK_DELAY_MAX_MS).contains(d))
        );
        let mut distinct = delays.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert!(
            distinct.len() > 100,
            "only {} distinct delays",
            distinct.len()
        );
        let mean_min = delays.iter().sum::<i64>() as f64 / delays.len() as f64 / MINUTE_MS as f64;
        assert!(
            (12.0..=28.0).contains(&mean_min),
            "mean delay {mean_min:.1} minutes is far from 20"
        );
    }
}
