import { beforeEach, describe, expect, it } from 'vitest';
import { env } from 'cloudflare:test';
import { pruneExpiredBatches } from '../src/lib/retention';
import { Env } from '../src/types/bindings';
import { clearDB, createDeviceForUser, signupAndGetCookie, uuidToBytes } from './helpers';

beforeEach(clearDB);

const DAY_MS = 24 * 60 * 60 * 1000;
const RETENTION_MS = 30 * DAY_MS;
const EMPTY_ACCESS_KEYS = JSON.stringify({ keys: {} });

const INSERT_BATCH_SQL = `INSERT INTO batches (id, user_id, device_id, start_time, end_time, end_hash, access_keys, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?)`;

async function insertBatch(id: string, userId: string, deviceId: string, createdAt: number) {
  await env.DB.prepare(INSERT_BATCH_SQL)
    .bind(
      uuidToBytes(id),
      uuidToBytes(userId),
      uuidToBytes(deviceId),
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
  rows: { id: string; userId: string; deviceId: string; createdAt: number }[],
) {
  const statements = rows.map((row) =>
    env.DB.prepare(INSERT_BATCH_SQL).bind(
      uuidToBytes(row.id),
      uuidToBytes(row.userId),
      uuidToBytes(row.deviceId),
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

describe('pruneExpiredBatches (API-038)', () => {
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

    await insertBatch(oldId, userId, device.id, now - RETENTION_MS - DAY_MS);
    await insertBatch(recentId, userId, device.id, now - DAY_MS);

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
    await insertBatch(boundaryId, userId, device.id, now - RETENTION_MS);

    const deleted = await pruneExpiredBatches(env, now);

    expect(deleted).toBe(0);
    expect(await batchCount()).toBe(1);
  });

  it('deletes a large backlog for one user in one pass', async () => {
    const now = Date.UTC(2026, 0, 6, 0, 0, 0);
    const { userId } = await signupAndGetCookie('retention-backlog@example.com', 'pw');
    await signupAndGetCookie('retention-backlog-login@example.com', 'pw');
    const device = await createDeviceForUser(
      'retention-backlog-login@example.com',
      'pw',
      'Backlog Device',
      'linux',
    );

    const backlogSize = 620;
    await insertBatches(
      Array.from({ length: backlogSize }, (_, i) => ({
        id: `00000000-0000-4000-9000-${i.toString(16).padStart(12, '0')}`,
        userId,
        deviceId: device.id,
        createdAt: now - RETENTION_MS - DAY_MS,
      })),
    );

    const deleted = await pruneExpiredBatches(env, now);

    expect(deleted).toBe(backlogSize);
    expect(await batchCount()).toBe(0);
  });

  it('covers every user when they span several chunks', async () => {
    const now = Date.UTC(2026, 0, 6, 0, 0, 0);
    const users = [];
    for (let i = 0; i < 5; i++) {
      users.push(await signupAndGetCookie(`retention-chunk-${i}@example.com`, 'pw'));
    }
    const device = await createDeviceForUser(
      'retention-chunk-0@example.com',
      'pw',
      'Chunk Device',
      'linux',
    );

    // Users 0, 2 and 4 each have an expired and a recent batch; 1 and 3 have none.
    await insertBatches(
      [0, 2, 4].flatMap((u) => [
        {
          id: `00000000-0000-4000-a000-${(u * 2).toString(16).padStart(12, '0')}`,
          userId: users[u].userId,
          deviceId: device.id,
          createdAt: now - RETENTION_MS - DAY_MS,
        },
        {
          id: `00000000-0000-4000-a000-${(u * 2 + 1).toString(16).padStart(12, '0')}`,
          userId: users[u].userId,
          deviceId: device.id,
          createdAt: now - DAY_MS,
        },
      ]),
    );

    const deleted = await pruneExpiredBatches(env, now, 2);

    expect(deleted).toBe(3);
    expect(await batchCount()).toBe(3);
  });

  it('reads nothing for users without batches', async () => {
    const now = Date.UTC(2026, 0, 6, 0, 0, 0);
    const { userId } = await signupAndGetCookie('retention-active@example.com', 'pw');
    const device = await createDeviceForUser(
      'retention-active@example.com',
      'pw',
      'Active Device',
      'linux',
    );

    // Counts the rows the prune reads, which is what D1 bills.
    let rowsRead = 0;
    const counted = {
      ...env,
      DB: {
        prepare: (sql: string) => env.DB.prepare(sql),
        batch: async (statements: D1PreparedStatement[]) => {
          const results = await env.DB.batch(statements);
          for (const result of results) rowsRead += result.meta.rows_read;
          return results;
        },
      } as unknown as D1Database,
    } as Env;

    async function prune() {
      await insertBatches([
        {
          id: crypto.randomUUID(),
          userId,
          deviceId: device.id,
          createdAt: now - RETENTION_MS - DAY_MS,
        },
        { id: crypto.randomUUID(), userId, deviceId: device.id, createdAt: now - DAY_MS },
      ]);
      rowsRead = 0;
      const deleted = await pruneExpiredBatches(counted, now, 2);
      await env.DB.exec('DELETE FROM batches');
      return { deleted, rowsRead };
    }

    const alone = await prune();
    await env.DB.batch(
      Array.from({ length: 300 }, (_, i) =>
        env.DB.prepare(
          `INSERT INTO users (id, email, password_hash, password_salt, created_at)
           VALUES (?, ?, 'x', x'00', 0)`,
        ).bind(uuidToBytes(crypto.randomUUID()), `retention-inactive-${i}@example.com`),
      ),
    );
    const withInactive = await prune();

    expect(alone.deleted).toBe(1);
    expect(withInactive).toEqual(alone);
  });
});
