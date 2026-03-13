-- Virtue Initiative API schema

CREATE TABLE users (
  id BLOB PRIMARY KEY,
  email TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  name TEXT,
  email_verified INTEGER NOT NULL DEFAULT 0,
  email_bounced_at INTEGER,
  e2ee_key BLOB,
  pub_key BLOB,
  priv_key BLOB,
  created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX idx_users_email ON users(email);

CREATE TABLE devices (
  id BLOB PRIMARY KEY,
  owner BLOB NOT NULL,
  name TEXT NOT NULL,
  platform TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (owner) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_devices_owner ON devices(owner);

-- Encrypted 1-hour batch blobs stored in R2
CREATE TABLE batches (
  id BLOB PRIMARY KEY,
  user_id BLOB NOT NULL,
  device_id BLOB NOT NULL,
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
  id BLOB PRIMARY KEY,
  user_id BLOB NOT NULL,
  partner_user_id BLOB,
  partner_email TEXT NOT NULL,
  invite_token_hash TEXT UNIQUE,
  invite_expires_at INTEGER,
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
CREATE INDEX idx_partners_invite_expires_at ON partners(invite_expires_at);

CREATE TABLE partner_notification_preferences (
  partnership_id BLOB PRIMARY KEY,
  digest_cadence TEXT NOT NULL DEFAULT 'daily',
  immediate_tamper_severity TEXT NOT NULL DEFAULT 'critical',
  send_digest INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (partnership_id) REFERENCES partners(id) ON DELETE CASCADE
);

CREATE INDEX idx_partner_notification_preferences_digest_cadence
  ON partner_notification_preferences(digest_cadence);

-- Non-encrypted immediate device log entries sent directly from devices
CREATE TABLE device_logs (
  id BLOB PRIMARY KEY,
  user_id BLOB NOT NULL,
  device_id BLOB NOT NULL,
  ts INTEGER NOT NULL,
  type TEXT NOT NULL,
  data TEXT NOT NULL DEFAULT '{}',
  risk REAL,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
);

CREATE INDEX idx_device_logs_user_id ON device_logs(user_id);
CREATE INDEX idx_device_logs_device_id ON device_logs(device_id);
CREATE INDEX idx_device_logs_created_at ON device_logs(created_at);
CREATE INDEX idx_device_logs_risk ON device_logs(risk);

CREATE TABLE email_tokens (
  id BLOB PRIMARY KEY,
  user_id BLOB NOT NULL,
  email TEXT NOT NULL,
  purpose TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  expires_at INTEGER NOT NULL,
  consumed_at INTEGER,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_email_tokens_user_id ON email_tokens(user_id);
CREATE INDEX idx_email_tokens_email ON email_tokens(email);
CREATE INDEX idx_email_tokens_purpose ON email_tokens(purpose);
CREATE INDEX idx_email_tokens_expires_at ON email_tokens(expires_at);

CREATE TABLE hash_states (
  device_id BLOB PRIMARY KEY,
  user_id BLOB NOT NULL,
  state BLOB NOT NULL,
  updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_hash_states_user_id ON hash_states(user_id);
