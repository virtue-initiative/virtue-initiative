-- api/SPEC.md API-051: admin status lives only in this table and is granted by
-- hand (see api/CLAUDE.md), never through the API.
CREATE TABLE admins (
  user_id BLOB PRIMARY KEY,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- One AnalyticsMetrics JSON blob per UTC day, written by the daily cron and
-- replaced by POST /admin/analytics/refresh (API-053).
CREATE TABLE analytics_snapshots (
  day TEXT PRIMARY KEY,
  metrics TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
