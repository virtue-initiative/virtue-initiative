import { beforeEach, describe, expect, it } from 'vitest';
import { createExecutionContext, env, waitOnExecutionContext } from 'cloudflare:test';
import worker from '../src/index';
import { computeAnalyticsMetrics, snapshotAnalytics } from '../src/lib/analytics';
import { perPersonAverage } from '../../shared-web/types';
import { clearDB, createDeviceForUser, signupAndGetCookie, uuidToBytes } from './helpers';

beforeEach(clearDB);

const DAY_MS = 24 * 60 * 60 * 1000;
const EMPTY_ACCESS_KEYS = JSON.stringify({ keys: {} });

async function insertBatch(userId: string, deviceId: string, createdAt: number) {
  const id = crypto.randomUUID();
  await env.DB.prepare(
    `INSERT INTO batches (id, user_id, device_id, url, start_time, end_time, end_hash, access_keys, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
  )
    .bind(
      uuidToBytes(id),
      uuidToBytes(userId),
      uuidToBytes(deviceId),
      `batches/${id}.enc`,
      createdAt,
      createdAt,
      `hash-${id}`,
      EMPTY_ACCESS_KEYS,
      createdAt,
    )
    .run();
}

async function insertPartner(
  watchingUserId: string,
  watcherEmail: string,
  status: 'accepted' | 'pending',
  now: number,
) {
  await env.DB.prepare(
    `INSERT INTO partners (id, watching_user_id, watcher_email, status, created_at, updated_at)
     VALUES (?, ?, ?, ?, ?, ?)`,
  )
    .bind(
      uuidToBytes(crypto.randomUUID()),
      uuidToBytes(watchingUserId),
      watcherEmail,
      status,
      now,
      now,
    )
    .run();
}

async function setUserCreatedAt(userId: string, createdAt: number) {
  await env.DB.prepare('UPDATE users SET created_at = ? WHERE id = ?')
    .bind(createdAt, uuidToBytes(userId))
    .run();
}

describe('computeAnalyticsMetrics', () => {
  it('returns zeros on an empty database', async () => {
    const metrics = await computeAnalyticsMetrics(env.DB, Date.now());

    expect(metrics.users).toEqual({ total: 0, verified: 0, new_1d: 0, new_7d: 0, new_30d: 0 });
    expect(metrics.active_users).toEqual({ d1: 0, d7: 0, d30: 0 });
    expect(metrics.active_devices).toEqual({ d1: 0, d7: 0, d30: 0 });
    expect(metrics.devices).toEqual({ total: 0, owners: 0, by_platform: {} });
    expect(metrics.batches).toEqual({ total: 0, d1: 0, d7: 0 });
    expect(metrics.partners).toEqual({ total: 0, accepted: 0, pending: 0, watched_users: 0 });
    expect(metrics.locked_passwords).toEqual({ total: 0 });
  });

  it('counts users, devices, batches, and partners against the windows', async () => {
    const now = Date.now();
    const { userId: aliceId } = await signupAndGetCookie('alice@example.com');
    const { userId: bobId } = await signupAndGetCookie('bob@example.com');
    const { userId: carolId } = await signupAndGetCookie('carol@example.com');
    await setUserCreatedAt(aliceId, now - 2 * DAY_MS);
    await setUserCreatedAt(bobId, now - 10 * DAY_MS);
    await setUserCreatedAt(carolId, now - 40 * DAY_MS);

    const aliceLaptop = await createDeviceForUser('alice@example.com', 'password123', 'L', 'linux');
    const alicePhone = await createDeviceForUser(
      'alice@example.com',
      'password123',
      'P',
      'android',
    );
    const bobLaptop = await createDeviceForUser('bob@example.com', 'password123', 'L', 'linux');
    const bobOld = await createDeviceForUser('bob@example.com', 'password123', 'Old', 'mac');
    await env.DB.prepare('UPDATE devices SET deleted_at = ? WHERE id = ?')
      .bind(now, uuidToBytes(bobOld.id))
      .run();
    // Signup verifies the address (device login needs it), so knock two back after.
    await env.DB.prepare('UPDATE users SET email_verified = 0 WHERE id IN (?, ?)')
      .bind(uuidToBytes(bobId), uuidToBytes(carolId))
      .run();

    await insertBatch(aliceId, aliceLaptop.id, now - 60_000); // 1d, 7d, 30d
    await insertBatch(aliceId, alicePhone.id, now - 3 * DAY_MS); // 7d, 30d
    await insertBatch(bobId, bobLaptop.id, now - 20 * DAY_MS); // 30d only
    await insertBatch(bobId, bobLaptop.id, now - 45 * DAY_MS); // outside every window

    await insertPartner(aliceId, 'w1@example.com', 'accepted', now);
    await insertPartner(aliceId, 'w2@example.com', 'accepted', now);
    await insertPartner(bobId, 'w3@example.com', 'accepted', now);
    await insertPartner(carolId, 'w4@example.com', 'pending', now);

    await env.DB.prepare(
      `INSERT INTO locked_passwords (id, owner_id, label, wrapped_value, deleted_at, created_at)
       VALUES (?, ?, 'a', 'x', NULL, ?), (?, ?, 'b', 'y', ?, ?)`,
    )
      .bind(
        uuidToBytes(crypto.randomUUID()),
        uuidToBytes(aliceId),
        now,
        uuidToBytes(crypto.randomUUID()),
        uuidToBytes(aliceId),
        now,
        now,
      )
      .run();

    const metrics = await computeAnalyticsMetrics(env.DB, now);

    expect(metrics.users).toEqual({ total: 3, verified: 1, new_1d: 0, new_7d: 1, new_30d: 2 });
    expect(metrics.active_users).toEqual({ d1: 1, d7: 1, d30: 2 });
    // alice: laptop (1d) + phone (3d); bob: one laptop with uploads at 20d and 45d.
    expect(metrics.active_devices).toEqual({ d1: 1, d7: 2, d30: 3 });
    expect(metrics.devices).toEqual({
      total: 3,
      owners: 2, // carol has no device
      by_platform: { linux: 2, android: 1 },
    });
    expect(metrics.batches).toEqual({ total: 4, d1: 1, d7: 2 });
    expect(metrics.partners).toEqual({ total: 4, accepted: 3, pending: 1, watched_users: 2 });
    expect(metrics.locked_passwords).toEqual({ total: 1 });
  });

  it('counts owners and watched users, not users without any', async () => {
    const { userId } = await signupAndGetCookie('one@example.com');
    await signupAndGetCookie('two@example.com');
    await signupAndGetCookie('three@example.com');
    await createDeviceForUser('one@example.com');
    await createDeviceForUser('one@example.com', 'password123', 'Phone', 'android');
    await createDeviceForUser('one@example.com', 'password123', 'Tablet', 'ios');
    await createDeviceForUser('two@example.com');

    const now = Date.now();
    await insertPartner(userId, 'a@example.com', 'accepted', now);
    await insertPartner(userId, 'b@example.com', 'accepted', now);
    await insertPartner(userId, 'c@example.com', 'pending', now);

    const metrics = await computeAnalyticsMetrics(env.DB);
    expect(metrics.devices.total).toBe(4);
    expect(metrics.devices.owners).toBe(2);
    expect(metrics.partners.accepted).toBe(2);
    expect(metrics.partners.watched_users).toBe(1);
    // Ratios are derived by the client, never stored.
    expect(metrics.devices).not.toHaveProperty('per_user_avg');
    expect(metrics.partners).not.toHaveProperty('per_user_avg');
    expect(perPersonAverage(metrics.devices.total, metrics.devices.owners)).toBe(2);
    expect(perPersonAverage(1, 3)).toBe(0.33);
    expect(perPersonAverage(0, 0)).toBe(0);
  });
});

describe('snapshotAnalytics', () => {
  it('stores one row per UTC day and replaces it on rerun', async () => {
    const day = Date.UTC(2026, 8, 18, 3, 0, 0); // 2026-09-18T03:00Z
    const first = await snapshotAnalytics(env, day);
    expect(first.day).toBe('2026-09-18');
    expect(first.metrics.users.total).toBe(0);

    await signupAndGetCookie('later@example.com');
    const second = await snapshotAnalytics(env, day + 60 * 60 * 1000);
    expect(second.day).toBe('2026-09-18');
    expect(second.metrics.users.total).toBe(1);

    const rows = await env.DB.prepare('SELECT day, metrics, created_at FROM analytics_snapshots')
      .all<{ day: string; metrics: string; created_at: number }>()
      .then((r) => r.results);
    expect(rows).toHaveLength(1);
    expect(rows[0].day).toBe('2026-09-18');
    expect(rows[0].created_at).toBe(day + 60 * 60 * 1000);
    expect(JSON.parse(rows[0].metrics).users.total).toBe(1);
  });
});

describe('scheduled handler', () => {
  async function snapshotCount() {
    const row = await env.DB.prepare('SELECT COUNT(*) AS n FROM analytics_snapshots').first<{
      n: number;
    }>();
    return row?.n ?? 0;
  }

  async function fire(cron: string) {
    const ctx = createExecutionContext();
    const controller = {
      cron,
      scheduledTime: Date.now(),
      noRetry() {},
    } as ScheduledController;
    worker.scheduled(controller, env, ctx);
    await waitOnExecutionContext(ctx);
  }

  it('snapshots on the daily cron but not on the hourly one', async () => {
    await fire('5 * * * *');
    expect(await snapshotCount()).toBe(0);

    await fire('20 0 * * *');
    expect(await snapshotCount()).toBe(1);
  });
});
