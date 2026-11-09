-- api/SPEC.md API-061: one row per flag. The API caches this table in memory,
-- so evaluating flags on a request reads it at most once a minute per isolate.
CREATE TABLE feature_flags (
  key TEXT PRIMARY KEY,
  rollout_percent INTEGER NOT NULL DEFAULT 0 CHECK (rollout_percent BETWEEN 0 AND 100),
  description TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- Per-user overrides, {"flag_key": true | false}. Kept out of `settings`, which
-- the user can write through PATCH /user (API-020). Read alongside the user row
-- the request already loads, so overrides cost no extra query.
ALTER TABLE users ADD COLUMN feature_flags TEXT NOT NULL DEFAULT '{}';
