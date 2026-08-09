// All writes to hash_states go through a single dedicated SQLite connection
// (SQLite only ever allows one writer at a time anyway, so pooling multiple
// writer connections just creates lock-contention overhead). Requests queue
// here; whatever is immediately available gets committed together in one
// transaction, which amortizes fsync cost across many requests instead of
// paying it per-request. The queue is bounded: once full, callers get
// QueueFull immediately rather than piling up behind an ever-growing backlog.
use sha2::Digest;
use sqlx::sqlite::SqliteConnection;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

const MAX_BATCH: usize = 512;

enum Job {
    UpdateHashChain {
        device_id: String,
        new_hash: [u8; 32],
        respond: oneshot::Sender<Result<(), Arc<sqlx::Error>>>,
    },
    ResetHashState {
        device_id: String,
        respond: oneshot::Sender<Result<(), Arc<sqlx::Error>>>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("server is overloaded, try again shortly")]
    QueueFull,
    #[error("writer task is unavailable")]
    WriterGone,
    #[error("database error: {0}")]
    Db(#[from] Arc<sqlx::Error>),
}

#[derive(Clone)]
pub struct WriteHandle {
    tx: mpsc::Sender<Job>,
}

impl WriteHandle {
    pub async fn update_hash_chain(&self, device_id: &str, new_hash: [u8; 32]) -> Result<(), WriteError> {
        let (respond, rx) = oneshot::channel();
        self.tx
            .try_send(Job::UpdateHashChain { device_id: device_id.to_owned(), new_hash, respond })
            .map_err(|_| WriteError::QueueFull)?;
        rx.await.map_err(|_| WriteError::WriterGone)?.map_err(WriteError::from)
    }

    pub async fn reset_hash_state(&self, device_id: &str) -> Result<(), WriteError> {
        let (respond, rx) = oneshot::channel();
        self.tx
            .try_send(Job::ResetHashState { device_id: device_id.to_owned(), respond })
            .map_err(|_| WriteError::QueueFull)?;
        rx.await.map_err(|_| WriteError::WriterGone)?.map_err(WriteError::from)
    }
}

pub fn spawn(mut conn: SqliteConnection, queue_capacity: usize) -> WriteHandle {
    let (tx, mut rx) = mpsc::channel(queue_capacity);

    tokio::spawn(async move {
        while let Some(first) = rx.recv().await {
            let mut batch = vec![first];
            while batch.len() < MAX_BATCH {
                match rx.try_recv() {
                    Ok(job) => batch.push(job),
                    Err(_) => break,
                }
            }
            apply_batch(&mut conn, batch).await;
        }
    });

    WriteHandle { tx }
}

async fn apply_batch(conn: &mut SqliteConnection, batch: Vec<Job>) {
    if let Err(e) = sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await {
        let e = Arc::new(e);
        for job in batch {
            respond(job, Err(e.clone()));
        }
        return;
    }

    let mut results = Vec::with_capacity(batch.len());
    for job in batch {
        let r = match &job {
            Job::UpdateHashChain { device_id, new_hash, .. } => {
                apply_update_hash_chain(conn, device_id, new_hash).await
            }
            Job::ResetHashState { device_id, .. } => crate::db::reset_hash_state_tx(conn, device_id).await,
        };
        results.push((job, r));
    }

    match sqlx::query("COMMIT").execute(&mut *conn).await {
        Ok(_) => {
            for (job, r) in results {
                respond(job, r.map_err(Arc::new));
            }
        }
        Err(commit_err) => {
            let commit_err = Arc::new(commit_err);
            for (job, _) in results {
                respond(job, Err(commit_err.clone()));
            }
        }
    }
}

async fn apply_update_hash_chain(
    conn: &mut SqliteConnection,
    device_id: &str,
    new_hash: &[u8; 32],
) -> sqlx::Result<()> {
    let current = crate::db::get_hash_state_tx(conn, device_id).await?.unwrap_or([0u8; 32]);

    let mut input = [0u8; 64];
    input[..32].copy_from_slice(&current);
    input[32..].copy_from_slice(new_hash);
    let next: [u8; 32] = sha2::Sha256::digest(input).into();

    let now = crate::db::now_ms();
    crate::db::upsert_hash_state_tx(conn, device_id, &next, now, now).await
}

fn respond(job: Job, result: Result<(), Arc<sqlx::Error>>) {
    match job {
        Job::UpdateHashChain { respond, .. } => {
            let _ = respond.send(result);
        }
        Job::ResetHashState { respond, .. } => {
            let _ = respond.send(result);
        }
    }
}
