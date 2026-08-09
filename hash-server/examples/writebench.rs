// Raw SQLite write-throughput microbenchmark: single-transaction-per-write
// (current hash-server design) vs batching many independent-device writes
// into one transaction. Same connect options (WAL, synchronous=NORMAL,
// busy_timeout=30s) and same read-then-upsert shape as db::update_hash_chain,
// so the numbers are directly comparable to production write cost.
//
// Usage: writebench --db-path /tmp/writebench.db --count 5000 --devices 1000 --batch-sizes 1,10,50,200,1000
use clap::Parser;
use sha2::Digest;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous};
use std::str::FromStr;
use std::time::{Duration, Instant};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "/tmp/writebench.db")]
    db_path: String,
    #[arg(long, default_value_t = 5000)]
    count: u64,
    #[arg(long, default_value_t = 1000)]
    devices: u64,
    #[arg(long, default_value = "1,10,50,200,1000")]
    batch_sizes: String,
}

async fn fresh_pool(db_path: &str) -> SqlitePool {
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{db_path}{ext}"));
    }
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{db_path}"))
        .unwrap()
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(30))
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await.unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS hash_states (
            device_id  TEXT    PRIMARY KEY,
            state      BLOB    NOT NULL,
            updated_at INTEGER NOT NULL,
            count      INTEGER NOT NULL DEFAULT 0,
            hashed_at  INTEGER
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

async fn one_write(tx: &mut sqlx::SqliteConnection, device_id: &str) {
    let current: Option<Vec<u8>> = sqlx::query_scalar("SELECT state FROM hash_states WHERE device_id = ?")
        .bind(device_id)
        .fetch_optional(&mut *tx)
        .await
        .unwrap();
    let current: [u8; 32] = current.and_then(|v| v.try_into().ok()).unwrap_or([0u8; 32]);
    let mut input = [0u8; 64];
    input[..32].copy_from_slice(&current);
    input[32..].copy_from_slice(&[7u8; 32]);
    let next: [u8; 32] = sha2::Sha256::digest(input).into();
    sqlx::query(
        "INSERT INTO hash_states (device_id, state, updated_at, count, hashed_at)
         VALUES (?, ?, 0, 1, 0)
         ON CONFLICT(device_id) DO UPDATE SET
             state = excluded.state, updated_at = excluded.updated_at,
             count = count + 1, hashed_at = excluded.hashed_at",
    )
    .bind(device_id)
    .bind(next.as_slice())
    .execute(&mut *tx)
    .await
    .unwrap();
}

async fn run_batch_size(pool: &SqlitePool, count: u64, devices: u64, batch_size: u64) {
    let mut done = 0u64;
    let mut next_device = 0u64;
    let start = Instant::now();
    let mut batch_latencies_ms: Vec<f64> = Vec::new();
    while done < count {
        let this_batch = batch_size.min(count - done);
        let batch_start = Instant::now();
        let mut tx = pool.acquire().await.unwrap();
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *tx).await.unwrap();
        for _ in 0..this_batch {
            let device_id = format!("device-{}", next_device % devices);
            next_device += 1;
            one_write(&mut tx, &device_id).await;
        }
        sqlx::query("COMMIT").execute(&mut *tx).await.unwrap();
        batch_latencies_ms.push(batch_start.elapsed().as_secs_f64() * 1000.0);
        done += this_batch;
    }
    let elapsed = start.elapsed();
    let ops_per_sec = count as f64 / elapsed.as_secs_f64();
    batch_latencies_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = batch_latencies_ms[batch_latencies_ms.len() / 2];
    let p99 = batch_latencies_ms[(batch_latencies_ms.len() * 99 / 100).min(batch_latencies_ms.len() - 1)];
    let max = *batch_latencies_ms.last().unwrap();
    let effective_per_write_p50 = p50 / batch_size as f64;
    println!(
        "batch_size={:<6} writes={:<7} elapsed={:>7.2}s  {:>9.1} writes/s   commit_latency p50={:>8.2}ms p99={:>8.2}ms max={:>8.2}ms   effective_per_write_p50={:>7.3}ms",
        batch_size, count, elapsed.as_secs_f64(), ops_per_sec, p50, p99, max, effective_per_write_p50
    );
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let batch_sizes: Vec<u64> = args.batch_sizes.split(',').map(|s| s.trim().parse().unwrap()).collect();

    println!("Write-throughput microbenchmark: db={} count={} devices={}", args.db_path, args.count, args.devices);
    println!();
    for batch_size in batch_sizes {
        let pool = fresh_pool(&args.db_path).await;
        run_batch_size(&pool, args.count, args.devices, batch_size).await;
        pool.close().await;
    }
}
