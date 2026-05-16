CREATE TABLE IF NOT EXISTS hash_states (
    device_id  TEXT    PRIMARY KEY,
    state      BLOB    NOT NULL,
    updated_at INTEGER NOT NULL
);
