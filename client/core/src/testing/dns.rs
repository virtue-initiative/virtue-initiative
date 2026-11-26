use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use crate::module::dns_filter::{DnsProbe, HandshakeError};

type Answer = Result<Vec<IpAddr>, String>;

/// Test [`DnsProbe`]: serves a scripted answer per host and records every
/// lookup and TLS handshake. A host with no scripted answer fails to resolve,
/// like `NXDOMAIN`. Every address of a host shows a valid certificate for it
/// unless [`fail_tls`](Self::fail_tls) or [`refuse_tls`](Self::refuse_tls)
/// says otherwise.
///
/// Cheap to clone; clones share the script and the records.
#[derive(Clone, Default)]
pub struct FakeDnsProbe {
    answers: Arc<Mutex<HashMap<String, Answer>>>,
    invalid_tls: Arc<Mutex<HashSet<String>>>,
    unreachable_tls: Arc<Mutex<HashSet<String>>>,
    lookups: Arc<Mutex<Vec<String>>>,
    handshakes: Arc<Mutex<Vec<(String, IpAddr)>>>,
}

impl FakeDnsProbe {
    pub fn new() -> Self {
        Self::default()
    }

    /// Script `host` to resolve to `addrs` (an empty slice is an empty answer).
    pub fn answer(&self, host: &str, addrs: &[&str]) {
        let addrs = addrs
            .iter()
            .map(|addr| addr.parse().expect("scripted address must parse"))
            .collect();
        self.answers
            .lock()
            .unwrap()
            .insert(host.to_string(), Ok(addrs));
    }

    /// Script `host` to fail to resolve.
    pub fn fail(&self, host: &str) {
        self.answers
            .lock()
            .unwrap()
            .insert(host.to_string(), Err(format!("{host}: no such host")));
    }

    /// Script every address of `host` to fail the TLS handshake, as a block
    /// page or captive portal does: it cannot show a valid certificate.
    pub fn fail_tls(&self, host: &str) {
        self.unreachable_tls.lock().unwrap().remove(host);
        self.invalid_tls.lock().unwrap().insert(host.to_string());
    }

    /// Script every address of `host` to answer nothing, as a flaky network
    /// or an address that refuses connections does: the handshake is refused,
    /// reset or times out.
    pub fn refuse_tls(&self, host: &str) {
        self.invalid_tls.lock().unwrap().remove(host);
        self.unreachable_tls
            .lock()
            .unwrap()
            .insert(host.to_string());
    }

    /// Script `host` to show a valid certificate again.
    pub fn serve_tls(&self, host: &str) {
        self.invalid_tls.lock().unwrap().remove(host);
        self.unreachable_tls.lock().unwrap().remove(host);
    }

    /// Every host looked up so far, in order.
    pub fn lookups(&self) -> Vec<String> {
        self.lookups.lock().unwrap().clone()
    }

    /// Every TLS handshake tried so far, as `(host, address)`, in order.
    pub fn handshakes(&self) -> Vec<(String, IpAddr)> {
        self.handshakes.lock().unwrap().clone()
    }
}

impl DnsProbe for FakeDnsProbe {
    fn resolve(&self, host: &str) -> Answer {
        self.lookups.lock().unwrap().push(host.to_string());
        self.answers
            .lock()
            .unwrap()
            .get(host)
            .cloned()
            .unwrap_or_else(|| Err(format!("{host}: no answer scripted")))
    }

    fn tls_handshake(&self, host: &str, addr: IpAddr) -> Result<(), HandshakeError> {
        self.handshakes
            .lock()
            .unwrap()
            .push((host.to_string(), addr));
        if self.invalid_tls.lock().unwrap().contains(host) {
            Err(HandshakeError::NoValidCertificate(format!(
                "{addr}: no valid certificate for {host}"
            )))
        } else if self.unreachable_tls.lock().unwrap().contains(host) {
            Err(HandshakeError::Unreachable(format!(
                "{addr}: connection refused"
            )))
        } else {
            Ok(())
        }
    }
}
