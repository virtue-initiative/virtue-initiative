import { DigestFrequency, TamperSeverity } from './email-domain';
import { formatUtcDate, getDigestWindowForRun, reportDateForWindow } from './digest-schedule';
import { listDigestActivity, listDigestEligiblePartnerships } from './db';
import { sendEmail } from './email';
import { renderPartnerDigestTemplate } from './email/templates';
import { Env } from '../types/bindings';

const DEFAULT_CAPTURE_INTERVAL_MS = 300 * 1000;
const DAY_MS = 24 * 60 * 60 * 1000;
// API-057: SES allows the account 14 emails a second, shared with the alert and
// account emails sent while a digest run is going. Pacing digests below that
// leaves those room.
const DIGEST_SENDS_PER_SECOND = 10;
// How many watchers' digests are built at once. Each build is a D1 round trip,
// so a few in flight keep the paced sends busy.
const DIGEST_CONCURRENCY = 4;

type DigestPartnership = Awaited<ReturnType<typeof listDigestEligiblePartnerships>>[number];

function countApproximateScreenshots(
  batches: Array<{ start_time: number; end_time: number }>,
  captureIntervalMs: number,
  window: { start: number; end: number },
) {
  return batches.reduce((total, batch) => {
    const overlapMs =
      Math.min(batch.end_time, window.end) - Math.max(batch.start_time, window.start);

    if (overlapMs <= 0) {
      return total;
    }

    return total + Math.max(1, Math.round(overlapMs / captureIntervalMs));
  }, 0);
}

// High-risk events now live inside end-to-end encrypted batches, so the server can
// no longer read individual event risks. Each batch instead reports how many of its
// events fell in the high (>= 0.7) and medium (0.4–0.7) risk bands; those map to the
// `critical` and `warning` tamper severities respectively.
function summarizeTamperCounts(
  batches: Array<{ high_risk_count: number; medium_risk_count: number }>,
) {
  const counts: Record<TamperSeverity, number> = { info: 0, warning: 0, critical: 0 };
  for (const batch of batches) {
    counts.critical += batch.high_risk_count;
    counts.warning += batch.medium_risk_count;
  }
  return counts;
}

function batchOverlapsWindow(
  batch: { start_time: number; end_time: number },
  windowStart: number,
  windowEnd: number,
) {
  return batch.end_time > windowStart && batch.start_time < windowEnd;
}

function hasActivityInWindow(
  deviceId: string,
  batches: Array<{ device_id: string; start_time: number; end_time: number }>,
  windowStart: number,
  windowEnd: number,
) {
  return batches.some(
    (batch) => batch.device_id === deviceId && batchOverlapsWindow(batch, windowStart, windowEnd),
  );
}

function collectMissingLogPeriods(
  cadence: DigestFrequency,
  devices: Array<{ id: string; name: string; created_at: number }>,
  batches: Array<{ device_id: string; start_time: number; end_time: number }>,
  windowStart: number,
  windowEnd: number,
) {
  const missing: string[] = [];
  for (const device of devices) {
    const firstRelevantTime = Math.max(windowStart, device.created_at);

    if (firstRelevantTime >= windowEnd) {
      continue;
    }

    if (cadence === 'daily') {
      if (!hasActivityInWindow(device.id, batches, firstRelevantTime, windowEnd)) {
        missing.push(`${device.name}: no logs in the last 24 hours`);
      }
      continue;
    }

    for (let bucketStart = firstRelevantTime; bucketStart < windowEnd; bucketStart += DAY_MS) {
      const bucketEnd = Math.min(bucketStart + DAY_MS, windowEnd);
      if (!hasActivityInWindow(device.id, batches, bucketStart, bucketEnd)) {
        missing.push(`${device.name}: no logs on ${formatUtcDate(bucketStart)}`);
      }
    }
  }

  return missing;
}

// Returns a function that resolves once the caller may send, spacing calls at
// least `intervalMs` apart however many callers are waiting.
function createSendPacer(intervalMs: number) {
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

async function forEachConcurrently<T>(
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

// Builds and sends one watcher's digest if it is due this run. Returns whether
// a digest was due.
async function sendDigestIfDue(
  env: Env,
  watcherPartnerships: DigestPartnership[],
  now: number,
  pace: () => Promise<void>,
) {
  const recipient = watcherPartnerships[0];
  const { email_frequency: emailFrequency, timezone } = recipient.settings;
  if (emailFrequency !== 'daily' && emailFrequency !== 'weekly') {
    return false;
  }

  const window = getDigestWindowForRun({
    cadence: emailFrequency,
    now,
    timezone,
  });
  if (!window) {
    return false;
  }

  const reportQuery = new URLSearchParams({
    period: emailFrequency === 'weekly' ? 'week' : 'day',
    date: reportDateForWindow(window, timezone),
  });

  const activity = await listDigestActivity(
    env.DB,
    watcherPartnerships.map((partnership) => partnership.watching_user_id),
    window.start,
    window.end,
  );
  const batchesByUser = groupBy(activity.batches, (batch) => batch.user_id);
  const devicesByUser = groupBy(activity.devices, (device) => device.owner);

  const partnerSummaries = watcherPartnerships
    .slice()
    .sort((a, b) =>
      (a.watching_user_name ?? a.watching_user_email).localeCompare(
        b.watching_user_name ?? b.watching_user_email,
      ),
    )
    .map((partnership) => {
      const batches = batchesByUser.get(partnership.watching_user_id) ?? [];
      const devices = devicesByUser.get(partnership.watching_user_id) ?? [];

      return {
        partnershipId: partnership.partnership_id,
        ownerName: partnership.watching_user_name,
        ownerEmail: partnership.watching_user_email,
        reportUrl: `${env.APP_URL}/report/${partnership.watching_user_id}?${reportQuery}`,
        approxScreenshotCount: countApproximateScreenshots(
          batches,
          DEFAULT_CAPTURE_INTERVAL_MS,
          window,
        ),
        tamperCounts: summarizeTamperCounts(batches),
        missingLogDays: collectMissingLogPeriods(
          emailFrequency,
          devices,
          batches,
          window.start,
          window.end,
        ),
      };
    });

  const email = renderPartnerDigestTemplate({
    cadence: emailFrequency,
    appName: env.APP_NAME,
    partnerSummaries,
    appUrl: env.APP_URL,
  });

  await pace();
  await sendEmail({
    env,
    db: env.DB,
    kind: emailFrequency === 'weekly' ? 'weekly_digest' : 'daily_digest',
    recipient: recipient.watcher_email,
    // listDigestEligiblePartnerships only returns verified watchers.
    recipientEmailVerified: 1,
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
  return true;
}

function groupBy<T>(items: T[], key: (item: T) => string) {
  const groups = new Map<string, T[]>();
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
  const pace = createSendPacer(1000 / DIGEST_SENDS_PER_SECOND);

  let sent = 0;
  let failed = 0;
  await forEachConcurrently(
    [...partnershipsByWatcher.values()],
    DIGEST_CONCURRENCY,
    async (watcherPartnerships) => {
      // API-057: one watcher's failure must not stop the rest.
      try {
        if (await sendDigestIfDue(env, watcherPartnerships, now, pace)) {
          sent++;
        }
      } catch (error) {
        failed++;
        console.error('digest email failed', {
          watcher_user_id: watcherPartnerships[0].watcher_user_id,
          error,
        });
      }
    },
  );

  if (sent > 0 || failed > 0) {
    console.info('digest run finished', { sent, failed });
  }
}
