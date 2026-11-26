//! DNS filter check (CORE-029): resolves the owner's check domain through the
//! OS resolver, confirms a normal-looking answer with a TLS handshake, and
//! reports when their DNS filter stops blocking it. Report only; nothing here
//! blocks or changes DNS traffic.

use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection};

use serde::{Deserialize, Serialize};

use crate::model::UploadKind;
use crate::module::lifecycle::HIGH_RISK_LIFECYCLE_ALERT;
use crate::module::upload::{self, UploadState};
use crate::rng::RandomSource;

/// Domains no DNS filter should block, run by different operators, tried in
/// order (CORE-029). When none is reachable, the network is down or behind a
/// captive portal, and the check domain's answer says nothing.
pub const CONTROL_DOMAINS: [&str; 5] = [
    "virtueinitiative.org",
    "apple.com",
    "microsoft.com",
    "google.com",
    "one.one.one.one",
];
/// How long a TLS connection may take, from connect to finished handshake.
const TLS_TIMEOUT: Duration = Duration::from_secs(5);
/// How many addresses of one answer are tried, so a network that drops every
/// connection holds a check up for at most this many timeouts per domain.
const MAX_ADDRESSES_TRIED: usize = 2;
/// Mean, shortest and longest delay between checks (CORE-029).
const CHECK_DELAY_MEAN_MS: f64 = 20.0 * 60.0 * 1_000.0;
pub(crate) const CHECK_DELAY_MIN_MS: i64 = 60 * 1_000;
pub(crate) const CHECK_DELAY_MAX_MS: i64 = 60 * 60 * 1_000;
pub(crate) const UNKNOWN_RETRY_MS: i64 = 10 * 60 * 1_000;

/// The delay until the next check for a uniform draw `u` in `[0, 1)`
/// (CORE-029): exponentially distributed, so the chance of a check in the
/// next minute does not depend on how long ago the last one was, then
/// clamped so checks are neither back to back nor more than an hour apart.
pub(crate) fn check_delay_ms(u: f64) -> i64 {
    let u = u.clamp(0.0, 1.0 - f64::EPSILON);
    let delay = -(1.0 - u).ln() * CHECK_DELAY_MEAN_MS;
    (delay as i64).clamp(CHECK_DELAY_MIN_MS, CHECK_DELAY_MAX_MS)
}

/// The network as the check sees it: name lookups and TLS handshakes.
/// Injectable so tests can script both; production uses [`SystemDnsProbe`].
pub trait DnsProbe: Send + Sync {
    /// Resolves a host name to its addresses.
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String>;

    /// Connects to `addr` on port 443 and completes a TLS handshake with
    /// `host` as the server name, succeeding only when the certificate is
    /// valid for `host`.
    fn tls_handshake(&self, host: &str, addr: IpAddr) -> Result<(), String>;
}

/// Resolves through the operating system's resolver (`getaddrinfo` and its
/// equivalents), which is the path a DNS filter set on the device controls,
/// and verifies certificates the way the API client does.
pub struct SystemDnsProbe;

impl DnsProbe for SystemDnsProbe {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        (host, 443)
            .to_socket_addrs()
            .map(|addrs| addrs.map(|addr| addr.ip()).collect())
            .map_err(|err| err.to_string())
    }

    fn tls_handshake(&self, host: &str, addr: IpAddr) -> Result<(), String> {
        let config = tls_config()?;
        let name = ServerName::try_from(host.to_string()).map_err(|err| err.to_string())?;
        let deadline = Instant::now() + TLS_TIMEOUT;
        let mut tcp = TcpStream::connect_timeout(&SocketAddr::new(addr, 443), TLS_TIMEOUT)
            .map_err(|err| format!("connect: {err}"))?;
        let mut tls = ClientConnection::new(config, name).map_err(|err| err.to_string())?;
        while tls.is_handshaking() {
            let left = deadline
                .checked_duration_since(Instant::now())
                .filter(|left| !left.is_zero())
                .ok_or("handshake timed out")?;
            tcp.set_read_timeout(Some(left))
                .and_then(|()| tcp.set_write_timeout(Some(left)))
                .map_err(|err| err.to_string())?;
            match tls.complete_io(&mut tcp) {
                Ok((0, 0)) => return Err("connection closed during the handshake".into()),
                Ok(_) => {}
                Err(err) => return Err(format!("handshake: {err}")),
            }
        }
        Ok(())
    }
}

/// The TLS client configuration, built once. It trusts what the API client
/// trusts (see `HttpApiClient::new`): the OS certificate store, or bundled
/// Mozilla roots on Android.
fn tls_config() -> Result<Arc<ClientConfig>, String> {
    static CONFIG: OnceLock<Result<Arc<ClientConfig>, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let builder = ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|err| err.to_string())?;
            #[cfg(not(target_os = "android"))]
            let config = {
                use rustls_platform_verifier::BuilderVerifierExt;
                builder
                    .with_platform_verifier()
                    .map_err(|err| err.to_string())?
                    .with_no_client_auth()
            };
            #[cfg(target_os = "android")]
            let config = builder
                .with_root_certificates(rustls::RootCertStore {
                    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
                })
                .with_no_client_auth();
            Ok(Arc::new(config))
        })
        .clone()
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

/// What [`execute`] found; `result` is `None` when the answer is unknown
/// because no control domain was reachable.
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
            // Risk 80%: the server already emailed partners (API-020, API-030).
            upload::enqueue(
                upload,
                now_ms,
                HIGH_RISK_LIFECYCLE_ALERT,
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

/// Tries the control domains and then the check domain. The only I/O here.
pub fn execute(plan: DnsCheckPlan, probe: &dyn DnsProbe) -> DnsCheckOutcome {
    let network_works = CONTROL_DOMAINS
        .iter()
        .any(|control| reach(probe, control) == Reach::Reachable);
    let result = if !network_works {
        tracing::warn!("dns filter check: no control domain is reachable, result unknown");
        None
    } else {
        match reach(probe, &plan.domain) {
            Reach::Reachable => Some(DnsFilterResult::Unfiltered),
            Reach::Blocked => Some(DnsFilterResult::Filtered),
            Reach::NoValidCertificate => {
                tracing::info!(
                    "dns filter check: check domain answered without a valid certificate, \
                     as a block page does; counted as filtered"
                );
                Some(DnsFilterResult::Filtered)
            }
        }
    };
    DnsCheckOutcome {
        domain: plan.domain,
        result,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Reach {
    /// The lookup failed, or returned only unspecified or loopback addresses,
    /// which is how DNS filters answer a blocked name.
    Blocked,
    /// No address tried showed a valid certificate for the domain.
    NoValidCertificate,
    /// An address showed a valid certificate: the real site answered.
    Reachable,
}

/// Looks `host` up and tries a TLS handshake with its first few addresses,
/// never connecting to an unspecified or loopback one (CORE-029).
fn reach(probe: &dyn DnsProbe, host: &str) -> Reach {
    let Ok(addrs) = probe.resolve(host) else {
        return Reach::Blocked;
    };
    let mut candidates = addrs
        .into_iter()
        .filter(|addr| !addr.is_unspecified() && !addr.is_loopback())
        .take(MAX_ADDRESSES_TRIED)
        .peekable();
    if candidates.peek().is_none() {
        return Reach::Blocked;
    }
    let verified = candidates.any(|addr| match probe.tls_handshake(host, addr) {
        Ok(()) => true,
        Err(err) => {
            tracing::debug!("dns filter check: {host} at {addr}: {err}");
            false
        }
    });
    if verified {
        Reach::Reachable
    } else {
        Reach::NoValidCertificate
    }
}

/// Records the outcome, sends an event when the result changed, and schedules
/// the next check.
pub fn commit(
    state: &mut DnsFilterState,
    upload: &mut UploadState,
    outcome: DnsCheckOutcome,
    now_ms: i64,
    rng: &dyn RandomSource,
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
            HIGH_RISK_LIFECYCLE_ALERT,
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
    state.next_check_at_ms = now_ms + check_delay_ms(rng.uniform());
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
    use crate::rng::OsRandomSource;

    const MINUTE_MS: i64 = 60 * 1_000;

    // CORE-029: the delay is exponential with a mean of 20 minutes, clamped
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

    // CORE-029: each delay is drawn anew, so delays differ between checks and
    // stay within the clamp.
    #[test]
    fn each_check_delay_is_drawn_anew() {
        let delays: Vec<i64> = (0..200)
            .map(|_| check_delay_ms(OsRandomSource.uniform()))
            .collect();
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
