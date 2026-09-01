/// <reference lib="webworker" />
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import sqlite3InitModule from '@sqlite.org/sqlite-wasm';
import { decryptAndFlattenBatch, DecryptionError } from '../api/batch-materializer';
import { unwrapBatchKey } from '../api/crypto';
import { createNativeBatchKeyUnwrapper } from '../api/hpke-native';
import type { Batch, DataPage } from '../api/api';
import type { FeedLog } from '../../pages/Logs/types';
import { CACHE_SCHEMA_VERSION, CACHE_TABLES, findSchemaDrift } from './schema';

export type {};

// ---------------------------------------------------------------------------
// Types

type WorkerCacheQuery = {
  userId?: string;
  deviceId?: string;
  startTime?: number;
  endTime?: number;
  eventTypes?: string[];
};

type CacheRequest =
  | {
      id: string;
      method: 'setSession';
      userId: string;
      privateKey: CryptoKey | null;
    }
  | { id: string; method: 'cacheQuery'; query: WorkerCacheQuery; targetUserId: string }
  | { id: string; method: 'refetch' }
  | { id: string; method: 'clearCache' }
  | { id: string; method: 'deleteDeviceData'; viewerId: string; deviceId: string }
  | { id: string; method: 'getEventImage'; eventId: string }
  | {
      id: string;
      method: 'getDeviceBatchEndTimes';
      viewerId: string;
      targetUserId: string;
      deviceId: string;
    }
  | {
      id: string;
      method: 'getDecryptionStats';
      viewerId: string;
      targetUserId: string;
      deviceId?: string;
      startTime?: number;
      endTime?: number;
    };

export type DecryptionStats = {
  totalBatches: number;
  decryptedBatches: number;
  failedBatches: number;
  failureReasons: { error: string; count: number }[];
  totalEvents: number;
  totalScreenshots: number;
};

type CacheResponse = { id: string; result: unknown } | { id: string; error: string };
type CacheChunk = {
  type: 'queryChunk';
  id: string;
  logs: FeedLog[];
  done: boolean;
  processed: number;
  total: number;
  // 'replace' → authoritative snapshot (cached fast-path, final result); the consumer swaps
  // its log set. 'append' → incremental delta; the consumer merges it into the existing set.
  mode: 'replace' | 'append';
};
// Lightweight sync-progress signal: carries only the block counts, no log payload, so it
// stays a fixed ~tiny size no matter how much data has accumulated. The full log set is
// only ever shipped on the fast-path chunk and the final done chunk.
type CacheProgress = { type: 'queryProgress'; id: string; processed: number; total: number };

// ---------------------------------------------------------------------------
// SQLite / OPFS

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let db: any = null;
// The SAH pool VFS can only be installed once per worker, so the util object is kept here
// rather than being a local of openDatabase(). It also exposes wipeFiles(), which is how
// hardReset() empties the OPFS backing files without going through SQLite.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
let poolUtil: any = null;

// Directory the SAH pool VFS stores its backing files in: "." + vfsName, with the default
// vfsName being "opfs-sahpool". Removing it is the fallback when poolUtil is unavailable
// because init itself failed.
const SAH_POOL_DIR = '.opfs-sahpool';

function createSchema(): void {
  for (const table of CACHE_TABLES) {
    db.exec(table.ddl);
  }
}

// Every table the database currently holds, declared or not, mapped to its column names.
// Undeclared tables are included so that an empty map means "brand-new database" and not
// merely "none of the tables we know about".
function readExistingSchema(): Map<string, Set<string>> {
  const existing = new Map<string, Set<string>>();
  for (const name of listTables()) {
    const columns = new Set<string>();
    db.exec(`PRAGMA table_info("${name}")`, {
      rowMode: 'object',
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      callback: (row: any) => columns.add(row.name as string),
    });
    existing.set(name, columns);
  }
  return existing;
}

function listTables(): string[] {
  const names: string[] = [];
  db.exec(`SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'`, {
    rowMode: 'array',
    callback: (row: [string]) => names.push(row[0]),
  });
  return names;
}

// Drop every table the database currently holds, not just the declared ones, so stale
// tables from older releases (e.g. direct_logs, removed in #467) go too.
function dropAllTables(): void {
  for (const name of listTables()) {
    db.exec(`DROP TABLE IF EXISTS "${name}"`);
  }
}

async function openDatabase(): Promise<void> {
  if (!poolUtil) {
    console.log('[cache-worker] init: loading sqlite3 wasm');
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const sqlite3 = (await sqlite3InitModule()) as any;
    console.log('[cache-worker] init: installing OPFS SAH pool VFS');
    // Must open the DB via the pool returned here. Opening with sqlite3.oo1.OpfsDb instead
    // silently falls back to the default OPFS VFS, which proxies every I/O to a separate
    // worker and blocks on Atomics + an fsync per commit — ~100s of ms per write, which
    // throttles the whole decrypt pipeline to a crawl regardless of fetch concurrency.
    poolUtil = await sqlite3.installOpfsSAHPoolVfs({});
  }
  console.log('[cache-worker] init: opening /cache.db');
  db = new poolUtil.OpfsSAHPoolDb('/cache.db');

  // Inspect before creating anything: a cache provisioned by an older release keeps its old
  // table definitions, since CREATE TABLE IF NOT EXISTS is a no-op there and there is no
  // ALTER TABLE path. Rebuild on a mismatch rather than letting every query that names a
  // newer column fail with "no such column". A brand-new database reports no drift: it has
  // no tables yet, and its user_version is 0 only because that is SQLite's default.
  const userVersion = (db.selectValue('PRAGMA user_version') as number | undefined) ?? 0;
  const drift = findSchemaDrift(userVersion, readExistingSchema());
  if (drift) {
    console.warn('[cache-worker] init: rebuilding cache, schema drift:', drift);
    dropAllTables();
  }
  createSchema();
  db.exec(`PRAGMA user_version = ${CACHE_SCHEMA_VERSION}`);
  console.log('[cache-worker] init: done');
}

// Empty the cache and start over, for the clearCache request that logout issues. This is
// the in-worker path; a full reset (client-side termination plus deleting the OPFS
// directory) is what the user-facing button does, since that also survives a wedged worker.
async function hardReset(): Promise<void> {
  try {
    db?.close();
  } catch (err) {
    console.warn('[cache-worker] reset: closing db failed', err);
  }
  db = null;

  if (poolUtil) {
    await poolUtil.wipeFiles();
  } else {
    // Init never got far enough to install the VFS; remove its directory directly.
    const root = await navigator.storage.getDirectory();
    await root.removeEntry(SAH_POOL_DIR, { recursive: true }).catch((err: unknown) => {
      console.warn('[cache-worker] reset: removing', SAH_POOL_DIR, 'failed', err);
    });
  }

  await openDatabase();
}

// Mutable so a hard reset can replace it. Failures are surfaced to callers in
// self.onmessage rather than swallowed, so a wedged init can't leave requests hanging.
let initPromise: Promise<void> = openDatabase();
initPromise.catch((err) => console.error('[cache-worker] init FAILED', err));

// ---------------------------------------------------------------------------
// SQLite helpers

function sqlGetSince(viewerId: string, targetUserId: string): number {
  return (
    (db.selectValue('SELECT since FROM feeds WHERE viewer_id=? AND target_user_id=?', [
      viewerId,
      targetUserId,
    ]) as number | undefined) ?? 0
  );
}

function sqlMergeDataPage(viewerId: string, targetUserId: string, page: DataPage): void {
  db.exec('BEGIN');
  try {
    for (const batch of page.batches) {
      db.exec(
        `INSERT OR IGNORE INTO batches(id,viewer_id,target_user_id,device_id,url,encrypted_key,start_time,end_time,created_at,end_hash,version)
         VALUES(?,?,?,?,?,?,?,?,?,?,?)`,
        {
          bind: [
            batch.id,
            viewerId,
            targetUserId,
            batch.device_id,
            batch.url,
            batch.encrypted_key,
            batch.start_time,
            batch.end_time,
            batch.created_at,
            batch.end_hash,
            batch.version,
          ],
        },
      );
    }
    const allCreatedAts = [...page.batches.map((b) => b.created_at)];
    if (allCreatedAts.length > 0) {
      const maxCreatedAt = Math.max(...allCreatedAts);
      db.exec(
        `INSERT INTO feeds(viewer_id,target_user_id,since) VALUES(?,?,?)
         ON CONFLICT(viewer_id,target_user_id) DO UPDATE SET since=MAX(since,?)`,
        { bind: [viewerId, targetUserId, maxCreatedAt, maxCreatedAt] },
      );
    } else {
      db.exec(`INSERT OR IGNORE INTO feeds(viewer_id,target_user_id,since) VALUES(?,?,0)`, {
        bind: [viewerId, targetUserId],
      });
    }
    db.exec('COMMIT');
  } catch (e) {
    db.exec('ROLLBACK');
    throw e;
  }
}

function sqlGetUnmaterializedBatches(
  viewerId: string,
  targetUserId: string,
  cutoffTs: number,
): Batch[] {
  const batches: Batch[] = [];
  db.exec(
    `SELECT b.* FROM batches b
     WHERE b.viewer_id=? AND b.target_user_id=? AND b.created_at>=?
     AND b.id NOT IN (SELECT batch_id FROM materialized_batch_ids)
     AND b.id NOT IN (SELECT batch_id FROM failed_batch_ids)`,
    {
      bind: [viewerId, targetUserId, cutoffTs],
      rowMode: 'object',
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      callback: (row: any) =>
        batches.push({
          id: row.id as string,
          device_id: row.device_id as string,
          url: row.url as string,
          encrypted_key: row.encrypted_key as string,
          start_time: row.start_time as number,
          end_time: row.end_time as number,
          created_at: row.created_at as number,
          end_hash: (row.end_hash as string | null) ?? '0'.repeat(64),
          version: (row.version as string | null) ?? 'v0.1',
        }),
    },
  );
  return batches;
}

function sqlWriteMaterializedEvents(
  viewerId: string,
  targetUserId: string,
  batchId: string,
  events: FeedLog[],
): void {
  db.exec('BEGIN');
  try {
    for (const event of events) {
      const imageData = event.data.image instanceof Uint8Array ? event.data.image : undefined;
      const dataWithoutImage = { ...event.data };
      delete dataWithoutImage.image;

      db.exec(
        `INSERT OR REPLACE INTO events(id,viewer_id,target_user_id,device_id,ts,type,data,risk,batch_status,source,image_w,image_h,created_at)
         VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)`,
        {
          bind: [
            event.id,
            viewerId,
            targetUserId,
            event.device_id,
            event.ts,
            event.type,
            JSON.stringify(dataWithoutImage),
            event.risk ?? null,
            event.batch_status ?? null,
            event.source ?? 'batch',
            event.image_w ?? null,
            event.image_h ?? null,
            event.created_at,
          ],
        },
      );

      if (imageData) {
        db.exec(`INSERT OR REPLACE INTO event_images(event_id,data) VALUES(?,?)`, {
          bind: [event.id, imageData],
        });
      }
    }

    db.exec(`INSERT OR IGNORE INTO materialized_batch_ids(batch_id) VALUES(?)`, {
      bind: [batchId],
    });
    db.exec('COMMIT');
  } catch (e) {
    db.exec('ROLLBACK');
    throw e;
  }
}

function sqlQueryEvents(
  viewerId: string,
  targetUserId: string,
  query: WorkerCacheQuery,
): FeedLog[] {
  let sql = `SELECT * FROM events WHERE viewer_id=? AND target_user_id=?`;
  const bind: unknown[] = [viewerId, targetUserId];

  if (query.deviceId) {
    sql += ` AND device_id=?`;
    bind.push(query.deviceId);
  }
  if (query.startTime !== undefined && query.endTime !== undefined) {
    sql += ` AND ts BETWEEN ? AND ?`;
    bind.push(query.startTime, query.endTime);
  }

  const results: FeedLog[] = [];
  db.exec(sql, {
    bind,
    rowMode: 'object',
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    callback: (row: any) =>
      results.push({
        id: row.id as string,
        device_id: row.device_id as string,
        ts: row.ts as number,
        type: row.type as string,
        data: JSON.parse(row.data as string) as Record<string, unknown>,
        created_at: row.created_at as number,
        risk: row.risk != null ? (row.risk as number) : undefined,
        batch_status: ((row.batch_status as string | null) ?? 'unknown') as FeedLog['batch_status'],
        source: ((row.source as string | null) ?? 'batch') as FeedLog['source'],
        image_w: row.image_w != null ? (row.image_w as number) : undefined,
        image_h: row.image_h != null ? (row.image_h as number) : undefined,
      }),
  });

  let combined = results.sort((a, b) => b.ts - a.ts);
  if (query.eventTypes && query.eventTypes.length > 0) {
    const allowed = new Set(query.eventTypes);
    combined = combined.filter((log) => allowed.has(log.type));
  }
  return combined;
}

function sqlPruneOldData(viewerId: string, cutoffTs: number): void {
  db.exec(`DELETE FROM events WHERE viewer_id=? AND ts<?`, { bind: [viewerId, cutoffTs] });
}

function sqlDeleteDeviceData(viewerId: string, deviceId: string): void {
  db.exec(`DELETE FROM events WHERE viewer_id=? AND device_id=?`, {
    bind: [viewerId, deviceId],
  });
  db.exec(`DELETE FROM batches WHERE viewer_id=? AND device_id=?`, {
    bind: [viewerId, deviceId],
  });
}

function sqlGetEventImage(eventId: string): Uint8Array | null {
  const result = db.selectValue(`SELECT data FROM event_images WHERE event_id=?`, [eventId]);
  return (result as Uint8Array | undefined) ?? null;
}

function sqlGetDeviceBatchEndTimes(
  viewerId: string,
  targetUserId: string,
  deviceId: string,
): number[] {
  const times: number[] = [];
  db.exec(
    `SELECT end_time FROM batches WHERE viewer_id=? AND target_user_id=? AND device_id=? ORDER BY end_time ASC`,
    {
      bind: [viewerId, targetUserId, deviceId],
      rowMode: 'array',
      callback: (row: [number]) => times.push(row[0]),
    },
  );
  return times;
}

function sqlGetDecryptionStats(
  viewerId: string,
  targetUserId: string,
  deviceId?: string,
  startTime?: number,
  endTime?: number,
): DecryptionStats {
  let batchWhere = `b.viewer_id=? AND b.target_user_id=?`;
  const batchBind: unknown[] = [viewerId, targetUserId];
  if (deviceId) {
    batchWhere += ` AND b.device_id=?`;
    batchBind.push(deviceId);
  }
  if (startTime !== undefined && endTime !== undefined) {
    batchWhere += ` AND b.start_time<=? AND b.end_time>=?`;
    batchBind.push(endTime, startTime);
  }

  const totalBatches =
    (db.selectValue(`SELECT COUNT(*) FROM batches b WHERE ${batchWhere}`, batchBind) as
      | number
      | undefined) ?? 0;

  const decryptedBatches =
    (db.selectValue(
      `SELECT COUNT(*) FROM batches b JOIN materialized_batch_ids m ON b.id=m.batch_id WHERE ${batchWhere}`,
      batchBind,
    ) as number | undefined) ?? 0;

  const failedBatches =
    (db.selectValue(
      `SELECT COUNT(*) FROM batches b JOIN failed_batch_ids f ON b.id=f.batch_id WHERE ${batchWhere}`,
      batchBind,
    ) as number | undefined) ?? 0;

  const failureReasons: { error: string; count: number }[] = [];
  db.exec(
    `SELECT f.error AS error, COUNT(*) AS c FROM batches b JOIN failed_batch_ids f ON b.id=f.batch_id
     WHERE ${batchWhere} GROUP BY f.error ORDER BY c DESC LIMIT 10`,
    {
      bind: batchBind,
      rowMode: 'object',
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      callback: (row: any) =>
        failureReasons.push({
          error: (row.error as string | null) ?? 'Unknown error',
          count: row.c as number,
        }),
    },
  );

  let eventWhere = `viewer_id=? AND target_user_id=?`;
  const eventBind: unknown[] = [viewerId, targetUserId];
  if (deviceId) {
    eventWhere += ` AND device_id=?`;
    eventBind.push(deviceId);
  }
  if (startTime !== undefined && endTime !== undefined) {
    eventWhere += ` AND ts BETWEEN ? AND ?`;
    eventBind.push(startTime, endTime);
  }

  const totalEvents =
    (db.selectValue(`SELECT COUNT(*) FROM events WHERE ${eventWhere}`, eventBind) as
      | number
      | undefined) ?? 0;
  const totalScreenshots =
    (db.selectValue(
      `SELECT COUNT(*) FROM events WHERE ${eventWhere} AND image_w IS NOT NULL`,
      eventBind,
    ) as number | undefined) ?? 0;

  return {
    totalBatches,
    decryptedBatches,
    failedBatches,
    failureReasons,
    totalEvents,
    totalScreenshots,
  };
}

function sqlMarkBatchFailed(batchId: string, error: string): void {
  db.exec(`INSERT OR IGNORE INTO failed_batch_ids(batch_id, error) VALUES(?, ?)`, {
    bind: [batchId, error],
  });
}

// ---------------------------------------------------------------------------
// Decryption pipeline state

const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000;
// Decryption is dominated by per-batch network fetches (one R2 GET each) plus async
// WebCrypto, so it's I/O-bound — a high concurrency cuts wall-clock time. R2 is HTTP/2,
// so this is well above the legacy 6-connection-per-host cap.
const DECRYPT_CONCURRENCY = 24;
// Frequent counts-only progress ticks for a smooth status-line counter.
const PROGRESS_THROTTLE_MS = 250;
// Coarser interval for shipping the actual newly-decrypted logs as a delta, so logs fill in
// progressively during a long sync without re-sending (and re-querying) the whole growing
// result set each time. Only events materialized since the previous flush cross postMessage.
const DELTA_INTERVAL_MS = 5000;
const BASE =
  ((import.meta as { env?: { VITE_API_URL?: string } }).env?.VITE_API_URL ??
    'http://localhost:8787') + `/${CURRENT_API_VERSION}`;

let session: { userId: string; privateKey: CryptoKey | null } | null = null;

// queryId → query metadata; kept until done:true is posted
const activeQueries = new Map<string, { query: WorkerCacheQuery; targetUserId: string }>();

// targetUserId → in-flight fetch promise (deduplication)
const fetchInFlight = new Map<string, Promise<void>>();

// all target user IDs ever seen (so refetch can cover them all)
const knownTargetUserIds = new Set<string>();

// ---------------------------------------------------------------------------
// Helpers

function postChunk(
  id: string,
  logs: FeedLog[],
  done: boolean,
  processed = 0,
  total = 0,
  mode: 'replace' | 'append' = 'replace',
): void {
  (self as unknown as DedicatedWorkerGlobalScope).postMessage({
    type: 'queryChunk',
    id,
    logs,
    done,
    processed,
    total,
    mode,
  } satisfies CacheChunk);
}

function postProgress(id: string, processed: number, total: number): void {
  (self as unknown as DedicatedWorkerGlobalScope).postMessage({
    type: 'queryProgress',
    id,
    processed,
    total,
  } satisfies CacheProgress);
}

// JS mirror of the WHERE clauses in sqlQueryEvents, used to filter in-memory delta events
// for a query without re-hitting SQLite.
function matchesQuery(log: FeedLog, query: WorkerCacheQuery): boolean {
  if (query.deviceId && log.device_id !== query.deviceId) return false;
  if (query.startTime !== undefined && query.endTime !== undefined) {
    if (log.ts < query.startTime || log.ts > query.endTime) return false;
  }
  if (query.eventTypes && query.eventTypes.length > 0 && !query.eventTypes.includes(log.type)) {
    return false;
  }
  return true;
}

async function fetchData(params?: { since?: number }): Promise<DataPage> {
  const qs = new URLSearchParams();
  if (params?.since !== undefined) qs.set('since', String(params.since));
  const q = qs.toString();
  const res = await fetch(`${BASE}/data${q ? `?${q}` : ''}`, {
    credentials: 'include',
  });
  if (!res.ok) throw new Error(`getData failed: ${res.status}`);
  return res.json() as Promise<DataPage>;
}

// GET /data now returns every batch the viewer can decrypt (their own plus every accepted
// watched partner's) in one bundled response, with no server-side per-owner filtering.
// BatchData doesn't carry an owner field, so attributing each batch to the right per-target
// SQLite bucket below requires looking it up via its device's owner — GET /device already
// returns exactly the same self-plus-watched-partners device set /data's batches can come
// from, so it's a complete map for this purpose.
let deviceOwnersFetch: Promise<Map<string, string>> | null = null;

async function fetchDeviceOwners(): Promise<Map<string, string>> {
  if (deviceOwnersFetch) return deviceOwnersFetch;
  deviceOwnersFetch = (async () => {
    const res = await fetch(`${BASE}/device`, { credentials: 'include' });
    if (!res.ok) throw new Error(`getDevices failed: ${res.status}`);
    const devices = (await res.json()) as Array<{ id: string; owner: string }>;
    return new Map(devices.map((device) => [device.id, device.owner]));
  })();
  try {
    return await deviceOwnersFetch;
  } finally {
    deviceOwnersFetch = null;
  }
}

// ---------------------------------------------------------------------------
// Core fetch + decrypt loop

async function fetchAndDecrypt(targetUserId: string): Promise<void> {
  if (!session) return;
  const { userId: viewerId, privateKey } = session;
  const cutoffTs = Date.now() - THIRTY_DAYS_MS;
  console.log('[cache-worker] fetchAndDecrypt start for', targetUserId);

  sqlPruneOldData(viewerId, cutoffTs);

  // Return all currently active query IDs for this target user.
  // Called at "done" time so queries that arrived while the fetch was in-flight are also served.
  const activeQids = () =>
    [...activeQueries.entries()]
      .filter(([, aq]) => aq.targetUserId === targetUserId)
      .map(([id]) => id);

  const serveAll = (processed = 0, total = 0) => {
    const qids = activeQids();
    console.log('[cache-worker] serveAll done=true for', qids.length, 'queries');
    for (const qid of qids) {
      const aq = activeQueries.get(qid);
      if (!aq) continue;
      postChunk(qid, sqlQueryEvents(viewerId, targetUserId, aq.query), true, processed, total);
      activeQueries.delete(qid);
    }
  };

  try {
    const since = sqlGetSince(viewerId, targetUserId);
    console.log('[cache-worker] fetching /data since=', since, 'for', targetUserId);
    const [page, deviceOwners] = await Promise.all([fetchData({ since }), fetchDeviceOwners()]);
    // /data no longer filters by owner server-side — scope this target's slice
    // of the bundled response down to batches from devices it actually owns.
    const scopedBatches = page.batches.filter(
      (batch) => deviceOwners.get(batch.device_id) === targetUserId,
    );
    console.log(
      '[cache-worker] /data returned',
      page.batches.length,
      'batches,',
      scopedBatches.length,
      'in scope for',
      targetUserId,
    );
    sqlMergeDataPage(viewerId, targetUserId, { ...page, batches: scopedBatches });
  } catch (err) {
    console.warn('[cache-worker] fetch failed for', targetUserId, err);
    serveAll();
    return;
  }

  const unmaterialized = sqlGetUnmaterializedBatches(viewerId, targetUserId, cutoffTs);
  console.log(
    '[cache-worker] unmaterialized batches:',
    unmaterialized.length,
    'hasPrivateKey=',
    privateKey != null,
  );

  if (unmaterialized.length === 0 || !privateKey) {
    serveAll();
    return;
  }

  const queue = [...unmaterialized].sort((a, b) => b.created_at - a.created_at);
  let processed = 0;
  const total = queue.length;
  // Shared across the concurrent workers below; the runtime is single-threaded so the
  // read-modify of this timestamp between awaits is race-free.
  let lastProgressPostAt = 0;

  // Per-batch key unwrap. Prefer the native-WebCrypto path (off-thread X25519), falling back
  // to the pure-JS @hpke path on browsers whose crypto.subtle lacks X25519. The native
  // unwrapper does a small one-time setup, so build it once for the whole run.
  const nativeUnwrap = await createNativeBatchKeyUnwrapper(privateKey);
  console.log(
    '[cache-worker] batch-key unwrap path:',
    nativeUnwrap ? 'native WebCrypto' : 'JS @hpke',
  );
  const openBatchKey = (encKey: string): Promise<CryptoKey> => {
    const bytes = Uint8Array.fromBase64(encKey);
    return nativeUnwrap ? nativeUnwrap(bytes) : unwrapBatchKey(privateKey, bytes);
  };

  // Events materialized since the last delta flush (image bytes stripped to match
  // sqlQueryEvents output, which loads images lazily). Shared across the workers below;
  // single-threaded, so the push/splice between awaits is race-free.
  const deltaBuffer: FeedLog[] = [];
  let lastDeltaPostAt = Date.now();

  const flushDelta = () => {
    if (deltaBuffer.length === 0) return;
    const fresh = deltaBuffer.splice(0);
    for (const qid of activeQids()) {
      const aq = activeQueries.get(qid);
      if (!aq) continue;
      const matching = fresh.filter((log) => matchesQuery(log, aq.query));
      if (matching.length > 0) postChunk(qid, matching, false, processed, total, 'append');
    }
  };

  const worker = async () => {
    while (queue.length > 0) {
      const batch = queue.shift()!;
      try {
        const events = await decryptAndFlattenBatch(batch, openBatchKey, '0'.repeat(64));
        sqlWriteMaterializedEvents(viewerId, targetUserId, batch.id, events);
        for (const ev of events) {
          const data = { ...ev.data };
          delete data.image;
          deltaBuffer.push({ ...ev, data });
        }
      } catch (err) {
        if (err instanceof DecryptionError) {
          console.warn('[cache-worker] permanently failed to decrypt batch', batch.id, err);
          sqlMarkBatchFailed(batch.id, (err as Error).message);
        } else {
          console.warn('[cache-worker] transient failure materializing batch', batch.id, err);
        }
      }
      processed++;
      const isLast = processed === total;

      if (isLast) {
        // Final pass: serve every query waiting for this target (including late arrivals).
        // This authoritative full result supersedes any unflushed deltaBuffer entries.
        serveAll(processed, total);
      } else {
        const now = Date.now();
        // Frequent counts-only ticks keep the status-line counter smooth without re-querying
        // or re-serializing the growing result set.
        if (now - lastProgressPostAt >= PROGRESS_THROTTLE_MS) {
          lastProgressPostAt = now;
          for (const qid of activeQids()) {
            if (!activeQueries.has(qid)) continue;
            postProgress(qid, processed, total);
          }
        }
        // Coarser delta: ship the logs decrypted since the last flush so they appear
        // progressively. Bounded by the decrypt rate, not the accumulated total.
        if (now - lastDeltaPostAt >= DELTA_INTERVAL_MS) {
          lastDeltaPostAt = now;
          flushDelta();
        }
      }
    }
  };

  await Promise.all(Array.from({ length: Math.min(DECRYPT_CONCURRENCY, total) }, worker));
}

function fetchAndDecryptOnce(targetUserId: string): void {
  if (fetchInFlight.has(targetUserId)) {
    console.log('[cache-worker] fetch already in-flight for', targetUserId);
    return;
  }
  console.log('[cache-worker] starting fetch for', targetUserId);
  const p = fetchAndDecrypt(targetUserId)
    .catch((err) => console.error('[cache-worker] fetchAndDecrypt threw', err))
    .finally(() => fetchInFlight.delete(targetUserId));
  fetchInFlight.set(targetUserId, p);
}

// ---------------------------------------------------------------------------
// Streaming message handlers

async function handleStreaming(
  req: Extract<CacheRequest, { method: 'setSession' | 'cacheQuery' | 'refetch' }>,
): Promise<void> {
  if (req.method === 'setSession') {
    console.log(
      '[cache-worker] setSession userId=',
      req.userId,
      'hasPrivateKey=',
      req.privateKey != null,
    );
    session = { userId: req.userId, privateKey: req.privateKey };
    const targets = new Set([req.userId, ...knownTargetUserIds]);
    console.log('[cache-worker] setSession kicking off fetch for targets:', [...targets]);
    for (const targetId of targets) {
      fetchAndDecryptOnce(targetId);
    }
    return;
  }

  if (req.method === 'cacheQuery') {
    const { query, targetUserId } = req;
    console.log(
      '[cache-worker] cacheQuery id=',
      req.id,
      'targetUserId=',
      targetUserId,
      'hasSession=',
      session != null,
    );
    knownTargetUserIds.add(targetUserId);
    activeQueries.set(req.id, { query, targetUserId });

    if (!session) return; // deferred: will fire when setSession arrives

    // Immediate fast-path from SQLite
    const logs = sqlQueryEvents(session.userId, targetUserId, query);
    console.log('[cache-worker] fast-path returned', logs.length, 'logs');
    postChunk(req.id, logs, false);

    fetchAndDecryptOnce(targetUserId);
    return;
  }

  if (req.method === 'refetch') {
    if (!session) return;
    console.log('[cache-worker] refetch');
    const targets = new Set([session.userId, ...knownTargetUserIds]);
    for (const targetId of targets) {
      fetchAndDecryptOnce(targetId);
    }
  }
}

// ---------------------------------------------------------------------------
// One-shot handlers

async function dispatchOneShot(req: CacheRequest): Promise<unknown> {
  if (req.method === 'deleteDeviceData') {
    sqlDeleteDeviceData(req.viewerId, req.deviceId);
    return 0;
  }
  if (req.method === 'getEventImage') {
    return sqlGetEventImage(req.eventId);
  }
  if (req.method === 'getDeviceBatchEndTimes') {
    return sqlGetDeviceBatchEndTimes(req.viewerId, req.targetUserId, req.deviceId);
  }
  if (req.method === 'getDecryptionStats') {
    return sqlGetDecryptionStats(
      req.viewerId,
      req.targetUserId,
      req.deviceId,
      req.startTime,
      req.endTime,
    );
  }
  return 0;
}

// ---------------------------------------------------------------------------
// Message entry point

self.onmessage = async (e: MessageEvent<CacheRequest>) => {
  const req = e.data;
  console.log('[cache-worker] received', req.method);

  // Handled before the initPromise gate: clearing the cache has to work even when init
  // failed to open the database.
  if (req.method === 'clearCache') {
    let response: CacheResponse;
    try {
      await hardReset();
      activeQueries.clear();
      fetchInFlight.clear();
      knownTargetUserIds.clear();
      initPromise = Promise.resolve();
      response = { id: req.id, result: 0 };
    } catch (err) {
      console.error('[cache-worker] clearCache failed', err);
      initPromise = openDatabase();
      initPromise.catch((e2) => console.error('[cache-worker] reopen after reset FAILED', e2));
      response = { id: req.id, error: (err as Error).message };
    }
    self.postMessage(response);
    return;
  }

  try {
    await initPromise;
  } catch (err) {
    console.error('[cache-worker] init failed, failing message', req.method, err);
    const message = `cache unavailable: ${(err as Error).message}`;
    if (req.method === 'cacheQuery') {
      // Settle the stream so the Logs page shows an empty result instead of spinning.
      postChunk(req.id, [], true);
    } else if (req.method !== 'setSession' && req.method !== 'refetch') {
      self.postMessage({ id: req.id, error: message } satisfies CacheResponse);
    }
    return;
  }

  if (req.method === 'setSession' || req.method === 'cacheQuery' || req.method === 'refetch') {
    await handleStreaming(req).catch((err) =>
      console.warn('[cache-worker] streaming handler error', err),
    );
    return;
  }

  let response: CacheResponse;
  try {
    response = { id: req.id, result: await dispatchOneShot(req) };
  } catch (err) {
    response = { id: req.id, error: (err as Error).message };
  }
  self.postMessage(response);
};
