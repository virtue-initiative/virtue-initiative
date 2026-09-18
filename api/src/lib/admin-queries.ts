// api/SPEC.md API-054 / API-055: named preset queries and the guard around
// raw read-only SQL from admins.
import { ADMIN_QUERY_DEFAULT_LIMIT, type AdminQueryResult } from '../../../shared-web/types';
import { bytesToUuid } from './db';

// Never returned from admin queries, whatever the query asks for (API-055).
const SECRET_COLUMNS = new Set([
  'password_hash',
  'password_salt',
  'encrypted_priv_key',
  'wrapped_value',
  'access_keys',
]);

function isSecretColumn(name: string) {
  return SECRET_COLUMNS.has(name) || name.endsWith('token_hash');
}

export type AdminQueryPreset = {
  label: string;
  description: string;
  sql: string;
};

// Timestamps in this database are millisecond epochs, hence the /1000 below.
export const ADMIN_QUERY_PRESETS: Record<string, AdminQueryPreset> = {
  signups_by_day: {
    label: 'Signups by day',
    description: 'New accounts per UTC day over the last 30 days.',
    sql: `SELECT date(created_at / 1000, 'unixepoch') AS day, COUNT(*) AS signups
          FROM users
          WHERE created_at > (unixepoch() - 30 * 86400) * 1000
          GROUP BY day
          ORDER BY day DESC`,
  },
  batches_per_day: {
    label: 'Batches per day',
    description: 'Uploaded batches and distinct uploading users per UTC day over the last 30 days.',
    sql: `SELECT date(created_at / 1000, 'unixepoch') AS day,
                 COUNT(*) AS batches,
                 COUNT(DISTINCT user_id) AS users,
                 COUNT(DISTINCT device_id) AS devices
          FROM batches
          WHERE created_at > (unixepoch() - 30 * 86400) * 1000
          GROUP BY day
          ORDER BY day DESC`,
  },
  users_by_upload_activity: {
    label: 'Users by upload activity',
    description:
      'How many users last uploaded within each age bucket, plus users who never uploaded.',
    sql: `WITH last_upload AS (
            SELECT u.id, MAX(b.created_at) AS last_at
            FROM users u LEFT JOIN batches b ON b.user_id = u.id
            GROUP BY u.id
          )
          SELECT CASE
                   WHEN last_at IS NULL THEN 'never'
                   WHEN last_at > (unixepoch() - 1 * 86400) * 1000 THEN 'last 24 hours'
                   WHEN last_at > (unixepoch() - 7 * 86400) * 1000 THEN 'last 7 days'
                   WHEN last_at > (unixepoch() - 30 * 86400) * 1000 THEN 'last 30 days'
                   ELSE 'older than 30 days'
                 END AS bucket,
                 COUNT(*) AS users
          FROM last_upload
          GROUP BY bucket
          ORDER BY users DESC`,
  },
  devices_by_platform: {
    label: 'Devices by platform',
    description: 'Live devices per platform, with how many uploaded in the last 7 days.',
    sql: `SELECT d.platform,
                 COUNT(*) AS devices,
                 SUM(CASE WHEN EXISTS (
                   SELECT 1 FROM batches b
                   WHERE b.device_id = d.id AND b.created_at > (unixepoch() - 7 * 86400) * 1000
                 ) THEN 1 ELSE 0 END) AS active_7d
          FROM devices d
          WHERE d.deleted_at IS NULL
          GROUP BY d.platform
          ORDER BY devices DESC`,
  },
  top_uploaders_7d: {
    label: 'Top uploaders (7 days)',
    description: 'Users ranked by batches uploaded in the last 7 days.',
    sql: `SELECT u.email, COUNT(*) AS batches, MAX(b.created_at) AS last_upload_at
          FROM batches b JOIN users u ON u.id = b.user_id
          WHERE b.created_at > (unixepoch() - 7 * 86400) * 1000
          GROUP BY u.id
          ORDER BY batches DESC
          LIMIT 50`,
  },
  partner_status_counts: {
    label: 'Partner status counts',
    description: 'Partnership rows grouped by status.',
    sql: `SELECT status, COUNT(*) AS partnerships, COUNT(DISTINCT watching_user_id) AS watched_users
          FROM partners
          GROUP BY status
          ORDER BY partnerships DESC`,
  },
  recent_signups: {
    label: 'Recent signups',
    description: 'The 50 newest accounts with verification state and device count.',
    sql: `SELECT u.email, u.name, u.email_verified, u.created_at,
                 (SELECT COUNT(*) FROM devices d WHERE d.owner = u.id AND d.deleted_at IS NULL) AS devices
          FROM users u
          ORDER BY u.created_at DESC
          LIMIT 50`,
  },
  active_web_sessions: {
    label: 'Active web sessions',
    description: 'Unexpired web sessions per user, newest session first.',
    sql: `SELECT u.email, COUNT(*) AS sessions, MAX(s.created_at) AS newest_session_at
          FROM user_sessions s JOIN users u ON u.id = s.user_id
          WHERE s.expires_at > unixepoch() * 1000
          GROUP BY u.id
          ORDER BY newest_session_at DESC
          LIMIT 100`,
  },
};

export function listAdminQueryPresets() {
  return Object.entries(ADMIN_QUERY_PRESETS).map(([name, preset]) => ({
    name,
    label: preset.label,
    description: preset.description,
  }));
}

export class InvalidAdminQueryError extends Error {}

export class FullScanError extends Error {
  constructor(
    public readonly tables: string[],
    public readonly plan: string[],
  ) {
    super(`Query would scan a large table: ${tables.join(', ')}`);
  }
}

// D1 bills per row read, and these are the tables where a full scan is the
// difference between a few rows and millions. Everything else is small enough
// not to care about.
const LARGE_TABLES = ['batches'];

// Table names and any aliases the query gives them, since EXPLAIN QUERY PLAN
// reports "SCAN b" rather than "SCAN batches AS b".
function namesForTable(table: string, sql: string) {
  const names = new Set([table]);
  const aliasPattern = new RegExp(`\\b${table}\\s+(?:AS\\s+)?([A-Za-z_][A-Za-z0-9_]*)`, 'gi');
  const reserved = new Set([
    'where',
    'on',
    'join',
    'left',
    'inner',
    'cross',
    'group',
    'order',
    'limit',
    'set',
    'using',
    'natural',
    'union',
    'having',
  ]);
  for (const match of sql.matchAll(aliasPattern)) {
    const alias = match[1].toLowerCase();
    if (!reserved.has(alias)) names.add(alias);
  }
  return names;
}

// Reads no table rows itself; a plan line starting with "SCAN <name>" is a
// full scan of that table (or of a covering index over all of it).
export async function findFullScans(db: D1Database, sql: string) {
  const result = await db.prepare(`EXPLAIN QUERY PLAN ${sql}`).all<{ detail: string }>();
  const plan = result.results.map((row) => row.detail);
  const lowered = sql.toLowerCase();
  const tables = LARGE_TABLES.filter((table) => {
    if (!lowered.includes(table)) return false;
    const names = namesForTable(table, sql);
    return plan.some((line) => {
      const scanned = /^SCAN (\S+)/.exec(line)?.[1]?.toLowerCase();
      return scanned !== undefined && names.has(scanned);
    });
  });
  return { plan, tables };
}

function stripComments(sql: string) {
  return sql.replace(/\/\*[\s\S]*?\*\//g, ' ').replace(/--[^\n]*/g, ' ');
}

// Wraps any SELECT so the result is bounded to `limit` rows, whatever LIMIT
// the query itself carries. One extra row tells us whether it was truncated.
function bounded(sql: string, limit: number) {
  return `SELECT * FROM (${sql}) LIMIT ${limit + 1}`;
}

// Accepts a single SELECT/WITH statement and nothing else. Returns the SQL
// rewritten so the parser itself guarantees a SELECT and a bounded result.
export function guardReadOnlySql(input: string, limit = ADMIN_QUERY_DEFAULT_LIMIT) {
  let sql = stripComments(input).trim();
  if (sql.endsWith(';')) {
    sql = sql.slice(0, -1).trimEnd();
  }
  if (!sql) {
    throw new InvalidAdminQueryError('Query is empty');
  }
  if (sql.includes(';')) {
    throw new InvalidAdminQueryError('Only a single statement is allowed');
  }
  if (!/^(select|with)\b/i.test(sql)) {
    throw new InvalidAdminQueryError('Only SELECT or WITH queries are allowed');
  }
  return bounded(sql, limit);
}

// D1 hands BLOB values back as ArrayBuffers from .all() but as plain number
// arrays from .raw(); no column type ever yields a JSON array, so an array of
// bytes here is always a BLOB.
function asBytes(value: unknown): Uint8Array | null {
  if (value instanceof ArrayBuffer) {
    return new Uint8Array(value);
  }
  if (ArrayBuffer.isView(value)) {
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  }
  if (Array.isArray(value) && value.every((v) => typeof v === 'number')) {
    return Uint8Array.from(value as number[]);
  }
  return null;
}

function serializeValue(value: unknown): unknown {
  const bytes = asBytes(value);
  if (!bytes) {
    return value;
  }
  if (bytes.byteLength === 16) {
    return bytesToUuid(bytes);
  }
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

async function executeQuery(db: D1Database, sql: string, limit: number): Promise<AdminQueryResult> {
  // .all() rather than .raw() so we get meta.rows_read, the number D1 bills.
  // The SELECT * wrapper makes SQLite rename duplicate column names ("id:1"),
  // so object keys are a faithful column list; with zero rows it is empty.
  const result = await db.prepare(sql).all<Record<string, unknown>>();
  const rows = result.results;
  const allColumns = rows.length > 0 ? Object.keys(rows[0]) : [];
  const columns = allColumns.filter((name) => !isSecretColumn(name));
  const truncated = rows.length > limit;

  return {
    columns,
    rows: rows.slice(0, limit).map((row) => columns.map((name) => serializeValue(row[name]))),
    truncated,
    rows_read: result.meta.rows_read ?? 0,
  };
}

export async function runPresetQuery(
  db: D1Database,
  name: string,
  limit = ADMIN_QUERY_DEFAULT_LIMIT,
) {
  const preset = ADMIN_QUERY_PRESETS[name];
  if (!preset) {
    return null;
  }
  return executeQuery(db, bounded(preset.sql, limit), limit);
}

export async function runReadOnlyQuery(
  db: D1Database,
  sql: string,
  options: { limit?: number; allowScan?: boolean } = {},
) {
  const limit = options.limit ?? ADMIN_QUERY_DEFAULT_LIMIT;
  const guarded = guardReadOnlySql(sql, limit);
  if (!options.allowScan) {
    const { plan, tables } = await findFullScans(db, guarded);
    if (tables.length > 0) {
      throw new FullScanError(tables, plan);
    }
  }
  return executeQuery(db, guarded, limit);
}
