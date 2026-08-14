use std::collections::HashMap;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

use crate::error::ApiError;
use crate::state::{DeviceState, SharedDevices, ZERO_HASH};

enum WriteCommand {
    Ingest {
        device_id: String,
        unix_time: u32,
        seq: u32,
        hash: [u8; 32],
        respond: oneshot::Sender<Result<(), ApiError>>,
    },
    Reset {
        device_id: String,
        respond: oneshot::Sender<DeviceState>,
    },
}

#[derive(Clone)]
pub struct WriteHandle {
    tx: std_mpsc::Sender<WriteCommand>,
}

impl WriteHandle {
    pub async fn ingest(
        &self,
        device_id: String,
        unix_time: u32,
        seq: u32,
        hash: [u8; 32],
    ) -> Result<(), ApiError> {
        let (respond, rx) = oneshot::channel();
        self.tx
            .send(WriteCommand::Ingest {
                device_id,
                unix_time,
                seq,
                hash,
                respond,
            })
            .map_err(|_| ApiError::Internal(Some("writer thread stopped".into())))?;
        rx.await
            .map_err(|_| ApiError::Internal(Some("writer thread stopped".into())))?
    }

    pub async fn reset(&self, device_id: String) -> Result<DeviceState, ApiError> {
        let (respond, rx) = oneshot::channel();
        self.tx
            .send(WriteCommand::Reset { device_id, respond })
            .map_err(|_| ApiError::Internal(Some("writer thread stopped".into())))?;
        rx.await
            .map_err(|_| ApiError::Internal(Some("writer thread stopped".into())))
    }
}

/// Opens the database, loads existing state into `devices`, and starts the
/// single dedicated writer thread required by SPEC.md section 3.3. All
/// mutations funnel through this thread and are grouped into one transaction
/// per batch window so concurrent writers never block on SQLite locks.
pub fn spawn_writer(
    database_path: &str,
    batch_window: Duration,
    devices: SharedDevices,
) -> WriteHandle {
    let mut conn = Connection::open(database_path).expect("failed to open database");
    init_schema(&conn);
    load_into_memory(&conn, &devices);

    let (tx, rx) = std_mpsc::channel::<WriteCommand>();

    std::thread::Builder::new()
        .name("hash-server-writer".into())
        .spawn(move || writer_loop(&mut conn, rx, batch_window, devices))
        .expect("failed to spawn writer thread");

    WriteHandle { tx }
}

fn init_schema(conn: &Connection) {
    conn.pragma_update(None, "journal_mode", "WAL")
        .expect("failed to enable WAL");
    conn.pragma_update(None, "synchronous", "FULL")
        .expect("failed to set synchronous=FULL");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS device_hashes (
            device_id TEXT PRIMARY KEY,
            hash BLOB NOT NULL,
            seq INTEGER NOT NULL,
            last_received INTEGER NOT NULL
        ) STRICT;",
    )
    .expect("failed to create device_hashes table");
}

fn load_into_memory(conn: &Connection, devices: &SharedDevices) {
    let mut stmt = conn
        .prepare("SELECT device_id, hash, seq, last_received FROM device_hashes")
        .expect("failed to prepare load query");

    let rows = stmt
        .query_map([], |row| {
            let device_id: String = row.get(0)?;
            let hash_vec: Vec<u8> = row.get(1)?;
            let seq: u32 = row.get(2)?;
            let last_received: u32 = row.get(3)?;

            let mut hash = [0u8; 32];
            hash.copy_from_slice(&hash_vec);

            Ok((
                device_id,
                DeviceState {
                    hash,
                    seq,
                    last_received,
                },
            ))
        })
        .expect("failed to load device_hashes rows");

    let mut map = devices.write().unwrap();
    for row in rows {
        let (device_id, state) = row.expect("failed to read device_hashes row");
        map.insert(device_id, state);
    }
}

fn writer_loop(
    conn: &mut Connection,
    rx: std_mpsc::Receiver<WriteCommand>,
    batch_window: Duration,
    devices: SharedDevices,
) {
    loop {
        let Ok(first) = rx.recv() else { return };
        let mut batch = vec![first];

        let deadline = Instant::now() + batch_window;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(remaining) {
                Ok(cmd) => batch.push(cmd),
                Err(_) => break,
            }
        }

        process_batch(conn, batch, &devices);
    }
}

fn process_batch(conn: &mut Connection, batch: Vec<WriteCommand>, devices: &SharedDevices) {
    let snapshot = devices.read().unwrap().clone();
    let mut scratch: HashMap<String, DeviceState> = HashMap::new();

    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => {
            respond_internal(batch, format!("failed to start transaction: {e}"));
            return;
        }
    };

    // (responder, prior-state-for-resets, whether this is a reset)
    let mut ingest_ok: Vec<oneshot::Sender<Result<(), ApiError>>> = Vec::new();
    let mut reset_ok: Vec<(oneshot::Sender<DeviceState>, DeviceState)> = Vec::new();

    for cmd in batch {
        match cmd {
            WriteCommand::Ingest {
                device_id,
                unix_time,
                seq,
                hash,
                respond,
            } => {
                let current = scratch
                    .get(&device_id)
                    .copied()
                    .unwrap_or_else(|| snapshot.get(&device_id).copied().unwrap_or_default());

                if seq <= current.seq {
                    let _ = respond.send(Err(ApiError::SequenceConflict));
                    continue;
                }

                let mut hasher = Sha256::new();
                hasher.update(current.hash);
                hasher.update(hash);
                let new_hash: [u8; 32] = hasher.finalize().into();

                let new_state = DeviceState {
                    hash: new_hash,
                    seq,
                    last_received: unix_time,
                };

                if let Err(e) = upsert(&tx, &device_id, &new_state) {
                    let _ = respond.send(Err(ApiError::Internal(Some(e.to_string()))));
                    continue;
                }

                scratch.insert(device_id, new_state);
                ingest_ok.push(respond);
            }
            WriteCommand::Reset { device_id, respond } => {
                let prior = scratch
                    .get(&device_id)
                    .copied()
                    .unwrap_or_else(|| snapshot.get(&device_id).copied().unwrap_or_default());
                // SPEC.md section 2.3: the reset MUST NOT touch last_received.
                let new_state = DeviceState {
                    hash: ZERO_HASH,
                    seq: 0,
                    last_received: prior.last_received,
                };

                if let Err(e) = upsert(&tx, &device_id, &new_state) {
                    tracing::error!("failed to stage reset for {device_id}: {e}");
                    // The response channel expects a DeviceState, not a Result, so a
                    // staging failure here just drops the responder; the client's
                    // request will time out / disconnect rather than see a wrong value.
                    drop(respond);
                    continue;
                }

                scratch.insert(device_id, new_state);
                reset_ok.push((respond, prior));
            }
        }
    }

    match tx.commit() {
        Ok(()) => {
            devices.write().unwrap().extend(scratch);
            for respond in ingest_ok {
                let _ = respond.send(Ok(()));
            }
            for (respond, prior) in reset_ok {
                let _ = respond.send(prior);
            }
        }
        Err(e) => {
            let message = format!("failed to commit transaction: {e}");
            for respond in ingest_ok {
                let _ = respond.send(Err(ApiError::Internal(Some(message.clone()))));
            }
            // Resets can't report a commit failure through their DeviceState-only
            // channel; dropping the sender surfaces as a client-visible disconnect.
            drop(reset_ok);
        }
    }
}

fn upsert(
    tx: &rusqlite::Transaction,
    device_id: &str,
    state: &DeviceState,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO device_hashes (device_id, hash, seq, last_received)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(device_id) DO UPDATE SET
            hash = excluded.hash,
            seq = excluded.seq,
            last_received = excluded.last_received",
        rusqlite::params![
            device_id,
            state.hash.as_slice(),
            state.seq,
            state.last_received
        ],
    )?;
    Ok(())
}

fn respond_internal(batch: Vec<WriteCommand>, message: String) {
    for cmd in batch {
        match cmd {
            WriteCommand::Ingest { respond, .. } => {
                let _ = respond.send(Err(ApiError::Internal(Some(message.clone()))));
            }
            WriteCommand::Reset { respond, .. } => drop(respond),
        }
    }
}
