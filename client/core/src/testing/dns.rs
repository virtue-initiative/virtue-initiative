use std::collections::{HashMap, HashSet, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Condvar, Mutex};

use crate::module::dns_filter::{DnsCheckRunner, DnsProbe, HandshakeError};

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
    /// While true, every lookup waits, like a resolver that never answers.
    lookups_held: Arc<(Mutex<bool>, Condvar)>,
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

    /// Makes every lookup from now on wait until
    /// [`release_lookups`](Self::release_lookups), like a resolver that
    /// never answers. The lookup is recorded before it waits.
    pub fn hold_lookups(&self) {
        *self.lookups_held.0.lock().unwrap() = true;
    }

    /// Lets waiting and later lookups answer again.
    pub fn release_lookups(&self) {
        *self.lookups_held.0.lock().unwrap() = false;
        self.lookups_held.1.notify_all();
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
        let (held, released) = &*self.lookups_held;
        let _released = released
            .wait_while(held.lock().unwrap(), |held| *held)
            .unwrap();
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

type Job = Box<dyn FnOnce() + Send>;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum RunMode {
    #[default]
    Inline,
    Hold,
    Threads,
}

#[derive(Default)]
struct RunnerState {
    mode: RunMode,
    held: VecDeque<Job>,
    started: usize,
}

/// Test [`DnsCheckRunner`]. Runs each check inline by default, so the tick
/// that starts a check also records its outcome. [`hold`](Self::hold) keeps
/// checks waiting instead, to stand in for a check that is still running,
/// and [`on_threads`](Self::on_threads) runs them the way production does.
///
/// Cheap to clone; clones share the mode and the held checks.
#[derive(Clone, Default)]
pub struct FakeDnsCheckRunner {
    state: Arc<Mutex<RunnerState>>,
}

impl FakeDnsCheckRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep each check that starts from now on until
    /// [`finish_oldest`](Self::finish_oldest) runs it.
    pub fn hold(&self) {
        self.state.lock().unwrap().mode = RunMode::Hold;
    }

    /// Run each check that starts from now on on a thread of its own.
    pub fn on_threads(&self) {
        self.state.lock().unwrap().mode = RunMode::Threads;
    }

    /// Runs the oldest held check to its end. Returns false when none is held.
    pub fn finish_oldest(&self) -> bool {
        let job = self.state.lock().unwrap().held.pop_front();
        job.map(|job| job()).is_some()
    }

    /// How many checks have started so far.
    pub fn started(&self) -> usize {
        self.state.lock().unwrap().started
    }
}

impl DnsCheckRunner for FakeDnsCheckRunner {
    fn run(&self, job: Job) {
        let mode = {
            let mut state = self.state.lock().unwrap();
            state.started += 1;
            if state.mode == RunMode::Hold {
                state.held.push_back(job);
                return;
            }
            state.mode
        };
        match mode {
            RunMode::Threads => {
                std::thread::spawn(job);
            }
            _ => job(),
        }
    }
}
