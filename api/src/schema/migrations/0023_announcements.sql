-- api/SPEC.md API-068: messages shown in the client apps. The API caches this
-- table in memory, so choosing a device's announcement reads it at most once a
-- minute per isolate.
CREATE TABLE announcements (
  id BLOB PRIMARY KEY,
  message TEXT NOT NULL,
  url TEXT,
  level TEXT NOT NULL CHECK (level IN ('info', 'warning', 'urgent')),
  -- JSON array of platform strings, or NULL for every platform.
  platforms TEXT,
  below_version TEXT,
  created_at INTEGER NOT NULL
);
