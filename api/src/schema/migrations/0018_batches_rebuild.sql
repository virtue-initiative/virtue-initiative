-- Every index on batches is a billed row write on each upload, and batches is
-- by far the most-written table. This rebuild removes two of them:
--
-- * `url UNIQUE` kept an index nothing reads. No query looks a batch up by url,
--   and the url is built from the batch id, which is already unique.
-- * `id BLOB PRIMARY KEY` on a rowid table is a second index beside the table.
--   WITHOUT ROWID stores the rows in the primary key's own b-tree instead.
--
-- An insert goes from 6 written rows to 4. SQLite cannot drop a UNIQUE
-- constraint or change a table's rowid-ness in place, so the table is copied.
-- Nothing references batches, so dropping the old table cascades nowhere.
PRAGMA defer_foreign_keys = true;

CREATE TABLE batches_new (
  id BLOB PRIMARY KEY,
  user_id BLOB NOT NULL,
  device_id BLOB NOT NULL,
  url TEXT NOT NULL,
  start_time INTEGER NOT NULL,
  end_time INTEGER NOT NULL,
  end_hash TEXT NOT NULL,
  access_keys TEXT NOT NULL,
  version TEXT NOT NULL DEFAULT '',
  high_risk_count INTEGER NOT NULL DEFAULT 0,
  medium_risk_count INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
) WITHOUT ROWID;

INSERT INTO batches_new (
  id, user_id, device_id, url, start_time, end_time, end_hash, access_keys,
  version, high_risk_count, medium_risk_count, created_at
)
SELECT id, user_id, device_id, url, start_time, end_time, end_hash, access_keys,
       version, high_risk_count, medium_risk_count, created_at
FROM batches;

DROP TABLE batches;
ALTER TABLE batches_new RENAME TO batches;

CREATE INDEX idx_batches_created_at ON batches(created_at);
CREATE INDEX idx_batches_user_id_created_at ON batches(user_id, created_at);
CREATE INDEX idx_batches_device_id_end_time ON batches(device_id, end_time);
