import { env, SELF } from 'cloudflare:test';
import { TEST_TURNSTILE_TOKEN } from './helpers';

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
  id BLOB NOT NULL,
  user_id BLOB NOT NULL,
  device_id BLOB NOT NULL,
  start_time INTEGER NOT NULL,
  end_time INTEGER NOT NULL,
  end_hash BLOB NOT NULL,
  access_keys BLOB NOT NULL,
  version TEXT NOT NULL DEFAULT '',
  high_risk_count INTEGER NOT NULL DEFAULT 0,
  medium_risk_count INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (user_id, created_at, id),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS partners (
  id BLOB PRIMARY KEY,
  watching_user_id BLOB NOT NULL,
  watcher_user_id BLOB,
  watcher_email TEXT NOT NULL,
  invite_token_id BLOB UNIQUE,
  status TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  accepted_at INTEGER,
  FOREIGN KEY (watching_user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (watcher_user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (invite_token_id) REFERENCES email_tokens(id) ON DELETE SET NULL,
  UNIQUE (watching_user_id, watcher_email)
);
CREATE INDEX IF NOT EXISTS idx_partners_watching_user_id ON partners(watching_user_id);
CREATE INDEX IF NOT EXISTS idx_partners_watcher_user_id ON partners(watcher_user_id);

CREATE TABLE IF NOT EXISTS email_bounces (
  id BLOB PRIMARY KEY,
  email TEXT NOT NULL,
  bounced_at INTEGER NOT NULL,
  reason TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_email_bounces_email ON email_bounces(email);

CREATE TABLE IF NOT EXISTS email_tokens (
  id BLOB PRIMARY KEY,
  user_id BLOB,
  email TEXT NOT NULL,
  purpose TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  expires_at INTEGER NOT NULL,
  consumed_at INTEGER,
  created_at INTEGER NOT NULL,
  bounce_id BLOB REFERENCES email_bounces(id) ON DELETE SET NULL,
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

CREATE TABLE IF NOT EXISTS locked_passwords (
  id BLOB PRIMARY KEY,
  owner_id BLOB NOT NULL,
  label TEXT NOT NULL,
  wrapped_value TEXT NOT NULL,
  accessed_at INTEGER,
  deleted_at INTEGER,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (owner_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_locked_passwords_owner_id ON locked_passwords(owner_id);
CREATE INDEX IF NOT EXISTS idx_locked_passwords_deleted_at ON locked_passwords(deleted_at);

CREATE TABLE IF NOT EXISTS admins (
  user_id BLOB PRIMARY KEY,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS analytics_snapshots (
  day TEXT PRIMARY KEY,
  metrics TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
`;

const statements = schema
  .split(';')
  .map((statement) => statement.replace(/--[^\n]*/g, '').trim())
  .filter(Boolean);

for (const statement of statements) {
  await env.DB.prepare(statement).run();
}

// Mirrors api-donate/src/schema/migrations/0003_newsletter_signups.sql.
await env.DONATE_DB.prepare(
  `CREATE TABLE IF NOT EXISTS newsletter_signups (
    email TEXT PRIMARY KEY,
    token TEXT NOT NULL UNIQUE DEFAULT (lower(hex(randomblob(16)))),
    country TEXT,
    region TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    confirmed_at INTEGER,
    confirmation_sent_at INTEGER,
    unsubscribed_at INTEGER
  )`,
).run();

const TURNSTILE_SITEVERIFY_URL = 'https://challenges.cloudflare.com/turnstile/v0/siteverify';

const originalFetch = globalThis.fetch.bind(globalThis);

globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
  const url =
    typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url;

  if (url.startsWith('http://localhost/')) {
    return SELF.fetch(input, init);
  }

  // Accepts only TEST_TURNSTILE_TOKEN, sent with the test secret from vitest.config.ts.
  if (url === TURNSTILE_SITEVERIFY_URL) {
    const body = JSON.parse(String(init?.body ?? '{}')) as { secret?: string; response?: string };
    const success =
      body.secret === 'test-turnstile-secret' && body.response === TEST_TURNSTILE_TOKEN;
    return Promise.resolve(Response.json({ success }));
  }

  return originalFetch(input, init);
}) as typeof fetch;
