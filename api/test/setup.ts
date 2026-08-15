import { env, SELF } from 'cloudflare:test';

const schema = `
CREATE TABLE IF NOT EXISTS users (
  id BLOB PRIMARY KEY,
  email TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  password_salt BLOB NOT NULL,
  password_params_version TEXT NOT NULL DEFAULT 'argon2id-v1',
  name TEXT,
  email_verified INTEGER NOT NULL DEFAULT 0,
  email_bounced_at INTEGER,
  settings TEXT NOT NULL DEFAULT '{}',
  pub_key BLOB,
  encrypted_priv_key BLOB,
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
  deleted_at INTEGER,
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
  access_keys TEXT NOT NULL,
  version TEXT NOT NULL DEFAULT '',
  high_risk_count INTEGER NOT NULL DEFAULT 0,
  medium_risk_count INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_batches_user_id_created_at ON batches(user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_batches_created_at ON batches(created_at);

CREATE TABLE IF NOT EXISTS partners (
  id BLOB PRIMARY KEY,
  watching_user_id BLOB NOT NULL,
  watcher_user_id BLOB,
  watcher_email TEXT NOT NULL,
  invite_token_id BLOB UNIQUE,
  status TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (watching_user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (watcher_user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (invite_token_id) REFERENCES email_tokens(id) ON DELETE SET NULL,
  UNIQUE (watching_user_id, watcher_email)
);
CREATE INDEX IF NOT EXISTS idx_partners_watching_user_id ON partners(watching_user_id);
CREATE INDEX IF NOT EXISTS idx_partners_watcher_user_id ON partners(watcher_user_id);
CREATE INDEX IF NOT EXISTS idx_partners_status ON partners(status);

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
