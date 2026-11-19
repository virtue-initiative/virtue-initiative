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
pub(crate) const CHECK_INTERVAL_MS: i64 = 60 * 60 * 1_000;
pub(crate) const UNKNOWN_RETRY_MS: i64 = 10 * 60 * 1_000;

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
    state.next_check_at_ms = now_ms + CHECK_INTERVAL_MS;
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
