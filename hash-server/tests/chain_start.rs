//! HASH-006 to HASH-008: where the current chain starts (`first_seq`), and the
//! conditional reset the main API uses to reset exactly the chain it read.

mod common;

use common::*;

// HASH-006, HASH-007: first_seq is the seq of the first hash since the reset, and
// later hashes leave it alone.
#[tokio::test]
async fn first_seq_is_the_first_accepted_seq_of_the_chain() {
    let server = spawn_server().await;
    assert_eq!(server.state(DEVICE_A).await["first_seq"], 0);

    assert_eq!(server.post(DEVICE_A, 100, 7, [1; 32]).await.status(), 201);
    assert_eq!(server.post(DEVICE_A, 101, 8, [2; 32]).await.status(), 201);
    assert_eq!(server.post(DEVICE_A, 102, 12, [3; 32]).await.status(), 201);
    let state = server.state(DEVICE_A).await;
    assert_eq!(state["first_seq"], 7);
    assert_eq!(state["seq"], 12);

    // A rejected hash does not start or move the chain.
    assert_eq!(server.post(DEVICE_A, 103, 12, [4; 32]).await.status(), 409);
    assert_eq!(server.state(DEVICE_A).await["first_seq"], 7);
}

// HASH-007, HASH-008: a reset clears first_seq, and the next hash starts a new chain.
#[tokio::test]
async fn reset_starts_a_new_chain() {
    let server = spawn_server().await;
    server.post(DEVICE_A, 100, 3, [1; 32]).await;
    server.post(DEVICE_A, 101, 4, [2; 32]).await;

    let resp = server.delete_device(&format!("device={DEVICE_A}")).await;
    assert_eq!(resp.status(), 200);
    let prior: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(prior["first_seq"], 3);
    assert_eq!(prior["seq"], 4);
    assert_eq!(server.state(DEVICE_A).await["first_seq"], 0);

    // The device's seq may carry on from before the reset.
    server.post(DEVICE_A, 102, 5, [3; 32]).await;
    let state = server.state(DEVICE_A).await;
    assert_eq!(state["first_seq"], 5);
    assert_eq!(state["seq"], 5);
}

// HASH-008: a reset with `seq` happens only if the device is still at that seq.
#[tokio::test]
async fn conditional_reset_only_resets_the_chain_it_names() {
    let server = spawn_server().await;
    server.post(DEVICE_A, 100, 1, [1; 32]).await;
    server.post(DEVICE_A, 101, 2, [2; 32]).await;
    let before = server.state(DEVICE_A).await;

    // The chain moved on to seq 2, so a reset of seq 1 changes nothing.
    let resp = server
        .delete_device(&format!("device={DEVICE_A}&seq=1"))
        .await;
    assert_error_shape(resp, 409, "sequence_conflict").await;
    assert_eq!(server.state(DEVICE_A).await, before);

    let resp = server
        .delete_device(&format!("device={DEVICE_A}&seq=2"))
        .await;
    assert_eq!(resp.status(), 200);
    let prior: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(prior["hash"], before["hash"]);
    let after = server.state(DEVICE_A).await;
    assert_eq!(after["seq"], 0);
    assert_eq!(after["hash"], ZERO_HEX);

    // An empty chain is at seq 0, so `seq=0` resets it and anything else conflicts.
    let resp = server
        .delete_device(&format!("device={DEVICE_B}&seq=0"))
        .await;
    assert_eq!(resp.status(), 200);
    let resp = server
        .delete_device(&format!("device={DEVICE_B}&seq=1"))
        .await;
    assert_error_shape(resp, 409, "sequence_conflict").await;
}

// HASH-008: a `seq` that is not a non-negative integer is a malformed query.
#[tokio::test]
async fn conditional_reset_rejects_a_malformed_seq() {
    let server = spawn_server().await;
    for seq in ["-1", "abc", "1.5", "4294967296"] {
        let resp = server
            .delete_device(&format!("device={DEVICE_A}&seq={seq}"))
            .await;
        assert_error_shape(resp, 400, "invalid_query").await;
    }
}

// HASH-007: a chain stored before the server recorded first_seq reports 0 until
// its next reset, and the server keeps its state across the upgrade.
#[tokio::test]
async fn chain_from_before_first_seq_reports_zero() {
    let path = temp_db_path();
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE device_hashes (
                device_id TEXT PRIMARY KEY,
                hash BLOB NOT NULL,
                seq INTEGER NOT NULL,
                last_received INTEGER NOT NULL
            ) STRICT;",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO device_hashes VALUES (?1, ?2, 9, 500)",
            rusqlite::params![DEVICE_A, [7u8; 32].as_slice()],
        )
        .unwrap();
    }

    let server = spawn_server_at(&path).await;
    let state = server.state(DEVICE_A).await;
    assert_eq!(state["seq"], 9);
    assert_eq!(state["first_seq"], 0);
    assert_eq!(state["hash"], hex::encode([7u8; 32]));

    assert_eq!(server.post(DEVICE_A, 501, 10, [1; 32]).await.status(), 201);
    assert_eq!(server.state(DEVICE_A).await["first_seq"], 0);

    server.delete_device(&format!("device={DEVICE_A}")).await;
    server.post(DEVICE_A, 502, 11, [1; 32]).await;
    assert_eq!(server.state(DEVICE_A).await["first_seq"], 11);
}
