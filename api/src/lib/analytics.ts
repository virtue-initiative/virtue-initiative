// api/SPEC.md API-051: headline metrics, snapshotted once per UTC day by the
// cron in src/index.ts and on demand by POST /admin/analytics/refresh.
import type { AnalyticsMetrics, AnalyticsSnapshot } from '../../../shared-web/types';
import { upsertAnalyticsSnapshot } from './db';
import { Env } from '../types/bindings';

const DAY_MS = 24 * 60 * 60 * 1000;

type CountRow = { n: number };
type PlatformRow = { platform: string; n: number };
type BatchStatsRow = {
  total: number;
  d1: number;
  d7: number;
  users_d1: number;
  users_d7: number;
  users_d30: number;
  devices_d1: number;
  devices_d7: number;
  devices_d30: number;
};

function count(result: D1Result<unknown>): number {
  const row = result.results[0] as CountRow | undefined;
  return row?.n ?? 0;
}

export async function computeAnalyticsMetrics(
  db: D1Database,
  now = Date.now(),
): Promise<AnalyticsMetrics> {
  const since1d = now - DAY_MS;
  const since7d = now - 7 * DAY_MS;
  const since30d = now - 30 * DAY_MS;

  const [
    usersTotal,
    usersVerified,
    usersNew1d,
    usersNew7d,
    usersNew30d,
    devicesTotal,
    devicesOwners,
    devicesByPlatform,
    batchStats,
    partnersTotal,
    partnersAccepted,
    partnersPending,
    partnersWatchedUsers,
    lockedPasswordsTotal,
  ] = await db.batch([
    db.prepare('SELECT COUNT(*) AS n FROM users'),
    db.prepare('SELECT COUNT(*) AS n FROM users WHERE email_verified = 1'),
    db.prepare('SELECT COUNT(*) AS n FROM users WHERE created_at > ?').bind(since1d),
    db.prepare('SELECT COUNT(*) AS n FROM users WHERE created_at > ?').bind(since7d),
    db.prepare('SELECT COUNT(*) AS n FROM users WHERE created_at > ?').bind(since30d),
    db.prepare('SELECT COUNT(*) AS n FROM devices WHERE deleted_at IS NULL'),
    db.prepare('SELECT COUNT(DISTINCT owner) AS n FROM devices WHERE deleted_at IS NULL'),
    db.prepare(
      'SELECT platform, COUNT(*) AS n FROM devices WHERE deleted_at IS NULL GROUP BY platform',
    ),
    // Every batch figure in one pass. Batches are only kept for 30 days, so each
    // of these would read most of the table if it ran as its own query.
    db
      .prepare(
        `SELECT COUNT(*) AS total,
                COUNT(CASE WHEN created_at > ?1 THEN 1 END) AS d1,
                COUNT(CASE WHEN created_at > ?2 THEN 1 END) AS d7,
                COUNT(DISTINCT CASE WHEN created_at > ?1 THEN user_id END) AS users_d1,
                COUNT(DISTINCT CASE WHEN created_at > ?2 THEN user_id END) AS users_d7,
                COUNT(DISTINCT CASE WHEN created_at > ?3 THEN user_id END) AS users_d30,
                COUNT(DISTINCT CASE WHEN created_at > ?1 THEN device_id END) AS devices_d1,
                COUNT(DISTINCT CASE WHEN created_at > ?2 THEN device_id END) AS devices_d7,
                COUNT(DISTINCT CASE WHEN created_at > ?3 THEN device_id END) AS devices_d30
         FROM batches`,
      )
      .bind(since1d, since7d, since30d),
    db.prepare('SELECT COUNT(*) AS n FROM partners'),
    db.prepare("SELECT COUNT(*) AS n FROM partners WHERE status = 'accepted'"),
    db.prepare("SELECT COUNT(*) AS n FROM partners WHERE status = 'pending'"),
    db.prepare(
      "SELECT COUNT(DISTINCT watching_user_id) AS n FROM partners WHERE status = 'accepted'",
    ),
    db.prepare('SELECT COUNT(*) AS n FROM locked_passwords WHERE deleted_at IS NULL'),
  ]);

  const users = count(usersTotal);
  const batches = batchStats.results[0] as BatchStatsRow;

  const byPlatform: Record<string, number> = {};
  for (const row of devicesByPlatform.results as PlatformRow[]) {
    byPlatform[row.platform] = row.n;
  }

  return {
    users: {
      total: users,
      verified: count(usersVerified),
      new_1d: count(usersNew1d),
      new_7d: count(usersNew7d),
      new_30d: count(usersNew30d),
    },
    active_users: {
      d1: batches.users_d1,
      d7: batches.users_d7,
      d30: batches.users_d30,
    },
    active_devices: {
      d1: batches.devices_d1,
      d7: batches.devices_d7,
      d30: batches.devices_d30,
    },
    devices: {
      total: count(devicesTotal),
      owners: count(devicesOwners),
      by_platform: byPlatform,
    },
    batches: {
      total: batches.total,
      d1: batches.d1,
      d7: batches.d7,
    },
    partners: {
      total: count(partnersTotal),
      accepted: count(partnersAccepted),
      pending: count(partnersPending),
      watched_users: count(partnersWatchedUsers),
    },
    locked_passwords: {
      total: count(lockedPasswordsTotal),
    },
  };
}

export function utcDay(now: number) {
  return new Date(now).toISOString().slice(0, 10);
}

export async function snapshotAnalytics(env: Env, now = Date.now()): Promise<AnalyticsSnapshot> {
  const metrics = await computeAnalyticsMetrics(env.DB, now);
  const snapshot: AnalyticsSnapshot = { day: utcDay(now), metrics, created_at: now };
  await upsertAnalyticsSnapshot(env.DB, {
    day: snapshot.day,
    metrics: JSON.stringify(metrics),
    created_at: now,
  });
  return snapshot;
}
