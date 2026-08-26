import { beforeEach, describe, expect, it } from 'vitest';
import { env } from 'cloudflare:test';
import { pruneExpiredBatches } from '../src/lib/retention';
import { clearDB, createDeviceForUser, signupAndGetCookie, uuidToBytes } from './helpers';

beforeEach(clearDB);

const DAY_MS = 24 * 60 * 60 * 1000;
const RETENTION_MS = 30 * DAY_MS;
const EMPTY_ACCESS_KEYS = JSON.stringify({ keys: {} });

const INSERT_BATCH_SQL = `INSERT INTO batches (id, user_id, device_id, url, start_time, end_time, end_hash, access_keys, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`;

async function insertBatch(
  id: string,
  userId: string,
  deviceId: string,
  url: string,
  createdAt: number,
) {
  await env.DB.prepare(INSERT_BATCH_SQL)
    .bind(
      uuidToBytes(id),
      uuidToBytes(userId),
      uuidToBytes(deviceId),
      url,
      createdAt,
      createdAt,
      `hash-${id}`,
      EMPTY_ACCESS_KEYS,
      createdAt,
    )
    .run();
}

// One D1 round-trip per row (via insertBatch) is slow enough at a few hundred
// rows to strain the local test runtime's socket under CI load; batching
// keeps this test's own setup fast and out of the way of what it's testing.
async function insertBatches(
  rows: { id: string; userId: string; deviceId: string; url: string; createdAt: number }[],
) {
  const statements = rows.map((row) =>
    env.DB.prepare(INSERT_BATCH_SQL).bind(
      uuidToBytes(row.id),
      uuidToBytes(row.userId),
      uuidToBytes(row.deviceId),
      row.url,
      row.createdAt,
      row.createdAt,
      `hash-${row.id}`,
      EMPTY_ACCESS_KEYS,
      row.createdAt,
    ),
  );
  await env.DB.batch(statements);
}

async function batchCount() {
  const result = await env.DB.prepare('SELECT COUNT(*) AS count FROM batches').first<{
    count: number;
  }>();
  return result?.count ?? 0;
}

describe('pruneExpiredBatches', () => {
  it('deletes batches older than 30 days and keeps recent ones', async () => {
    const now = Date.UTC(2026, 0, 6, 0, 0, 0);
    const { userId } = await signupAndGetCookie('retention-owner@example.com', 'pw');
    await signupAndGetCookie('retention-owner-login@example.com', 'pw');
    const device = await createDeviceForUser(
      'retention-owner-login@example.com',
      'pw',
      'Retention Device',
      'linux',
    );

    const oldId = '00000000-0000-4000-8000-000000000101';
    const recentId = '00000000-0000-4000-8000-000000000102';

    await insertBatch(
      oldId,
      userId,
      device.id,
      'https://example.com/old.enc',
      now - RETENTION_MS - DAY_MS,
    );
    await insertBatch(recentId, userId, device.id, 'https://example.com/recent.enc', now - DAY_MS);

    const deleted = await pruneExpiredBatches(env, now);

    expect(deleted).toBe(1);
    const remaining = await env.DB.prepare('SELECT id FROM batches').all<{ id: ArrayBuffer }>();
    expect(remaining.results).toHaveLength(1);
  });

  it('retains a batch created exactly at the 30-day boundary', async () => {
    const now = Date.UTC(2026, 0, 6, 0, 0, 0);
    const { userId } = await signupAndGetCookie('retention-boundary@example.com', 'pw');
    await signupAndGetCookie('retention-boundary-login@example.com', 'pw');
    const device = await createDeviceForUser(
      'retention-boundary-login@example.com',
      'pw',
      'Boundary Device',
      'linux',
    );

    const boundaryId = '00000000-0000-4000-8000-000000000103';
    await insertBatch(
      boundaryId,
      userId,
      device.id,
      'https://example.com/boundary.enc',
      now - RETENTION_MS,
    );

    const deleted = await pruneExpiredBatches(env, now);

    expect(deleted).toBe(0);
    expect(await batchCount()).toBe(1);
  });

  it('drains a backlog larger than the per-round-trip chunk limit', async () => {
    const now = Date.UTC(2026, 0, 6, 0, 0, 0);
    const { userId } = await signupAndGetCookie('retention-backlog@example.com', 'pw');
    await signupAndGetCookie('retention-backlog-login@example.com', 'pw');
    const device = await createDeviceForUser(
      'retention-backlog-login@example.com',
      'pw',
      'Backlog Device',
      'linux',
    );

    const backlogSize = 620; // exceeds the 500-row PRUNE_CHUNK_LIMIT
    await insertBatches(
      Array.from({ length: backlogSize }, (_, i) => ({
        id: `00000000-0000-4000-9000-${i.toString(16).padStart(12, '0')}`,
        userId,
        deviceId: device.id,
        url: `https://example.com/backlog-${i}.enc`,
        createdAt: now - RETENTION_MS - DAY_MS,
      })),
    );

    const deleted = await pruneExpiredBatches(env, now);

    expect(deleted).toBe(backlogSize);
    expect(await batchCount()).toBe(0);
  });
});
