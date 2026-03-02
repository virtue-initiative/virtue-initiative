-- Per-device rolling state for the new hash verification system.
-- new_state = sha256(current_state || content_hash) is verified on each log upload.
-- State resets to random bytes after each batch upload (returned to client).
CREATE TABLE device_states (
  device_id  TEXT PRIMARY KEY,
  user_id    TEXT NOT NULL,
  state      BLOB NOT NULL,  -- raw 32 bytes (current rolling state)
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id)   REFERENCES users(id)   ON DELETE CASCADE
);

CREATE INDEX idx_device_states_user_id ON device_states(user_id);
