import { DigestFrequency, TamperSeverity } from './email-domain';
import { formatUtcDate, getDigestWindowForRun, reportDateForWindow } from './digest-schedule';
import {
  listDigestActivity,
  listDigestEligiblePartnerships,
  listPermanentEmailBounces,
} from './db';
import { sendEmail } from './email';
import { renderPartnerDigestTemplate } from './email/templates';
import { Env } from '../types/bindings';

const DEFAULT_CAPTURE_INTERVAL_MS = 300 * 1000;
const DAY_MS = 24 * 60 * 60 * 1000;
// API-057: SES allows the account 14 emails a second, shared with the alert and
// account emails sent while a digest run is going. Pacing digests below that
// leaves those room.
const DIGEST_SENDS_PER_SECOND = 10;
// How many digests are sent at once. Building one needs no I/O, so this only
// overlaps the SES round trips, which lets sends reach the paced rate.
const DIGEST_CONCURRENCY = 4;
// Watched users whose activity loads in one D1 batch call. listDigestActivity
// splits them into statements of 90, so a call is 10 statements of each kind,
// well inside D1's 30 second limit for a batch.
const DIGEST_LOAD_USERS = 900;

type DigestPartnership = Awaited<ReturnType<typeof listDigestEligiblePartnerships>>[number];
type DigestActivity = Awaited<ReturnType<typeof listDigestActivity>>;

// Lists the days each device has no logs on, from listDigestActivity's rows:
// one per device and run of days covered, counted from the later of the window
// start and the device's created_at.
function collectMissingLogPeriods(
  cadence: DigestFrequency,
  deviceDays: DigestActivity['devices'],
  windowStart: number,
  windowEnd: number,
) {
  const devices = new Map<string, { name: string; created_at: number; days: Set<number> }>();
  for (const row of deviceDays) {
    let device = devices.get(row.id);
    if (!device) {
      device = { name: row.name, created_at: row.created_at, days: new Set() };
      devices.set(row.id, device);
    }
    if (row.first_day !== null && row.last_day !== null) {
      for (let day = row.first_day; day <= row.last_day; day++) {
        device.days.add(day);
      }
    }
  }

  const missing: string[] = [];
  for (const device of devices.values()) {
    const firstRelevantTime = Math.max(windowStart, device.created_at);

    if (firstRelevantTime >= windowEnd) {
      continue;
    }

    if (cadence === 'daily') {
      if (device.days.size === 0) {
        missing.push(`${device.name}: no logs in the last 24 hours`);
      }
      continue;
    }

    for (
      let day = 0, bucketStart = firstRelevantTime;
      bucketStart < windowEnd;
      day++, bucketStart += DAY_MS
    ) {
      if (!device.days.has(day)) {
        missing.push(`${device.name}: no logs on ${formatUtcDate(bucketStart)}`);
      }
    }
  }

  return missing;
}

// Returns a function that resolves once the caller may send, spacing calls at
// least `intervalMs` apart however many callers are waiting.
export function createSendPacer(intervalMs: number) {
  let nextSlot = 0;
  return async () => {
    const now = Date.now();
    const slot = Math.max(now, nextSlot);
    nextSlot = slot + intervalMs;
    if (slot > now) {
      await new Promise((resolve) => setTimeout(resolve, slot - now));
    }
  };
}

export async function forEachConcurrently<T>(
  items: T[],
  concurrency: number,
  run: (item: T) => Promise<void>,
) {
  let next = 0;
  const workers = Array.from({ length: Math.min(concurrency, items.length) }, async () => {
    while (next < items.length) {
      await run(items[next++]);
    }
  });
  await Promise.all(workers);
}

interface DueDigest {
  cadence: DigestFrequency;
  timezone: string;
  window: { start: number; end: number };
  partnerships: DigestPartnership[];
}

// Returns the digest a watcher is due this run, if any.
function findDueDigest(watcherPartnerships: DigestPartnership[], now: number): DueDigest | null {
  const { email_frequency: cadence, timezone } = watcherPartnerships[0].settings;
  if (cadence !== 'daily' && cadence !== 'weekly') {
    return null;
  }

  const window = getDigestWindowForRun({ cadence, now, timezone });
  return window ? { cadence, timezone, window, partnerships: watcherPartnerships } : null;
}

interface WindowActivity {
  totalsByUser: Map<string, DigestActivity['totals'][number]>;
  devicesByUser: Map<string, DigestActivity['devices']>;
  // Watched users whose activity failed to load, with the error.
  loadErrors: Map<string, unknown>;
}

function windowKey(digest: DueDigest) {
  return `${digest.window.start}-${digest.window.end}`;
}

// Loads the activity for every due digest at once (API-057). Digests whose
// timezones share an offset share a window, so a run has only a few distinct
// windows. Each is summed up in SQL over all its watched users, reading each
// user once however many watchers they have, and returns totals rather than
// batch rows.
async function loadDigestActivity(env: Env, due: DueDigest[]) {
  const byWindow = new Map<string, WindowActivity>();
  for (const [key, digests] of groupBy(due, windowKey)) {
    const activity: WindowActivity = {
      totalsByUser: new Map(),
      devicesByUser: new Map(),
      loadErrors: new Map(),
    };
    byWindow.set(key, activity);

    const userIds = Array.from(
      new Set(
        digests.flatMap((digest) =>
          digest.partnerships.map((partnership) => partnership.watching_user_id),
        ),
      ),
    );

    for (let i = 0; i < userIds.length; i += DIGEST_LOAD_USERS) {
      const slice = userIds.slice(i, i + DIGEST_LOAD_USERS);
      try {
        const loaded = await listDigestActivity(
          env.DB,
          slice,
          digests[0].window,
          DEFAULT_CAPTURE_INTERVAL_MS,
        );
        for (const totals of loaded.totals) {
          activity.totalsByUser.set(totals.user_id, totals);
        }
        for (const [userId, devices] of groupBy(loaded.devices, (device) => device.owner)) {
          activity.devicesByUser.set(userId, devices);
        }
      } catch (error) {
        // Only the digests that cover these users fail; sendDigest throws for them.
        console.error('digest activity load failed', {
          window: digests[0].window,
          users: slice.length,
          error,
        });
        for (const userId of slice) {
          activity.loadErrors.set(userId, error);
        }
      }
    }
  }
  return byWindow;
}

// Builds and sends one due digest from the loaded activity. Returns whether
// sendEmail skipped it. `permanentBounce` is undefined when the bulk bounce
// lookup failed, so sendEmail checks this recipient itself.
async function sendDigest(
  env: Env,
  digest: DueDigest,
  activity: WindowActivity,
  permanentBounce: { id: string } | null | undefined,
  pace: () => Promise<void>,
) {
  const { cadence: emailFrequency, timezone, window, partnerships: watcherPartnerships } = digest;
  const recipient = watcherPartnerships[0];

  for (const partnership of watcherPartnerships) {
    if (activity.loadErrors.has(partnership.watching_user_id)) {
      throw new Error('activity for a watched user failed to load', {
        cause: activity.loadErrors.get(partnership.watching_user_id),
      });
    }
  }

  const reportQuery = new URLSearchParams({
    period: emailFrequency === 'weekly' ? 'week' : 'day',
    date: reportDateForWindow(window, timezone),
  });

  const partnerSummaries = watcherPartnerships
    .slice()
    .sort((a, b) =>
      (a.watching_user_name ?? a.watching_user_email).localeCompare(
        b.watching_user_name ?? b.watching_user_email,
      ),
    )
    .map((partnership) => {
      const totals = activity.totalsByUser.get(partnership.watching_user_id);
      const devices = activity.devicesByUser.get(partnership.watching_user_id) ?? [];
      // High-risk events live inside end-to-end encrypted batches, so the server
      // can't read individual event risks. Each batch instead reports how many of
      // its events fell in the high (>= 0.7) and medium (0.4–0.7) risk bands; those
      // map to the `critical` and `warning` tamper severities respectively.
      const tamperCounts: Record<TamperSeverity, number> = {
        info: 0,
        warning: totals?.warning ?? 0,
        critical: totals?.critical ?? 0,
      };

      return {
        partnershipId: partnership.partnership_id,
        ownerName: partnership.watching_user_name,
        ownerEmail: partnership.watching_user_email,
        reportUrl: `${env.APP_URL}/report/${partnership.watching_user_id}?${reportQuery}`,
        approxScreenshotCount: totals?.screenshots ?? 0,
        tamperCounts,
        missingLogDays: collectMissingLogPeriods(emailFrequency, devices, window.start, window.end),
      };
    });

  const email = renderPartnerDigestTemplate({
    cadence: emailFrequency,
    appName: env.APP_NAME,
    partnerSummaries,
    appUrl: env.APP_URL,
  });

  const result = await sendEmail({
    env,
    db: env.DB,
    kind: emailFrequency === 'weekly' ? 'weekly_digest' : 'daily_digest',
    recipient: recipient.watcher_email,
    recipientEmailVerified: recipient.watcher_email_verified,
    recipientPermanentBounce: permanentBounce,
    beforeDeliver: pace,
    subject: email.subject,
    text: email.text,
    html: email.html,
    related_user_id: recipient.watcher_user_id,
    metadata: {
      email_frequency: emailFrequency,
      timezone,
      windowStart: window.start,
      windowEnd: window.end,
      partnershipIds: partnerSummaries.map((summary) => summary.partnershipId),
      watchedUserIds: watcherPartnerships.map((partnership) => partnership.watching_user_id),
    },
  });
  return Boolean(result.skipped);
}

function groupBy<T, K>(items: T[], key: (item: T) => K) {
  const groups = new Map<K, T[]>();
  for (const item of items) {
    const group = groups.get(key(item));
    if (group) {
      group.push(item);
    } else {
      groups.set(key(item), [item]);
    }
  }
  return groups;
}

export async function runNotificationSchedule(env: Env, now = Date.now()) {
  const partnerships = await listDigestEligiblePartnerships(env.DB);
  const partnershipsByWatcher = groupBy(partnerships, (partnership) => partnership.watcher_user_id);

  let failed = 0;
  const logFailure = (watcherUserId: string, error: unknown) => {
    failed++;
    console.error('digest email failed', { watcher_user_id: watcherUserId, error });
  };

  // API-057: one watcher's failure must not stop the rest, here or below.
  const due: DueDigest[] = [];
  for (const [watcherUserId, watcherPartnerships] of partnershipsByWatcher) {
    try {
      const digest = findDueDigest(watcherPartnerships, now);
      if (digest) {
        due.push(digest);
      }
    } catch (error) {
      logFailure(watcherUserId, error);
    }
  }
  if (due.length === 0 && failed === 0) {
    return;
  }
  // Logged up front, so a run cut off by a platform limit still shows how many
  // digests it owed.
  console.info('digest run started', { due: due.length });

  const activityByWindow = await loadDigestActivity(env, due);
  // One lookup for every recipient. If it fails, sendEmail checks each one itself.
  let bounceFor: ((email: string) => { id: string } | null) | undefined;
  try {
    bounceFor = await listPermanentEmailBounces(
      env.DB,
      due.map((digest) => digest.partnerships[0].watcher_email),
    );
  } catch (error) {
    console.error('digest bounce lookup failed; checking each send instead', error);
  }

  const pace = createSendPacer(1000 / DIGEST_SENDS_PER_SECOND);
  let sent = 0;
  let skipped = 0;
  await forEachConcurrently(due, DIGEST_CONCURRENCY, async (digest) => {
    try {
      const skippedBySend = await sendDigest(
        env,
        digest,
        activityByWindow.get(windowKey(digest))!,
        bounceFor?.(digest.partnerships[0].watcher_email),
        pace,
      );
      if (skippedBySend) {
        skipped++;
      } else {
        sent++;
      }
    } catch (error) {
      logFailure(digest.partnerships[0].watcher_user_id, error);
    }
  });

  console.info('digest run finished', { due: due.length, sent, skipped, failed });
  // Fail the invocation so it shows as an error in Cloudflare, rather than
  // completing cleanly when, say, every send was rejected.
  if (failed > 0) {
    throw new Error(`${failed} digest email(s) failed; see the logged errors`);
  }
}
