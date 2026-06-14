/// <reference lib="webworker" />
import sqlite3InitModule from '@sqlite.org/sqlite-wasm';
import { decryptAndFlattenBatch } from '../api/batch-materializer';
import { unwrapBatchKey } from '../api/crypto';
import type { Batch, DataPage } from '../api/api';
import type { FeedLog } from '../../pages/Logs/types';

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
      token: string;
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
    };

type CacheResponse = { id: string; result: unknown } | { id: string; error: string };
type CacheChunk = { type: 'queryChunk'; id: string; logs: FeedLog[]; done: boolean };

// ---------------------------------------------------------------------------
// SQLite / OPFS

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let db: any = null;

const initPromise = (async () => {
  console.log('[cache-worker] init: loading sqlite3 wasm');
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const sqlite3 = (await sqlite3InitModule()) as any;
  console.log('[cache-worker] init: installing OPFS SAH pool VFS');
  await sqlite3.installOpfsSAHPoolVfs({});
  console.log('[cache-worker] init: opening /cache.db');
  db = new sqlite3.oo1.OpfsDb('/cache.db');
  db.exec(
    `CREATE TABLE IF NOT EXISTS feeds (
       viewer_id TEXT NOT NULL,
       target_user_id TEXT NOT NULL,
       since INTEGER,
       PRIMARY KEY (viewer_id, target_user_id)
     )`,
  );
  db.exec(
    `CREATE TABLE IF NOT EXISTS batches (
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
       end_hash TEXT
     )`,
  );
  db.exec(
    `CREATE TABLE IF NOT EXISTS materialized_batch_ids (
       batch_id TEXT PRIMARY KEY
     )`,
  );
  db.exec(
    `CREATE TABLE IF NOT EXISTS events (
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
  );
  db.exec(
    `CREATE TABLE IF NOT EXISTS event_images (
       event_id TEXT PRIMARY KEY,
       data BLOB NOT NULL
     )`,
  );
  db.exec(
    `CREATE TABLE IF NOT EXISTS direct_logs (
       id TEXT PRIMARY KEY,
       viewer_id TEXT NOT NULL,
       target_user_id TEXT NOT NULL,
       device_id TEXT NOT NULL,
       ts INTEGER NOT NULL,
       type TEXT NOT NULL,
       data TEXT NOT NULL,
       created_at INTEGER NOT NULL,
       risk REAL
     )`,
  );
  console.log('[cache-worker] init: done');
})().catch((err) => {
  console.error('[cache-worker] init FAILED', err);
});

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
        `INSERT OR IGNORE INTO batches(id,viewer_id,target_user_id,device_id,url,encrypted_key,start_time,end_time,created_at,end_hash)
         VALUES(?,?,?,?,?,?,?,?,?,?)`,
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
          ],
        },
      );
    }
    for (const log of page.logs) {
      db.exec(
        `INSERT OR IGNORE INTO direct_logs(id,viewer_id,target_user_id,device_id,ts,type,data,created_at,risk)
         VALUES(?,?,?,?,?,?,?,?,?)`,
        {
          bind: [
            log.id,
            viewerId,
            targetUserId,
            log.device_id,
            log.ts,
            log.type,
            JSON.stringify(log.data),
            log.created_at,
            log.risk ?? null,
          ],
        },
      );
    }

    const allCreatedAts = [
      ...page.batches.map((b) => b.created_at),
      ...page.logs.map((l) => l.created_at),
    ];
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
     AND b.id NOT IN (SELECT batch_id FROM materialized_batch_ids)`,
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

  let logSql = `SELECT * FROM direct_logs WHERE viewer_id=? AND target_user_id=?`;
  const logBind: unknown[] = [viewerId, targetUserId];

  if (query.deviceId) {
    logSql += ` AND device_id=?`;
    logBind.push(query.deviceId);
  }
  if (query.startTime !== undefined && query.endTime !== undefined) {
    logSql += ` AND ts BETWEEN ? AND ?`;
    logBind.push(query.startTime, query.endTime);
  }

  db.exec(logSql, {
    bind: logBind,
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
        batch_status: 'unknown' as const,
        source: 'log' as const,
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
  db.exec(`DELETE FROM direct_logs WHERE viewer_id=? AND ts<?`, { bind: [viewerId, cutoffTs] });
}

function sqlClearAll(): void {
  db.exec(`DELETE FROM events`);
  db.exec(`DELETE FROM direct_logs`);
  db.exec(`DELETE FROM batches`);
  db.exec(`DELETE FROM feeds`);
  db.exec(`DELETE FROM materialized_batch_ids`);
  db.exec(`DELETE FROM event_images`);
}

function sqlDeleteDeviceData(viewerId: string, deviceId: string): void {
  db.exec(`DELETE FROM events WHERE viewer_id=? AND device_id=?`, {
    bind: [viewerId, deviceId],
  });
  db.exec(`DELETE FROM batches WHERE viewer_id=? AND device_id=?`, {
    bind: [viewerId, deviceId],
  });
  db.exec(`DELETE FROM direct_logs WHERE viewer_id=? AND device_id=?`, {
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

// ---------------------------------------------------------------------------
// Decryption pipeline state

const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000;
const DECRYPT_CONCURRENCY = 5;
const BASE =
  (import.meta as { env?: { VITE_API_URL?: string } }).env?.VITE_API_URL ?? 'http://localhost:8787';

let session: { token: string; userId: string; privateKey: CryptoKey | null } | null = null;

// queryId → query metadata; kept until done:true is posted
const activeQueries = new Map<string, { query: WorkerCacheQuery; targetUserId: string }>();

// targetUserId → in-flight fetch promise (deduplication)
const fetchInFlight = new Map<string, Promise<void>>();

// all target user IDs ever seen (so refetch can cover them all)
const knownTargetUserIds = new Set<string>();

// ---------------------------------------------------------------------------
// Helpers

function postChunk(id: string, logs: FeedLog[], done: boolean): void {
  (self as unknown as DedicatedWorkerGlobalScope).postMessage({
    type: 'queryChunk',
    id,
    logs,
    done,
  } satisfies CacheChunk);
}

async function fetchData(
  token: string,
  params?: { user?: string; since?: number },
): Promise<DataPage> {
  const qs = new URLSearchParams();
  if (params?.user) qs.set('user', params.user);
  if (params?.since !== undefined) qs.set('since', String(params.since));
  const q = qs.toString();
  const res = await fetch(`${BASE}/data${q ? `?${q}` : ''}`, {
    credentials: 'include',
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!res.ok) throw new Error(`getData failed: ${res.status}`);
  return res.json() as Promise<DataPage>;
}

// ---------------------------------------------------------------------------
// Core fetch + decrypt loop

async function fetchAndDecrypt(targetUserId: string): Promise<void> {
  if (!session) return;
  const { token, userId: viewerId, privateKey } = session;
  const cutoffTs = Date.now() - THIRTY_DAYS_MS;
  console.log('[cache-worker] fetchAndDecrypt start for', targetUserId);

  sqlPruneOldData(viewerId, cutoffTs);

  // Return all currently active query IDs for this target user.
  // Called at "done" time so queries that arrived while the fetch was in-flight are also served.
  const activeQids = () =>
    [...activeQueries.entries()]
      .filter(([, aq]) => aq.targetUserId === targetUserId)
      .map(([id]) => id);

  const serveAll = () => {
    const qids = activeQids();
    console.log('[cache-worker] serveAll done=true for', qids.length, 'queries');
    for (const qid of qids) {
      const aq = activeQueries.get(qid);
      if (!aq) continue;
      postChunk(qid, sqlQueryEvents(viewerId, targetUserId, aq.query), true);
      activeQueries.delete(qid);
    }
  };

  try {
    const since = sqlGetSince(viewerId, targetUserId);
    console.log('[cache-worker] fetching /data since=', since, 'for', targetUserId);
    const page = await fetchData(token, {
      user: targetUserId === viewerId ? undefined : targetUserId,
      since,
    });
    console.log(
      '[cache-worker] /data returned',
      page.batches.length,
      'batches,',
      page.logs.length,
      'logs',
    );
    sqlMergeDataPage(viewerId, targetUserId, page);
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

  const worker = async () => {
    while (queue.length > 0) {
      const batch = queue.shift()!;
      try {
        const events = await decryptAndFlattenBatch(
          batch,
          (encKey) => unwrapBatchKey(privateKey, Uint8Array.fromBase64(encKey)),
          '0'.repeat(64),
        );
        sqlWriteMaterializedEvents(viewerId, targetUserId, batch.id, events);
      } catch (err) {
        console.warn('[cache-worker] failed to materialize batch', batch.id, err);
      }
      processed++;
      const isLast = processed === total;

      if (isLast) {
        // Final pass: serve every query waiting for this target (including late arrivals)
        serveAll();
      } else {
        // Intermediate progress: push current state to all active queries
        for (const qid of activeQids()) {
          const aq = activeQueries.get(qid);
          if (!aq) continue;
          postChunk(qid, sqlQueryEvents(viewerId, targetUserId, aq.query), false);
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
    session = { token: req.token, userId: req.userId, privateKey: req.privateKey };
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
  if (req.method === 'clearCache') {
    sqlClearAll();
    return 0;
  }
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
  return 0;
}

// ---------------------------------------------------------------------------
// Message entry point

self.onmessage = async (e: MessageEvent<CacheRequest>) => {
  const req = e.data;
  console.log('[cache-worker] received', req.method);
  try {
    await initPromise;
  } catch (err) {
    console.error('[cache-worker] init failed, dropping message', req.method, err);
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
