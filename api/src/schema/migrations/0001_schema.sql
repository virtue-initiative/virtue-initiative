-- Virtue Initiative API schema

CREATE TABLE users (
  id TEXT PRIMARY KEY,
  email TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  name TEXT,
  e2ee_key BLOB,
  pub_key BLOB,
  created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX idx_users_email ON users(email);

CREATE TABLE devices (
  id TEXT PRIMARY KEY,
  owner TEXT NOT NULL,
  name TEXT NOT NULL,
  platform TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (owner) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_devices_owner ON devices(owner);

-- Encrypted 1-hour batch blobs stored in R2
CREATE TABLE batches (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  device_id TEXT NOT NULL,
  url TEXT NOT NULL UNIQUE,
  start INTEGER NOT NULL,
  "end" INTEGER NOT NULL,
  end_hash TEXT NOT NULL,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
);

CREATE INDEX idx_batches_user_id ON batches(user_id);
CREATE INDEX idx_batches_device_id ON batches(device_id);
CREATE INDEX idx_batches_created_at ON batches(created_at);

CREATE TABLE partners (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  partner_user_id TEXT,
  partner_email TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  permissions TEXT NOT NULL,
  e2ee_key BLOB,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (partner_user_id) REFERENCES users(id) ON DELETE CASCADE,
  UNIQUE(user_id, partner_email)
);

CREATE INDEX idx_partners_user_id ON partners(user_id);
CREATE INDEX idx_partners_partner_user_id ON partners(partner_user_id);
CREATE INDEX idx_partners_status ON partners(status);

-- Non-encrypted immediate device log entries sent directly from devices
CREATE TABLE device_logs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  device_id TEXT NOT NULL,
  ts INTEGER NOT NULL,
  type TEXT NOT NULL,
  data TEXT NOT NULL DEFAULT '{}',
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
);

CREATE INDEX idx_device_logs_user_id ON device_logs(user_id);
CREATE INDEX idx_device_logs_device_id ON device_logs(device_id);
CREATE INDEX idx_device_logs_created_at ON device_logs(created_at);

CREATE TABLE hash_states (
  device_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  state BLOB NOT NULL,
  updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_hash_states_user_id ON hash_states(user_id);
