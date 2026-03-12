import { env, SELF } from 'cloudflare:test';

const schema = `
CREATE TABLE IF NOT EXISTS users (
  id BLOB PRIMARY KEY,
  email TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  name TEXT,
  email_verified INTEGER NOT NULL DEFAULT 0,
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
  start INTEGER NOT NULL,
  end INTEGER NOT NULL,
  end_hash TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_batches_user_id ON batches(user_id);
CREATE INDEX IF NOT EXISTS idx_batches_device_id ON batches(device_id);
CREATE INDEX IF NOT EXISTS idx_batches_created_at ON batches(created_at);

CREATE TABLE IF NOT EXISTS partners (
  id BLOB PRIMARY KEY,
  user_id BLOB NOT NULL,
  partner_user_id BLOB,
  partner_email TEXT NOT NULL,
  invite_token_hash TEXT UNIQUE,
  invite_expires_at INTEGER,
  status TEXT NOT NULL,
  permissions TEXT NOT NULL,
  e2ee_key BLOB,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (partner_user_id) REFERENCES users(id) ON DELETE CASCADE,
  UNIQUE (user_id, partner_email)
);
CREATE INDEX IF NOT EXISTS idx_partners_user_id ON partners(user_id);
CREATE INDEX IF NOT EXISTS idx_partners_partner_user_id ON partners(partner_user_id);
CREATE INDEX IF NOT EXISTS idx_partners_status ON partners(status);
CREATE INDEX IF NOT EXISTS idx_partners_invite_expires_at ON partners(invite_expires_at);

CREATE TABLE IF NOT EXISTS partner_notification_preferences (
  partnership_id BLOB PRIMARY KEY,
  digest_cadence TEXT NOT NULL DEFAULT 'daily',
  immediate_tamper_severity TEXT NOT NULL DEFAULT 'critical',
  send_digest INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (partnership_id) REFERENCES partners(id) ON DELETE CASCADE
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
  user_id BLOB NOT NULL,
  email TEXT NOT NULL,
  purpose TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  expires_at INTEGER NOT NULL,
  consumed_at INTEGER,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  session_type TEXT NOT NULL,
  user_id TEXT,
  device_id TEXT,
  refresh_token_hash TEXT NOT NULL UNIQUE,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE,
  CHECK (session_type IN ('web', 'device')),
  CHECK (
    (session_type = 'web' AND user_id IS NOT NULL AND device_id IS NULL) OR
    (session_type = 'device' AND device_id IS NOT NULL AND user_id IS NULL)
  )
);
CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_device_id ON sessions(device_id);
CREATE INDEX IF NOT EXISTS idx_sessions_expires_at ON sessions(expires_at);

CREATE TABLE IF NOT EXISTS hash_states (
  device_id BLOB PRIMARY KEY,
  user_id BLOB NOT NULL,
  state BLOB NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_hash_states_user_id ON hash_states(user_id);
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
