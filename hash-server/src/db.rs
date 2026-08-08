use sqlx::{SqlitePool, sqlite::SqliteConnection};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

pub async fn get_hash_state(pool: &SqlitePool, device_id: &str) -> sqlx::Result<Option<[u8; 32]>> {
    let row = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT state FROM hash_states WHERE device_id = ?",
    )
    .bind(device_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.and_then(|v| v.try_into().ok()))
}

async fn get_hash_state_tx(
    tx: &mut SqliteConnection,
    device_id: &str,
) -> sqlx::Result<Option<[u8; 32]>> {
    let row = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT state FROM hash_states WHERE device_id = ?",
    )
    .bind(device_id)
    .fetch_optional(&mut *tx)
    .await?;

    Ok(row.and_then(|v| v.try_into().ok()))
}

async fn upsert_hash_state_tx(
    tx: &mut SqliteConnection,
    device_id: &str,
    state: &[u8; 32],
    updated_at: i64,
    hashed_at: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO hash_states (device_id, state, updated_at, count, hashed_at)
         VALUES (?, ?, ?, 1, ?)
         ON CONFLICT(device_id) DO UPDATE SET
             state = excluded.state,
             updated_at = excluded.updated_at,
             count = count + 1,
             hashed_at = excluded.hashed_at",
    )
    .bind(device_id)
    .bind(state.as_slice())
    .bind(updated_at)
    .bind(hashed_at)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

pub async fn update_hash_chain(
    pool: &SqlitePool,
    device_id: &str,
    new_hash: &[u8; 32],
) -> sqlx::Result<()> {
    use sha2::Digest;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    let current = get_hash_state_tx(&mut tx, device_id)
        .await?
        .unwrap_or([0u8; 32]);

    let mut input = [0u8; 64];
    input[..32].copy_from_slice(&current);
    input[32..].copy_from_slice(new_hash);

    let next: [u8; 32] = sha2::Sha256::digest(input).into();
    let now = now_ms();
    upsert_hash_state_tx(&mut tx, device_id, &next, now, now).await?;

    tx.commit().await?;
    Ok(())
}

pub async fn reset_hash_state(pool: &SqlitePool, device_id: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO hash_states (device_id, state, updated_at, count)
         VALUES (?, ?, ?, 0)
         ON CONFLICT(device_id) DO UPDATE SET
             state = excluded.state,
             updated_at = excluded.updated_at,
             count = 0",
    )
    .bind(device_id)
    .bind([0u8; 32].as_slice())
    .bind(now_ms())
    .execute(pool)
    .await?;
    Ok(())
}

pub struct HashInfo {
    pub count: i64,
    pub hashed_at: Option<i64>,
    pub updated_at: i64,
}

pub async fn get_hash_info(pool: &SqlitePool, device_id: &str) -> sqlx::Result<Option<HashInfo>> {
    let row = sqlx::query_as::<_, (i64, Option<i64>, i64)>(
        "SELECT count, hashed_at, updated_at FROM hash_states WHERE device_id = ?",
    )
    .bind(device_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(count, hashed_at, updated_at)| HashInfo { count, hashed_at, updated_at }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sha2::Digest;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    pub async fn in_memory_pool() -> SqlitePool {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:").unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn get_missing_is_none() {
        let pool = in_memory_pool().await;
        let result = get_hash_state(&pool, "device-1").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn first_update_chains_from_zeros() {
        let pool = in_memory_pool().await;
        let new_hash = [1u8; 32];
        update_hash_chain(&pool, "device-1", &new_hash).await.unwrap();

        let state = get_hash_state(&pool, "device-1").await.unwrap().unwrap();

        let mut input = [0u8; 64];
        input[32..].fill(1);
        let expected: [u8; 32] = sha2::Sha256::digest(input).into();
        assert_eq!(state, expected);
    }

    #[tokio::test]
    async fn second_update_chains_from_first() {
        let pool = in_memory_pool().await;

        update_hash_chain(&pool, "device-1", &[1u8; 32]).await.unwrap();
        let state1 = get_hash_state(&pool, "device-1").await.unwrap().unwrap();

        update_hash_chain(&pool, "device-1", &[2u8; 32]).await.unwrap();
        let state2 = get_hash_state(&pool, "device-1").await.unwrap().unwrap();

        let mut input = [0u8; 64];
        input[..32].copy_from_slice(&state1);
        input[32..].fill(2);
        let expected: [u8; 32] = sha2::Sha256::digest(input).into();
        assert_eq!(state2, expected);
    }

    #[tokio::test]
    async fn reset_returns_zeros() {
        let pool = in_memory_pool().await;

        update_hash_chain(&pool, "device-1", &[42u8; 32]).await.unwrap();
        reset_hash_state(&pool, "device-1").await.unwrap();

        let state = get_hash_state(&pool, "device-1").await.unwrap().unwrap();
        assert_eq!(state, [0u8; 32]);
    }

    #[tokio::test]
    async fn devices_are_independent() {
        let pool = in_memory_pool().await;

        update_hash_chain(&pool, "device-a", &[1u8; 32]).await.unwrap();
        update_hash_chain(&pool, "device-b", &[2u8; 32]).await.unwrap();

        let a = get_hash_state(&pool, "device-a").await.unwrap().unwrap();
        let b = get_hash_state(&pool, "device-b").await.unwrap().unwrap();
        assert_ne!(a, b);
    }
}
