import { uuidToBytes } from './db';
import { decodeBase64 } from './encoding';
import { generateToken } from './jwt';
import { Env } from '../types/bindings';

const ZERO_STATE = new Uint8Array(32);

async function readHashStateRow(db: D1Database, deviceId: string) {
  // No uuid-field decoding needed here (unlike db.ts's old getHashState) since we
  // already have deviceId as a string and don't need device_id back in the row.
  return db
    .prepare('SELECT state, updated_at, count, hashed_at FROM hash_states WHERE device_id = ?')
    .bind(uuidToBytes(deviceId))
    .first<{ state: ArrayBuffer; updated_at: number; count: number; hashed_at: number | null }>();
}

export async function localHashGet(db: D1Database, deviceId: string): Promise<Uint8Array> {
  const row = await readHashStateRow(db, deviceId);
  return row ? new Uint8Array(row.state) : ZERO_STATE;
}

export async function localHashReset(db: D1Database, deviceId: string): Promise<void> {
  await db
    .prepare(
      `INSERT INTO hash_states (device_id, state, updated_at, count)
       VALUES (?, ?, ?, 0)
       ON CONFLICT(device_id) DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at, count = 0`,
    )
    .bind(uuidToBytes(deviceId), ZERO_STATE.buffer, Date.now())
    .run();
}

export async function localHashIngest(
  db: D1Database,
  deviceId: string,
  content: Uint8Array,
): Promise<void> {
  const now = Date.now();
  const current = await localHashGet(db, deviceId);
  const hashInput = new Uint8Array(64);
  hashInput.set(current, 0);
  hashInput.set(content, 32);
  const nextState = await crypto.subtle.digest('SHA-256', hashInput);
  await db
    .prepare(
      `INSERT INTO hash_states (device_id, state, updated_at, count, hashed_at)
       VALUES (?, ?, ?, 1, ?)
       ON CONFLICT(device_id) DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at, count = count + 1, hashed_at = excluded.hashed_at`,
    )
    .bind(uuidToBytes(deviceId), nextState, now, now)
    .run();
}

// Used by hashes.ts's GET /hash/info and by devices.ts's batched local-branch lookup —
// the one hash-state read shape that isn't get/reset/ingest, kept here so it's still
// the single owner of hash_states access instead of a duplicate query living in db.ts.
// Returns null (not a zero-filled object) when no row exists — devices.ts's caller
// relies on this to distinguish "no hash data yet" (falls back to its own denormalized
// device.last_hash_at/pending_count columns) from "a real row with zero values".
export async function localHashInfo(db: D1Database, deviceId: string) {
  const row = await readHashStateRow(db, deviceId);
  return row ? { count: row.count, hashed_at: row.hashed_at, updated_at: row.updated_at } : null;
}

export function isLocalHashServer(env: Env): boolean {
  const url = env.HASH_SERVER_URL?.trim();
  return !url || url.endsWith('/api');
}

// GET /hash on the real hash-server is a merged endpoint (see hash-server's
// routes.rs) returning {state, count, hashed_at, updated_at} as JSON, not raw
// bytes. This caller only ever wanted the state, so that's all it reads.
export async function hashGet(env: Env, deviceId: string): Promise<Uint8Array> {
  if (isLocalHashServer(env)) return localHashGet(env.DB, deviceId);
  const token = await generateToken('hash-server', deviceId, env.JWT_PRIVATE_KEY, 60);
  const resp = await fetch(`${env.HASH_SERVER_URL!.trim()}/hash`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!resp.ok) throw new Error(`remote hash server GET /hash failed: ${resp.status}`);
  const body = (await resp.json()) as { state: string };
  return new Uint8Array(decodeBase64(body.state));
}

export async function hashReset(env: Env, deviceId: string): Promise<void> {
  if (isLocalHashServer(env)) return localHashReset(env.DB, deviceId);
  const token = await generateToken('server', deviceId, env.JWT_PRIVATE_KEY, 60);
  const resp = await fetch(`${env.HASH_SERVER_URL!.trim()}/hash`, {
    method: 'DELETE',
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!resp.ok) throw new Error(`remote hash server DELETE /hash failed: ${resp.status}`);
}

export async function hashIngest(env: Env, deviceId: string, content: Uint8Array): Promise<void> {
  if (isLocalHashServer(env)) return localHashIngest(env.DB, deviceId, content);
  const token = await generateToken('hash-server', deviceId, env.JWT_PRIVATE_KEY, 60);
  const resp = await fetch(`${env.HASH_SERVER_URL!.trim()}/hash`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/octet-stream' },
    body: content,
  });
  if (!resp.ok) throw new Error(`remote hash server POST /hash failed: ${resp.status}`);
}
