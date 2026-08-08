//! End-to-end load generator for a running `hash-server` instance.
//!
//! Hits a live server over plain HTTP with traffic modeled on the real
//! production pattern (per-device POST every capture interval, per-device
//! DELETE every batch window, per-user-group INFO bursts on session start).
//! See the "Performance testing" section of `hash-server/README.md` for how
//! to configure and run this.
//!
//! Note: this mints tokens with the `"device-access"` / `"server"` claim
//! values that `src/auth.rs` actually checks. The top-level repo `CLAUDE.md`
//! describes the JWT `type` claim as `"hash-server"` for this service, but
//! that's a pre-existing doc/code mismatch in `hash-server`, not a bug
//! introduced here.

use clap::Parser;
use ed25519_dalek::{
    pkcs8::{EncodePrivateKey, EncodePublicKey},
    SigningKey,
};
use hdrhistogram::Histogram;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use pkcs8::LineEnding;
use rand::Rng;
use serde::Serialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::task::JoinSet;
use tokio::time::{interval_at, sleep_until, Instant, MissedTickBehavior};

#[derive(Parser, Debug)]
#[command(about = "Load-test a running hash-server instance over plain HTTP")]
struct Args {
    /// Base URL of the running hash-server instance.
    #[arg(long, default_value = "http://localhost:3000")]
    url: String,

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
}

struct Stats {
    hist: Histogram<u64>,
    total: u64,
    errors: u64,
}

impl Stats {
    fn new() -> Self {
        Stats {
            hist: Histogram::new(3).unwrap(),
            total: 0,
            errors: 0,
        }
    }

    fn record(&mut self, elapsed: Duration, success: bool) {
        self.total += 1;
        if !success {
            self.errors += 1;
        }
        let _ = self.hist.record(elapsed.as_micros() as u64);
    }

    fn merge(&mut self, other: &Stats) {
        self.total += other.total;
        self.errors += other.errors;
        let _ = self.hist.add(&other.hist);
    }
}

struct DeviceStats {
    post: Stats,
    delete: Stats,
}

impl DeviceStats {
    fn new() -> Self {
        DeviceStats {
            post: Stats::new(),
            delete: Stats::new(),
        }
    }
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

#[allow(clippy::too_many_arguments)]
async fn run_device_task(
    client: reqwest::Client,
    base_url: String,
    access_token: String,
    server_token: String,
    post_interval: Duration,
    reset_interval: Duration,
    deadline: Instant,
    post_phase: Duration,
    reset_phase: Duration,
) -> DeviceStats {
    let mut stats = DeviceStats::new();

    let mut post_iv = interval_at(Instant::now() + post_phase, post_interval);
    post_iv.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut reset_iv = interval_at(Instant::now() + reset_phase, reset_interval);
    reset_iv.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let sleep = sleep_until(deadline);
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            _ = &mut sleep => break,
            _ = post_iv.tick() => {
                let mut payload = [0u8; 32];
                rand::thread_rng().fill(&mut payload);

                let start = Instant::now();
                let result = client
                    .post(format!("{base_url}/hash"))
                    .bearer_auth(&access_token)
                    .body(payload.to_vec())
                    .send()
                    .await;
                match result {
                    Ok(resp) => stats.post.record(start.elapsed(), resp.status().is_success()),
                    Err(_) => stats.post.record(start.elapsed(), false),
                }
            }
            _ = reset_iv.tick() => {
                let start = Instant::now();
                let result = client
                    .delete(format!("{base_url}/hash"))
                    .bearer_auth(&server_token)
                    .send()
                    .await;
                match result {
                    Ok(resp) => stats.delete.record(start.elapsed(), resp.status().is_success()),
                    Err(_) => stats.delete.record(start.elapsed(), false),
                }
            }
        }
    }

    stats
}

async fn run_user_group_task(
    client: reqwest::Client,
    base_url: String,
    devices: Vec<String>,
    info_interval: Duration,
    deadline: Instant,
    info_phase: Duration,
) -> Stats {
    let mut stats = Stats::new();

    let mut info_iv = interval_at(Instant::now() + info_phase, info_interval);
    info_iv.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let sleep = sleep_until(deadline);
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            _ = &mut sleep => break,
            _ = info_iv.tick() => {
                let mut set = JoinSet::new();
                for token in &devices {
                    let client = client.clone();
                    let base_url = base_url.clone();
                    let token = token.clone();
                    set.spawn(async move {
                        let start = Instant::now();
                        let result = client
                            .get(format!("{base_url}/hash/info"))
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
                        stats.record(elapsed, success);
                    }
                }
            }
        }
    }

    stats
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

    let base_url = args.url.trim_end_matches('/').to_string();
    let total_devices = args.users * args.devices_per_user;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let exp = now + args.duration_secs + 3600;

    let mut access_tokens = Vec::with_capacity(total_devices);
    let mut server_tokens = Vec::with_capacity(total_devices);
    for i in 0..total_devices {
        let device_id = format!("loadtest-device-{i:05}");
        access_tokens.push(make_token(&enc_key, "device-access", &device_id, exp));
        server_tokens.push(make_token(&enc_key, "server", &device_id, exp));
    }

    let post_interval =
        Duration::from_secs_f64(args.post_interval_secs as f64 / args.time_scale as f64);
    let reset_interval =
        Duration::from_secs_f64(args.reset_interval_secs as f64 / args.time_scale as f64);
    let info_interval =
        Duration::from_secs_f64(args.info_session_interval_secs as f64 / args.time_scale as f64);

    println!(
        "Simulating {} users x {} devices = {} devices against {} for {}s (time-scale {}x)",
        args.users,
        args.devices_per_user,
        total_devices,
        base_url,
        args.duration_secs,
        args.time_scale
    );
    println!(
        "Scaled intervals: POST every {:.2}s, DELETE every {:.2}s, INFO burst every {:.2}s",
        post_interval.as_secs_f64(),
        reset_interval.as_secs_f64(),
        info_interval.as_secs_f64()
    );

    let client = reqwest::Client::builder().build()?;
    let deadline = Instant::now() + Duration::from_secs(args.duration_secs);

    let mut rng = rand::thread_rng();

    let mut device_handles = Vec::with_capacity(total_devices);
    for i in 0..total_devices {
        let post_phase = Duration::from_secs_f64(rng.gen::<f64>() * post_interval.as_secs_f64());
        let reset_phase = Duration::from_secs_f64(rng.gen::<f64>() * reset_interval.as_secs_f64());
        device_handles.push(tokio::spawn(run_device_task(
            client.clone(),
            base_url.clone(),
            access_tokens[i].clone(),
            server_tokens[i].clone(),
            post_interval,
            reset_interval,
            deadline,
            post_phase,
            reset_phase,
        )));
    }

    let mut group_handles = Vec::with_capacity(args.users);
    for chunk in server_tokens.chunks(args.devices_per_user) {
        let info_phase = Duration::from_secs_f64(rng.gen::<f64>() * info_interval.as_secs_f64());
        group_handles.push(tokio::spawn(run_user_group_task(
            client.clone(),
            base_url.clone(),
            chunk.to_vec(),
            info_interval,
            deadline,
            info_phase,
        )));
    }

    let mut agg_post = Stats::new();
    let mut agg_delete = Stats::new();
    let mut agg_info = Stats::new();

    for handle in device_handles {
        let ds = handle.await?;
        agg_post.merge(&ds.post);
        agg_delete.merge(&ds.delete);
    }
    for handle in group_handles {
        let s = handle.await?;
        agg_info.merge(&s);
    }

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
    print_row("GET /hash/info", &agg_info, args.duration_secs);

    Ok(())
}
