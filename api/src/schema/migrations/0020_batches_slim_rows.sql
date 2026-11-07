-- Shrinks each batches row by about a third, to stay further from D1's 10 GB
-- database limit:
--
-- * `url` is dropped. It is derived from the batch's version, owner and id
--   (API-038, lib/batch-url.ts).
-- * `end_hash` becomes the 32 raw bytes instead of 64 hex characters.
-- * `access_keys` becomes a BLOB holding msgpack (lib/access-keys.ts). Existing
--   rows keep their JSON text, which the API still reads. They age out with the
--   30-day batch retention.
-- * The index on (device_id, end_time) and the foreign key on device_id are
--   dropped. The index cost a written row on every batch. Without it the
--   foreign key would make every device delete scan the whole table for
--   children, so deleteDeviceById removes a device's batches itself, through
--   the owner's primary key range.
-- * The index on created_at is not recreated. It too cost a written row on
--   every batch. Retention cleanup now deletes each user's expired batches
--   through the primary key, where they sit at the front of the user's range
--   (lib/retention.ts).
PRAGMA defer_foreign_keys = true;

-- Stop here unless every stored url is exactly what the API will now derive.
-- The CHECK fails, and the migration with it, if any row differs.
CREATE TABLE batches_url_guard (mismatched INTEGER NOT NULL CHECK (mismatched = 0));
INSERT INTO batches_url_guard
SELECT COUNT(*)
FROM (
  SELECT url,
         '/user/'
         || substr(u, 1, 8) || '-' || substr(u, 9, 4) || '-' || substr(u, 13, 4) || '-'
         || substr(u, 17, 4) || '-' || substr(u, 21)
         || '/batches/'
         || substr(i, 1, 8) || '-' || substr(i, 9, 4) || '-' || substr(i, 13, 4) || '-'
         || substr(i, 17, 4) || '-' || substr(i, 21)
         || '.enc' AS derived
  FROM (SELECT url, lower(hex(user_id)) AS u, lower(hex(id)) AS i FROM batches)
)
WHERE substr(url, -length(derived)) != derived;
DROP TABLE batches_url_guard;

CREATE TABLE batches_new (
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
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  PRIMARY KEY (user_id, created_at, id),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
) WITHOUT ROWID;

-- unhex() returns NULL for anything that is not hex, which the NOT NULL on
-- end_hash turns into a failed migration instead of a silently lost hash.
INSERT INTO batches_new (
  id, user_id, device_id, start_time, end_time, end_hash, access_keys,
  version, high_risk_count, medium_risk_count, created_at
)
SELECT id, user_id, device_id, start_time, end_time, unhex(end_hash), access_keys,
       version, high_risk_count, medium_risk_count, created_at
FROM batches;

DROP TABLE batches;
ALTER TABLE batches_new RENAME TO batches;
