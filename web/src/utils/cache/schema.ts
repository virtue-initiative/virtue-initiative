// Declarative schema for the OPFS-backed SQLite log cache.
//
// The DDL lives here rather than inline in worker.ts so that drift detection is a pure
// function that can be unit-tested without OPFS. Every table is created with
// CREATE TABLE IF NOT EXISTS, which is a no-op against an existing database, so a cache
// provisioned by an older release keeps its old column set forever. That is what produced
// the "no such column" failures this module guards against: the fix is to detect the
// mismatch on open and rebuild the cache from scratch.
//
// Bump CACHE_SCHEMA_VERSION whenever a table's DDL changes.
export const CACHE_SCHEMA_VERSION = 1;

export type TableSchema = {
  name: string;
  ddl: string;
  columns: string[];
};

export const CACHE_TABLES: TableSchema[] = [
  {
    name: 'feeds',
    ddl: `CREATE TABLE IF NOT EXISTS feeds (
       viewer_id TEXT NOT NULL,
       target_user_id TEXT NOT NULL,
       since INTEGER,
       PRIMARY KEY (viewer_id, target_user_id)
     )`,
    columns: ['viewer_id', 'target_user_id', 'since'],
  },
  {
    name: 'batches',
    ddl: `CREATE TABLE IF NOT EXISTS batches (
       id TEXT PRIMARY KEY,
       viewer_id TEXT NOT NULL,
       target_user_id TEXT NOT NULL,
       device_id TEXT NOT NULL,
       url TEXT NOT NULL,
       encrypted_key TEXT NOT NULL,
       start_time INTEGER NOT NULL,
       end_time INTEGER NOT NULL,
       created_at INTEGER NOT NULL,
       start_hash TEXT,
       end_hash TEXT,
       version TEXT
     )`,
    columns: [
      'id',
      'viewer_id',
      'target_user_id',
      'device_id',
      'url',
      'encrypted_key',
      'start_time',
      'end_time',
      'created_at',
      'start_hash',
      'end_hash',
      'version',
    ],
  },
  {
    name: 'materialized_batch_ids',
    ddl: `CREATE TABLE IF NOT EXISTS materialized_batch_ids (
       batch_id TEXT PRIMARY KEY
     )`,
    columns: ['batch_id'],
  },
  {
    name: 'events',
    ddl: `CREATE TABLE IF NOT EXISTS events (
       id TEXT PRIMARY KEY,
       viewer_id TEXT NOT NULL,
       target_user_id TEXT NOT NULL,
       device_id TEXT NOT NULL,
       ts INTEGER NOT NULL,
       type TEXT NOT NULL,
       data TEXT NOT NULL,
       risk REAL,
       batch_status TEXT,
       source TEXT NOT NULL DEFAULT 'batch',
       image_w INTEGER,
       image_h INTEGER,
       created_at INTEGER NOT NULL DEFAULT 0
     )`,
    columns: [
      'id',
      'viewer_id',
      'target_user_id',
      'device_id',
      'ts',
      'type',
      'data',
      'risk',
      'batch_status',
      'source',
      'image_w',
      'image_h',
      'created_at',
    ],
  },
  {
    name: 'event_images',
    ddl: `CREATE TABLE IF NOT EXISTS event_images (
       event_id TEXT PRIMARY KEY,
       data BLOB NOT NULL
     )`,
    columns: ['event_id', 'data'],
  },
  {
    name: 'failed_batch_ids',
    ddl: `CREATE TABLE IF NOT EXISTS failed_batch_ids (
       batch_id TEXT PRIMARY KEY,
       error TEXT
     )`,
    columns: ['batch_id', 'error'],
  },
];

/**
 * Report why the cache must be rebuilt, or null when it is up to date.
 *
 * `existing` maps every table present in the database to its column names, as read from
 * PRAGMA table_info. The column check is deliberately redundant with the version check: it
 * catches drift even when a schema change forgets to bump CACHE_SCHEMA_VERSION, which is
 * exactly how the original bug shipped.
 *
 * An empty map means the database has no tables at all, so there is nothing to migrate and
 * nothing to throw away. That is the ordinary first-run case, including the run right after
 * the cache is cleared, and it must not be reported as drift: user_version reads 0 there
 * only because 0 is SQLite's default for a fresh database.
 */
export function findSchemaDrift(
  userVersion: number,
  existing: Map<string, Set<string>>,
): string | null {
  if (existing.size === 0) return null;
  if (userVersion !== CACHE_SCHEMA_VERSION) {
    return `schema version ${userVersion} does not match expected ${CACHE_SCHEMA_VERSION}`;
  }
  for (const table of CACHE_TABLES) {
    const columns = existing.get(table.name);
    if (!columns) return `table "${table.name}" is missing`;
    for (const column of table.columns) {
      if (!columns.has(column)) {
        return `table "${table.name}" is missing column "${column}"`;
      }
    }
  }
  return null;
}
