import { env, SELF } from 'cloudflare:test';

const schema = `
CREATE TABLE IF NOT EXISTS users (
  id BLOB PRIMARY KEY,
  email TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  name TEXT,
  email_verified INTEGER NOT NULL DEFAULT 0,
  email_bounced_at INTEGER,
  e2ee_key BLOB,
  pub_key BLOB,
  priv_key BLOB,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);

CREATE TABLE IF NOT EXISTS devices (
  id BLOB PRIMARY KEY,
  owner BLOB NOT NULL,
  name TEXT NOT NULL,
  platform TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (owner) REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_devices_owner ON devices(owner);

CREATE TABLE IF NOT EXISTS batches (
  id BLOB PRIMARY KEY,
  user_id BLOB NOT NULL,
  device_id BLOB NOT NULL,
  url TEXT NOT NULL UNIQUE,
  start_time INTEGER NOT NULL,
  end_time INTEGER NOT NULL,
  end_hash TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_batches_user_id ON batches(user_id);
CREATE INDEX IF NOT EXISTS idx_batches_created_at ON batches(created_at);

CREATE TABLE IF NOT EXISTS partners (
  id BLOB PRIMARY KEY,
  user_id BLOB NOT NULL,
  partner_user_id BLOB,
  partner_email TEXT NOT NULL,
  invite_token_id BLOB UNIQUE,
  status TEXT NOT NULL,
  permissions TEXT NOT NULL,
  e2ee_key BLOB,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (partner_user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (invite_token_id) REFERENCES email_tokens(id) ON DELETE SET NULL,
  UNIQUE (user_id, partner_email)
);
CREATE INDEX IF NOT EXISTS idx_partners_user_id ON partners(user_id);
CREATE INDEX IF NOT EXISTS idx_partners_partner_user_id ON partners(partner_user_id);
CREATE INDEX IF NOT EXISTS idx_partners_status ON partners(status);

CREATE TABLE IF NOT EXISTS partner_preferences (
  partnership_id BLOB PRIMARY KEY,
  email_frequency TEXT NOT NULL DEFAULT 'daily',
  immediate_tamper_severity TEXT NOT NULL DEFAULT 'critical',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (partnership_id) REFERENCES partners(id) ON DELETE CASCADE,
  CHECK (email_frequency IN ('none', 'alerts-only', 'daily', 'weekly'))
);

CREATE TABLE IF NOT EXISTS device_logs (
  id BLOB PRIMARY KEY,
  user_id BLOB NOT NULL,
  device_id BLOB NOT NULL,
  ts INTEGER NOT NULL,
  type TEXT NOT NULL,
  data TEXT NOT NULL,
  risk REAL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_device_logs_user_id ON device_logs(user_id);
CREATE INDEX IF NOT EXISTS idx_device_logs_device_id ON device_logs(device_id);
CREATE INDEX IF NOT EXISTS idx_device_logs_created_at ON device_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_device_logs_risk ON device_logs(risk);

CREATE TABLE IF NOT EXISTS email_tokens (
  id BLOB PRIMARY KEY,
  user_id BLOB,
  email TEXT NOT NULL,
  purpose TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  expires_at INTEGER NOT NULL,
  consumed_at INTEGER,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_sessions (
  refresh_token_hash TEXT PRIMARY KEY,
  user_id BLOB NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS device_sessions (
  refresh_token_hash TEXT PRIMARY KEY,
  device_id BLOB NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_user_sessions_user_id ON user_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_device_sessions_device_id ON device_sessions(device_id);

CREATE TABLE IF NOT EXISTS hash_states (
  device_id BLOB PRIMARY KEY,
  state BLOB NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
);
`;

const statements = schema
  .split(';')
  .map((statement) => statement.replace(/--[^\n]*/g, '').trim())
  .filter(Boolean);

for (const statement of statements) {
  await env.DB.prepare(statement).run();
}

const originalFetch = globalThis.fetch.bind(globalThis);

globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
  const url =
    typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url;

  if (url.startsWith('http://localhost/')) {
    return SELF.fetch(input, init);
  }

  return originalFetch(input, init);
}) as typeof fetch;
