-- api/SPEC.md API-043: a self-service secret vault, scoped to its owner only.
-- Reading it is a deliberate tripwire (see API-046), and delete is soft
-- (API-047/048/049) so the client can offer restore and a 7-day undo window
-- before the retention job in lib/retention.ts hard-deletes it.
CREATE TABLE locked_passwords (
  id BLOB PRIMARY KEY,
  owner_id BLOB NOT NULL,
  label TEXT NOT NULL,
  wrapped_value TEXT NOT NULL,
  accessed_at INTEGER,
  deleted_at INTEGER,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (owner_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_locked_passwords_owner_id ON locked_passwords(owner_id);
CREATE INDEX idx_locked_passwords_deleted_at ON locked_passwords(deleted_at);
