-- api/SPEC.md API-043: passwordless device sign-in. A device creates a pairing
-- holding the name/platform it chose plus a short user code; the owner approves
-- it from a web session, and the device collects its credentials on its next
-- poll. Rows are short lived (10 minutes) and pruned by the hourly cron.
CREATE TABLE device_auth_codes (
  id BLOB PRIMARY KEY,
  user_code TEXT NOT NULL UNIQUE,         -- normalized: uppercase, no separator
  device_code_hash TEXT NOT NULL UNIQUE,  -- sha256 hex, same shape as email_tokens
  name TEXT NOT NULL,
  platform TEXT NOT NULL,
  approved_by BLOB,                       -- users.id, set on approve
  approved_at INTEGER,
  consumed_at INTEGER,                    -- set when the device collects its token
  expires_at INTEGER NOT NULL,            -- ms, matching email_tokens
  created_at INTEGER NOT NULL,
  requested_from TEXT,                    -- coarse origin (API-044), best effort
  FOREIGN KEY (approved_by) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_device_auth_codes_expires_at ON device_auth_codes(expires_at);
