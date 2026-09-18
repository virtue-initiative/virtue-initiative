// api/SPEC.md API-051: headline metrics, snapshotted once per UTC day by the
// cron in src/index.ts and on demand by POST /admin/analytics/refresh.
import type { AnalyticsMetrics, AnalyticsSnapshot } from '../../../shared-web/types';
import { upsertAnalyticsSnapshot } from './db';
import { Env } from '../types/bindings';

const DAY_MS = 24 * 60 * 60 * 1000;

type CountRow = { n: number };
type PlatformRow = { platform: string; n: number };

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
    active1d,
    active7d,
    active30d,
    activeDevices1d,
    activeDevices7d,
    activeDevices30d,
    devicesTotal,
    devicesOwners,
    devicesByPlatform,
    batchesTotal,
    batches1d,
    batches7d,
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
    db
      .prepare('SELECT COUNT(DISTINCT user_id) AS n FROM batches WHERE created_at > ?')
      .bind(since1d),
    db
      .prepare('SELECT COUNT(DISTINCT user_id) AS n FROM batches WHERE created_at > ?')
      .bind(since7d),
    db
      .prepare('SELECT COUNT(DISTINCT user_id) AS n FROM batches WHERE created_at > ?')
      .bind(since30d),
    db
      .prepare('SELECT COUNT(DISTINCT device_id) AS n FROM batches WHERE created_at > ?')
      .bind(since1d),
    db
      .prepare('SELECT COUNT(DISTINCT device_id) AS n FROM batches WHERE created_at > ?')
      .bind(since7d),
    db
      .prepare('SELECT COUNT(DISTINCT device_id) AS n FROM batches WHERE created_at > ?')
      .bind(since30d),
    db.prepare('SELECT COUNT(*) AS n FROM devices WHERE deleted_at IS NULL'),
    db.prepare('SELECT COUNT(DISTINCT owner) AS n FROM devices WHERE deleted_at IS NULL'),
    db.prepare(
      'SELECT platform, COUNT(*) AS n FROM devices WHERE deleted_at IS NULL GROUP BY platform',
    ),
    db.prepare('SELECT COUNT(*) AS n FROM batches'),
    db.prepare('SELECT COUNT(*) AS n FROM batches WHERE created_at > ?').bind(since1d),
    db.prepare('SELECT COUNT(*) AS n FROM batches WHERE created_at > ?').bind(since7d),
    db.prepare('SELECT COUNT(*) AS n FROM partners'),
    db.prepare("SELECT COUNT(*) AS n FROM partners WHERE status = 'accepted'"),
    db.prepare("SELECT COUNT(*) AS n FROM partners WHERE status = 'pending'"),
    db.prepare(
      "SELECT COUNT(DISTINCT watching_user_id) AS n FROM partners WHERE status = 'accepted'",
    ),
    db.prepare('SELECT COUNT(*) AS n FROM locked_passwords WHERE deleted_at IS NULL'),
  ]);

  const users = count(usersTotal);

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
      d1: count(active1d),
      d7: count(active7d),
      d30: count(active30d),
    },
    active_devices: {
      d1: count(activeDevices1d),
      d7: count(activeDevices7d),
      d30: count(activeDevices30d),
    },
    devices: {
      total: count(devicesTotal),
      owners: count(devicesOwners),
      by_platform: byPlatform,
    },
    batches: {
      total: count(batchesTotal),
      d1: count(batches1d),
      d7: count(batches7d),
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
