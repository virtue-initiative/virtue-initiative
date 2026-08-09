//! End-to-end load generator for a running `hash-server` instance.
//!
//! Per-device traffic (`POST /hash`) hits the server over plain HTTP,
//! authenticated with a `device-cert` JWT plus a per-request Ed25519
//! signature (see `src/auth.rs`'s `verify_signature`) — this is the
//! TLS-handshake-sensitive path the device-cert scheme exists to unblock.
//! Server-only traffic (`DELETE /hash`, the merged `GET /hash` info burst)
//! stays on HTTPS with the old unsigned `server`-typed bearer token, hitting
//! `--secure-url` instead of `--url`, since a real device never originates
//! either of those calls. See the "Performance testing" section of
//! `hash-server/README.md` for how to configure and run this.
//!
//! Latency stats are NOT tracked per simulated device: at device counts in
//! the hundreds of thousands to millions, one `hdrhistogram::Histogram` per
//! device task would allocate far more memory than the tasks themselves (a
//! histogram is sized by value range, not sample count). Instead, device and
//! group tasks send small `(latency, success)` messages over a channel to a
//! handful of shared aggregator tasks that own the histograms.
//!
//! ## Per-device memory model
//!
//! Per-device traffic does NOT spawn one `tokio::task::JoinHandle` per
//! device — a prior version did, and at 500k simulated devices that model's
//! per-task stack/scheduler overhead measured out to roughly 7.9GB, which
//! made ramping toward the eventual 1M-device target impractical on typical
//! hardware. Instead, every device's state (id, device-cert token, signing
//! key, server token, and next-due timestamps for its two actions) lives as
//! one entry in a flat `Arc<[DeviceState]>` — a single contiguous
//! allocation instead of a million independently-scheduled tasks. A single
//! scheduler task scans that slice once per tick (cheap even at 1M entries:
//! a tick over 1M atomic loads is a few milliseconds), finds devices whose
//! `POST`/`DELETE` action is due, and pushes `(device_index, action)` pairs
//! onto an `mpsc` channel. A fixed pool of `--workers` worker tasks pulls
//! from that channel, signs and sends the request, records into the same
//! `Recorder`/`Stats` aggregators used before, then reschedules that
//! device's next-due timestamp. The scheduler optimistically claims a
//! device (setting its next-due to a sentinel) before enqueuing it, so a
//! slow in-flight request doesn't get redundantly re-enqueued on the next
//! tick; if the channel is full the claim is released so the device is
//! retried on a later tick instead of being stuck claimed forever.
//! Per-device random phase offsets (computed once at construction, same as
//! before) keep the initial fleet from bursting in lockstep. The
//! per-user-group `GET /hash` info-burst simulation is unaffected by this —
//! it was already one task per user group, not per device.

use base64::Engine;
use clap::Parser;
use ed25519_dalek::{
    pkcs8::{EncodePrivateKey, EncodePublicKey},
    Signer, SigningKey,
};
use hdrhistogram::Histogram;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use pkcs8::LineEnding;
use rand::Rng;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{interval_at, sleep_until, Instant, MissedTickBehavior};

#[derive(Parser, Debug)]
#[command(about = "Load-test a running hash-server instance over plain HTTP")]
struct Args {
    /// Base URL of the running hash-server instance, for per-device traffic
    /// (POST /hash) — plain HTTP, since that's the whole point of the
    /// device-cert scheme this load test exercises.
    #[arg(long, default_value = "http://localhost:3000")]
    url: String,

    /// HTTPS base URL for server-only traffic (DELETE /hash, the merged GET
    /// /hash info burst) — these never originate from a real device, so
    /// they stay on TLS with the old unsigned server-token scheme instead of
    /// moving to the device-cert path. Defaults to --url with its scheme
    /// swapped to https.
    #[arg(long)]
    secure_url: Option<String>,

    /// Devices per simulated user (real-world fleets are typically 2-3).
    #[arg(long, default_value_t = 2)]
    devices_per_user: usize,

    /// Number of independent simulated users/fleets.
    #[arg(long, default_value_t = 250)]
    users: usize,

    /// Real-world seconds between a device's POST /hash pings, before --time-scale.
    #[arg(long, default_value_t = 300)]
    post_interval_secs: u64,

    /// Real-world seconds between a device's DELETE /hash resets, before --time-scale.
    #[arg(long, default_value_t = 3600)]
    reset_interval_secs: u64,

    /// Real-world seconds between a user's browser-session INFO bursts, before --time-scale.
    #[arg(long, default_value_t = 1800)]
    info_session_interval_secs: u64,

    /// Divides all interval flags above by this factor to compress cadence into a short run.
    #[arg(long, default_value_t = 60)]
    time_scale: u64,

    /// How long to run the load test for.
    #[arg(long, default_value_t = 120)]
    duration_secs: u64,

    /// Size of the fixed worker-task pool that signs and sends per-device
    /// POST/DELETE requests, draining a scheduler-fed queue instead of one
    /// task per device (see the module doc comment).
    #[arg(long, default_value_t = 256)]
    workers: usize,
}

/// How often the scheduler task scans the full device slice for due
/// actions. A tick over even 1M devices is a few milliseconds of atomic
/// loads, so this can stay well below the shortest realistic interval
/// without the scan itself becoming a bottleneck.
const SCHEDULER_TICK: Duration = Duration::from_millis(100);

/// One aggregator task per endpoint owns the (memory-heavy) histogram, so
/// per-device tasks stay tiny no matter how many devices are simulated.
struct Recorder {
    tx: mpsc::Sender<(u64, bool)>,
}

impl Recorder {
    fn record(&self, elapsed: Duration, success: bool) {
        // try_send, not send: under extreme load a full channel should drop
        // a stats sample rather than slow down (or block) the request loop
        // and skew the very latency numbers we're trying to measure.
        let _ = self.tx.try_send((elapsed.as_micros() as u64, success));
    }
}

struct Stats {
    hist: Histogram<u64>,
    total: u64,
    errors: u64,
}

impl Stats {
    fn new() -> Self {
        Stats {
            // 2 significant figures keeps the histogram's backing array
            // small; at device counts in the millions this is spawned
            // exactly 3 times (post/delete/info) so it doesn't matter for
            // memory, but there's no reason to pay for more precision than
            // we report.
            hist: Histogram::new(2).unwrap(),
            total: 0,
            errors: 0,
        }
    }
}

fn spawn_recorder(capacity: usize) -> (Recorder, tokio::task::JoinHandle<Stats>) {
    let (tx, mut rx) = mpsc::channel::<(u64, bool)>(capacity);
    let handle = tokio::spawn(async move {
        let mut stats = Stats::new();
        while let Some((micros, success)) = rx.recv().await {
            stats.total += 1;
            if !success {
                stats.errors += 1;
            }
            let _ = stats.hist.record(micros);
        }
        stats
    });
    (Recorder { tx }, handle)
}

fn make_token(enc: &EncodingKey, typ: &str, sub: &str, exp: u64) -> String {
    #[derive(Serialize)]
    struct Claims<'a> {
        sub: &'a str,
        #[serde(rename = "type")]
        typ: &'a str,
        exp: u64,
    }
    encode(
        &Header::new(Algorithm::EdDSA),
        &Claims { sub, typ, exp },
        enc,
    )
    .unwrap()
}

/// Mints a `device-cert`-typed token embedding the device's pubkey, mirroring
/// what api/'s `buildDeviceState` mints in remote-hash-server mode.
fn make_device_cert_token(enc: &EncodingKey, sub: &str, pubkey: &[u8; 32], exp: u64) -> String {
    #[derive(Serialize)]
    struct Claims<'a> {
        sub: &'a str,
        #[serde(rename = "type")]
        typ: &'a str,
        pubkey: &'a str,
        exp: u64,
    }
    let pubkey_b64 = base64::engine::general_purpose::STANDARD.encode(pubkey);
    encode(
        &Header::new(Algorithm::EdDSA),
        &Claims { sub, typ: "device-cert", pubkey: &pubkey_b64, exp },
        enc,
    )
    .unwrap()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Signs a `POST /hash` request. Byte layout must stay identical to
/// `src/auth.rs`'s `verify_signature` and `client/core/src/crypto.rs`'s
/// `sign_request`: `timestamp_ms (i64 LE) || device_id || 0x00 || method ||
/// 0x00 || path || 0x00 || body`.
fn sign_request(
    signing_key: &SigningKey,
    timestamp_ms: i64,
    device_id: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> String {
    let mut msg = Vec::with_capacity(
        8 + device_id.len() + 1 + method.len() + 1 + path.len() + 1 + body.len(),
    );
    msg.extend_from_slice(&timestamp_ms.to_le_bytes());
    msg.extend_from_slice(device_id.as_bytes());
    msg.push(0);
    msg.extend_from_slice(method.as_bytes());
    msg.push(0);
    msg.extend_from_slice(path.as_bytes());
    msg.push(0);
    msg.extend_from_slice(body);
    let sig = signing_key.sign(&msg);
    base64::engine::general_purpose::STANDARD.encode(sig.to_bytes())
}

/// Deterministic per-device Ed25519 identity, distinct from the fixed
/// `[42u8; 32]` seed reserved for the simulated server's own JWT-signing key.
fn device_identity_signing_key(i: usize) -> SigningKey {
    let seed: [u8; 32] = Sha256::digest(format!("loadtest-device-identity-{i}")).into();
    SigningKey::from_bytes(&seed)
}

/// One simulated device's identity and per-action scheduling state. Lives as
/// one entry in a flat `Arc<[DeviceState]>` — see the module doc comment.
struct DeviceState {
    device_id: String,
    cert_token: String,
    signing_key: SigningKey,
    server_token: String,
    /// Next-due timestamp (ms since epoch) for POST /hash. Set to
    /// `CLAIMED_SENTINEL` by the scheduler between claiming a due device
    /// and its worker finishing the request and rescheduling it.
    next_post_due_ms: AtomicI64,
    /// Same as `next_post_due_ms`, for DELETE /hash.
    next_reset_due_ms: AtomicI64,
}

/// Marks a device's action as claimed (enqueued, not yet completed), so the
/// scheduler's next tick doesn't see it as still-due and enqueue it again
/// while a request for it is already in flight.
const CLAIMED_SENTINEL: i64 = i64::MAX;

#[derive(Clone, Copy)]
enum Action {
    Post,
    Reset,
}

/// Scans the full device slice once per `SCHEDULER_TICK` and enqueues due
/// actions. Cheap even at 1M devices — see the module doc comment.
async fn run_scheduler(
    devices: Arc<[DeviceState]>,
    tx: mpsc::Sender<(usize, Action)>,
    deadline: Instant,
) {
    let mut tick = tokio::time::interval(SCHEDULER_TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = sleep_until(deadline) => break,
            _ = tick.tick() => {
                let now = now_ms();
                for (idx, device) in devices.iter().enumerate() {
                    if device.next_post_due_ms.load(Ordering::Relaxed) <= now {
                        device.next_post_due_ms.store(CLAIMED_SENTINEL, Ordering::Relaxed);
                        if tx.try_send((idx, Action::Post)).is_err() {
                            // Queue full — release the claim so this device
                            // is retried on a later tick instead of getting
                            // stuck claimed forever.
                            device.next_post_due_ms.store(now, Ordering::Relaxed);
                        }
                    }
                    if device.next_reset_due_ms.load(Ordering::Relaxed) <= now {
                        device.next_reset_due_ms.store(CLAIMED_SENTINEL, Ordering::Relaxed);
                        if tx.try_send((idx, Action::Reset)).is_err() {
                            device.next_reset_due_ms.store(now, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    }
}

/// Pulls `(device_index, action)` pairs off the scheduler's queue, signs and
/// sends the request, records into the shared aggregators, then reschedules
/// that device's next-due timestamp. A fixed pool of these replaces the
/// former one-task-per-device model — see the module doc comment.
///
/// `rx` is shared (behind a mutex) across the whole worker pool rather than
/// each worker owning an independent channel: with a single scheduler
/// producer, a multi-consumer work queue is exactly what's needed, and
/// `tokio::sync::mpsc::Receiver` isn't `Clone` / doesn't support multiple
/// consumers natively.
#[allow(clippy::too_many_arguments)]
async fn run_worker(
    rx: Arc<tokio::sync::Mutex<mpsc::Receiver<(usize, Action)>>>,
    client: reqwest::Client,
    server_client: reqwest::Client,
    base_url: Arc<str>,
    secure_base_url: Arc<str>,
    devices: Arc<[DeviceState]>,
    post_interval_ms: i64,
    reset_interval_ms: i64,
    post_rec: Arc<Recorder>,
    delete_rec: Arc<Recorder>,
) {
    loop {
        let next = rx.lock().await.recv().await;
        let Some((idx, action)) = next else { break };
        let device = &devices[idx];
        match action {
            Action::Post => {
                let mut payload = [0u8; 32];
                rand::thread_rng().fill(&mut payload);

                let timestamp_ms = now_ms();
                let signature = sign_request(
                    &device.signing_key,
                    timestamp_ms,
                    &device.device_id,
                    "POST",
                    "/hash",
                    &payload,
                );

                let start = Instant::now();
                let result = client
                    .post(format!("{base_url}/hash"))
                    .bearer_auth(&device.cert_token)
                    .header("X-Signature-Timestamp", timestamp_ms.to_string())
                    .header("X-Signature", signature)
                    .body(payload.to_vec())
                    .send()
                    .await;
                match result {
                    Ok(resp) => post_rec.record(start.elapsed(), resp.status().is_success()),
                    Err(_) => post_rec.record(start.elapsed(), false),
                }
                device
                    .next_post_due_ms
                    .store(now_ms() + post_interval_ms, Ordering::Relaxed);
            }
            Action::Reset => {
                // Server-only, unsigned, TLS-fronted — see module docs.
                let start = Instant::now();
                let result = server_client
                    .delete(format!("{secure_base_url}/hash"))
                    .bearer_auth(&device.server_token)
                    .send()
                    .await;
                match result {
                    Ok(resp) => delete_rec.record(start.elapsed(), resp.status().is_success()),
                    Err(_) => delete_rec.record(start.elapsed(), false),
                }
                device
                    .next_reset_due_ms
                    .store(now_ms() + reset_interval_ms, Ordering::Relaxed);
            }
        }
    }
}

async fn run_user_group_task(
    server_client: reqwest::Client,
    secure_base_url: Arc<str>,
    devices: Arc<[Arc<str>]>,
    info_interval: Duration,
    deadline: Instant,
    info_phase: Duration,
    info_rec: Arc<Recorder>,
) {
    let mut info_iv = interval_at(Instant::now() + info_phase, info_interval);
    info_iv.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let sleep = sleep_until(deadline);
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            _ = &mut sleep => break,
            _ = info_iv.tick() => {
                let mut set = JoinSet::new();
                for token in devices.iter() {
                    let client = server_client.clone();
                    let base_url = secure_base_url.clone();
                    let token = token.clone();
                    set.spawn(async move {
                        let start = Instant::now();
                        // Merged into GET /hash (see decision #3 in the
                        // device-cert plan) — server-only, unsigned, still
                        // returns JSON ({state, count, hashed_at,
                        // updated_at} now, vs. the old {count, hashed_at,
                        // updated_at}-only shape). We only care about
                        // success/latency here, not the parsed body.
                        let result = client
                            .get(format!("{base_url}/hash"))
                            .bearer_auth(&token)
                            .send()
                            .await;
                        match result {
                            Ok(resp) => (start.elapsed(), resp.status().is_success()),
                            Err(_) => (start.elapsed(), false),
                        }
                    });
                }
                while let Some(result) = set.join_next().await {
                    if let Ok((elapsed, success)) = result {
                        info_rec.record(elapsed, success);
                    }
                }
            }
        }
    }
}

fn print_row(name: &str, stats: &Stats, duration_secs: u64) {
    let hist = &stats.hist;
    if stats.total == 0 {
        println!(
            "{name:<16} {:>10} {:>8} {:>9} {:>10} {:>10} {:>10} {:>10} {:>10}",
            0, 0, "0.0", "-", "-", "-", "-", "-"
        );
        return;
    }
    let req_per_sec = stats.total as f64 / duration_secs as f64;
    let ms = |v: u64| v as f64 / 1000.0;
    println!(
        "{name:<16} {:>10} {:>8} {:>9.1} {:>10.2} {:>10.2} {:>10.2} {:>10.2} {:>10.2}",
        stats.total,
        stats.errors,
        req_per_sec,
        ms(hist.min()),
        ms(hist.value_at_quantile(0.5)),
        ms(hist.value_at_quantile(0.95)),
        ms(hist.value_at_quantile(0.99)),
        ms(hist.max()),
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.time_scale == 0 {
        anyhow::bail!("--time-scale must be >= 1");
    }

    // Fixed seed so the signing key is deterministic across runs, matching
    // the pattern in src/routes.rs's `make_test_state` test helper.
    let signing_key = SigningKey::from_bytes(&[42u8; 32]);
    let verifying_key = signing_key.verifying_key();
    let enc_key = EncodingKey::from_ed_der(signing_key.to_pkcs8_der().unwrap().as_bytes());
    let pub_pem = verifying_key.to_public_key_pem(LineEnding::LF).unwrap();

    println!("Set this as JWT_PUBLIC_KEY in hash-server/.env before starting the server:");
    println!("{pub_pem}");

    let base_url: Arc<str> = Arc::from(args.url.trim_end_matches('/'));
    let secure_base_url: Arc<str> = Arc::from(
        args.secure_url
            .clone()
            .unwrap_or_else(|| match args.url.strip_prefix("http://") {
                Some(rest) => format!("https://{rest}"),
                None => args.url.clone(),
            })
            .trim_end_matches('/')
            .to_string(),
    );
    let total_devices = args.users * args.devices_per_user;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let exp = now + args.duration_secs + 3600;

    let post_interval =
        Duration::from_secs_f64(args.post_interval_secs as f64 / args.time_scale as f64);
    let reset_interval =
        Duration::from_secs_f64(args.reset_interval_secs as f64 / args.time_scale as f64);
    let info_interval =
        Duration::from_secs_f64(args.info_session_interval_secs as f64 / args.time_scale as f64);

    // Each device's state (identity, tokens, next-due timestamps) lives as
    // one entry in this flat slice — see the module doc comment. Per-device
    // random phase offsets are computed once here, same as before.
    let mint_start = Instant::now();
    let mut rng = rand::thread_rng();
    let start_now_ms = now_ms();
    let mut devices: Vec<DeviceState> = Vec::with_capacity(total_devices);
    // The per-user-group INFO-burst task (unaffected by the worker-pool
    // rewrite) needs its own chunked view of server tokens, independent of
    // the per-device DeviceState slice the worker pool drains.
    let mut server_tokens: Vec<Arc<str>> = Vec::with_capacity(total_devices);
    for i in 0..total_devices {
        let device_id = format!("loadtest-device-{i:07}");
        let device_signing_key = device_identity_signing_key(i);
        let device_pubkey = device_signing_key.verifying_key().to_bytes();
        let cert_token = make_device_cert_token(&enc_key, &device_id, &device_pubkey, exp);
        let server_token = make_token(&enc_key, "server", &device_id, exp);
        server_tokens.push(Arc::from(server_token.as_str()));

        let post_phase_ms = (rng.gen::<f64>() * post_interval.as_secs_f64() * 1000.0) as i64;
        let reset_phase_ms = (rng.gen::<f64>() * reset_interval.as_secs_f64() * 1000.0) as i64;

        devices.push(DeviceState {
            device_id,
            cert_token,
            signing_key: device_signing_key,
            server_token,
            next_post_due_ms: AtomicI64::new(start_now_ms + post_phase_ms),
            next_reset_due_ms: AtomicI64::new(start_now_ms + reset_phase_ms),
        });
    }
    let devices: Arc<[DeviceState]> = Arc::from(devices.into_boxed_slice());
    println!(
        "Minted {} device-cert tokens + {} server tokens ({} device identity keys) in {:.1}s",
        total_devices,
        total_devices,
        total_devices,
        mint_start.elapsed().as_secs_f64()
    );

    println!(
        "Simulating {} users x {} devices = {} devices against {} (secure: {}) for {}s (time-scale {}x)",
        args.users,
        args.devices_per_user,
        total_devices,
        base_url,
        secure_base_url,
        args.duration_secs,
        args.time_scale
    );
    println!(
        "Scaled intervals: POST every {:.2}s, DELETE every {:.2}s, INFO burst every {:.2}s",
        post_interval.as_secs_f64(),
        reset_interval.as_secs_f64(),
        info_interval.as_secs_f64()
    );

    // Real devices are physically separate machines, so in production every
    // ping is a brand-new TCP+TLS connection -- there's no shared connection
    // pool to warm up across devices the way there would be if this process
    // reused connections. Disabling keep-alive reuse here means we pay the
    // real TLS handshake cost on every request, matching production.
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .build()?;
    // DELETE/GET (info) are server-to-server calls that in production come
    // from a single Cloudflare Worker, which does reuse a connection pool --
    // unlike per-device traffic, forcing a fresh TLS handshake per call here
    // would overstate their cost and add spurious load to the target.
    let server_client = reqwest::Client::builder().build()?;
    let deadline = Instant::now() + Duration::from_secs(args.duration_secs);

    let (post_rec, post_handle) = spawn_recorder(65_536);
    let (delete_rec, delete_handle) = spawn_recorder(65_536);
    let (info_rec, info_handle) = spawn_recorder(65_536);
    let post_rec = Arc::new(post_rec);
    let delete_rec = Arc::new(delete_rec);
    let info_rec = Arc::new(info_rec);

    // Scheduler + fixed worker pool drain the device slice built above,
    // replacing the former one-task-per-device model — see the module doc
    // comment. Queue capacity is generous relative to the worker count so a
    // burst of simultaneously-due devices doesn't immediately trip the
    // scheduler's queue-full claim-release path under normal load.
    let (action_tx, action_rx) = mpsc::channel::<(usize, Action)>((args.workers * 64).max(4096));
    let worker_rx = Arc::new(tokio::sync::Mutex::new(action_rx));
    let post_interval_ms = post_interval.as_millis() as i64;
    let reset_interval_ms = reset_interval.as_millis() as i64;

    let scheduler_handle = tokio::spawn(run_scheduler(devices.clone(), action_tx, deadline));

    let mut worker_handles = Vec::with_capacity(args.workers);
    for _ in 0..args.workers {
        worker_handles.push(tokio::spawn(run_worker(
            worker_rx.clone(),
            client.clone(),
            server_client.clone(),
            base_url.clone(),
            secure_base_url.clone(),
            devices.clone(),
            post_interval_ms,
            reset_interval_ms,
            post_rec.clone(),
            delete_rec.clone(),
        )));
    }

    let mut rng = rand::thread_rng();
    let mut group_handles = Vec::with_capacity(args.users);
    for chunk in server_tokens.chunks(args.devices_per_user) {
        let group_tokens: Arc<[Arc<str>]> = Arc::from(chunk.to_vec().into_boxed_slice());
        let info_phase = Duration::from_secs_f64(rng.gen::<f64>() * info_interval.as_secs_f64());
        group_handles.push(tokio::spawn(run_user_group_task(
            server_client.clone(),
            secure_base_url.clone(),
            group_tokens,
            info_interval,
            deadline,
            info_phase,
            info_rec.clone(),
        )));
    }

    scheduler_handle.await?;
    for handle in worker_handles {
        handle.await?;
    }
    for handle in group_handles {
        handle.await?;
    }

    // Dropping the last Arc<Recorder> clone closes each channel, which lets
    // the aggregator tasks drain remaining messages and return.
    drop(post_rec);
    drop(delete_rec);
    drop(info_rec);

    let agg_post = post_handle.await?;
    let agg_delete = delete_handle.await?;
    let agg_info = info_handle.await?;

    println!();
    println!(
        "{:<16} {:>10} {:>8} {:>9} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "Endpoint",
        "Requests",
        "Errors",
        "Req/s",
        "Min(ms)",
        "p50(ms)",
        "p95(ms)",
        "p99(ms)",
        "Max(ms)"
    );
    print_row("POST /hash", &agg_post, args.duration_secs);
    print_row("DELETE /hash", &agg_delete, args.duration_secs);
    print_row("GET /hash (info)", &agg_info, args.duration_secs);

    Ok(())
}
