use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use crate::module::dns_filter::DnsResolver;

type Answer = Result<Vec<IpAddr>, String>;

/// Test [`DnsResolver`]: serves a scripted answer per host and records every
/// lookup. A host with no scripted answer fails to resolve, like `NXDOMAIN`.
///
/// Cheap to clone; clones share the script and the recorded lookups.
#[derive(Clone, Default)]
pub struct FakeDnsResolver {
    answers: Arc<Mutex<HashMap<String, Answer>>>,
    lookups: Arc<Mutex<Vec<String>>>,
}

impl FakeDnsResolver {
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

    /// Every host looked up so far, in order.
    pub fn lookups(&self) -> Vec<String> {
        self.lookups.lock().unwrap().clone()
    }
}

impl DnsResolver for FakeDnsResolver {
    fn resolve(&self, host: &str) -> Answer {
        self.lookups.lock().unwrap().push(host.to_string());
        self.answers
            .lock()
            .unwrap()
            .get(host)
            .cloned()
            .unwrap_or_else(|| Err(format!("{host}: no answer scripted")))
    }
}
