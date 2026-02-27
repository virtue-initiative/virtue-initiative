-- BePure API schema

CREATE TABLE users (
  id TEXT PRIMARY KEY,
  email TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  name TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_users_email ON users(email);

CREATE TABLE devices (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  name TEXT NOT NULL,
  platform TEXT NOT NULL,
  avg_interval_seconds INTEGER NOT NULL DEFAULT 300,
  last_seen_at TEXT,
  last_upload_at TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_devices_user_id ON devices(user_id);

-- Encrypted 1-hour batch blobs stored in R2
CREATE TABLE r2_batches (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  device_id TEXT NOT NULL,
  r2_key TEXT NOT NULL UNIQUE,
  start_time TEXT NOT NULL,
  end_time TEXT NOT NULL,
  start_chain_hash TEXT NOT NULL, -- hex SHA-256
  end_chain_hash TEXT NOT NULL,   -- hex SHA-256
  item_count INTEGER NOT NULL DEFAULT 0,
  size_bytes INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
);

CREATE INDEX idx_r2_batches_user_id ON r2_batches(user_id);
CREATE INDEX idx_r2_batches_device_id ON r2_batches(device_id);
CREATE INDEX idx_r2_batches_start_time ON r2_batches(start_time);

-- Per-minute binary hash chain entries for tamper detection
CREATE TABLE chain_hashes (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  device_id TEXT NOT NULL,
  hash BLOB NOT NULL,              -- raw 32 bytes
  client_timestamp TEXT NOT NULL,  -- ISO-8601 from client
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
);

CREATE INDEX idx_chain_hashes_user_device ON chain_hashes(user_id, device_id);
CREATE INDEX idx_chain_hashes_client_timestamp ON chain_hashes(client_timestamp);

CREATE TABLE partners (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  partner_user_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  permissions TEXT NOT NULL, -- JSON: { "view_data": true }
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (partner_user_id) REFERENCES users(id) ON DELETE CASCADE,
  UNIQUE(user_id, partner_user_id)
);

CREATE INDEX idx_partners_user_id ON partners(user_id);
CREATE INDEX idx_partners_partner_user_id ON partners(partner_user_id);
CREATE INDEX idx_partners_status ON partners(status);

-- Settings stored as a single JSON blob for flexibility
CREATE TABLE settings (
  user_id TEXT PRIMARY KEY,
  data TEXT NOT NULL DEFAULT '{}',
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
