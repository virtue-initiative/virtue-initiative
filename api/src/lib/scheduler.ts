import { DigestFrequency, TamperSeverity } from './email-domain';
import {
  DAY_MS,
  formatUtcDate,
  getDigestWindowForRun,
  reportDateForWindow,
} from './digest-schedule';
import { listDigestActivity, listDigestEligiblePartnerships } from './db';
import { sendEmail } from './email';
import { renderPartnerDigestTemplate } from './email/templates';
import { Env } from '../types/bindings';

const DEFAULT_CAPTURE_INTERVAL_MS = 300 * 1000;
// API-057: SES allows the account 14 emails a second, shared with the alert and
// account emails sent while a digest run is going. Pacing digests below that
// leaves those room.
const DIGEST_SENDS_PER_SECOND = 10;

type DigestPartnership = Awaited<ReturnType<typeof listDigestEligiblePartnerships>>[number];
type DigestActivityRow = Awaited<ReturnType<typeof listDigestActivity>>[number];

// One watched user's activity in a window, built once from listDigestActivity's
// rows and shared by every digest that covers the user.
interface UserActivity {
  screenshots: number;
  critical: number;
  warning: number;
  // In the order listDigestActivity returns them, newest first. `days` are the
  // days with logs, counted from the later of the window start and created_at.
  devices: Array<{ name: string; created_at: number; days: Set<number> }>;
}

function summarizeUserActivity(rows: DigestActivityRow[]): UserActivity {
  const activity: UserActivity = { screenshots: 0, critical: 0, warning: 0, devices: [] };
  const devices = new Map<string, UserActivity['devices'][number]>();
  for (const row of rows) {
    activity.screenshots += row.screenshots ?? 0;
    activity.critical += row.critical ?? 0;
    activity.warning += row.warning ?? 0;

    let device = devices.get(row.id);
    if (!device) {
      device = { name: row.name, created_at: row.created_at, days: new Set() };
      devices.set(row.id, device);
      activity.devices.push(device);
    }
    if (row.first_day !== null && row.last_day !== null) {
      for (let day = row.first_day; day <= row.last_day; day++) {
        device.days.add(day);
      }
    }
  }
  return activity;
}

function collectMissingLogPeriods(
  cadence: DigestFrequency,
  devices: UserActivity['devices'],
  windowStart: number,
  windowEnd: number,
) {
  const missing: string[] = [];
  for (const device of devices) {
    // Day 0 starts here, as in listDigestActivity.
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
// least `intervalMs` apart.
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

interface DueDigest {
  cadence: DigestFrequency;
  timezone: string;
  window: { start: number; end: number };
  // The watcher; every partnership row repeats the watcher's fields.
  recipient: DigestPartnership;
  partnerships: DigestPartnership[];
}

// Returns the digest a watcher is due this run, if any.
function findDueDigest(watcherPartnerships: DigestPartnership[], now: number): DueDigest | null {
  const recipient = watcherPartnerships[0];
  const { email_frequency: cadence, timezone } = recipient.settings;
  if (cadence !== 'daily' && cadence !== 'weekly') {
    return null;
  }

  const window = getDigestWindowForRun({ cadence, now, timezone });
  return window
    ? { cadence, timezone, window, recipient, partnerships: watcherPartnerships }
    : null;
}

function windowKey(digest: DueDigest) {
  return `${digest.window.start}-${digest.window.end}`;
}

// Loads the activity for every due digest at once (API-057), keyed by window
// and then by watched user. Digests whose timezones share an offset share a
// window, so a run has only a few distinct windows, loaded concurrently. Each
// is summed up in SQL over all its watched users, reading each user once
// however many watchers they have.
async function loadDigestActivity(env: Env, due: DueDigest[]) {
  const byWindow = new Map<string, Map<string, UserActivity>>();
  await Promise.all(
    Array.from(groupBy(due, windowKey), async ([key, digests]) => {
      const userIds = new Set(
        digests.flatMap((digest) =>
          digest.partnerships.map((partnership) => partnership.watching_user_id),
        ),
      );
      const rows = await listDigestActivity(
        env.DB,
        Array.from(userIds),
        digests[0].window,
        DEFAULT_CAPTURE_INTERVAL_MS,
      );
      const byUser = new Map<string, UserActivity>();
      for (const [userId, userRows] of groupBy(rows, (row) => row.owner)) {
        byUser.set(userId, summarizeUserActivity(userRows));
      }
      byWindow.set(key, byUser);
    }),
  );
  return byWindow;
}

// Builds and sends one due digest from the loaded activity. Returns whether
// sendEmail skipped it.
async function sendDigest(env: Env, digest: DueDigest, activity: Map<string, UserActivity>) {
  const { cadence: emailFrequency, timezone, window, recipient } = digest;

  const reportQuery = new URLSearchParams({
    period: emailFrequency === 'weekly' ? 'week' : 'day',
    date: reportDateForWindow(window, timezone),
  });

  const partnerSummaries = digest.partnerships
    .slice()
    .sort((a, b) =>
      (a.watching_user_name ?? a.watching_user_email).localeCompare(
        b.watching_user_name ?? b.watching_user_email,
      ),
    )
    .map((partnership) => {
      const user = activity.get(partnership.watching_user_id);
      // High-risk events live inside end-to-end encrypted batches, so the server
      // can't read individual event risks. Each batch instead reports how many of
      // its events fell in the high (>= 0.7) and medium (0.4–0.7) risk bands; those
      // map to the `critical` and `warning` tamper severities respectively.
      const tamperCounts: Record<TamperSeverity, number> = {
        info: 0,
        warning: user?.warning ?? 0,
        critical: user?.critical ?? 0,
      };

      return {
        partnershipId: partnership.partnership_id,
        ownerName: partnership.watching_user_name,
        ownerEmail: partnership.watching_user_email,
        reportUrl: `${env.APP_URL}/report/${partnership.watching_user_id}?${reportQuery}`,
        approxScreenshotCount: user?.screenshots ?? 0,
        tamperCounts,
        missingLogDays: collectMissingLogPeriods(
          emailFrequency,
          user?.devices ?? [],
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

  const bounceId = recipient.watcher_permanent_bounce_id;
  const result = await sendEmail({
    env,
    db: env.DB,
    kind: emailFrequency === 'weekly' ? 'weekly_digest' : 'daily_digest',
    recipient: recipient.watcher_email,
    recipientEmailVerified: recipient.watcher_email_verified,
    recipientPermanentBounce: bounceId ? { id: bounceId } : null,
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
      watchedUserIds: digest.partnerships.map((partnership) => partnership.watching_user_id),
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

  // If this fails, no digest can be built, so the run fails before sending any.
  const activityByWindow = await loadDigestActivity(env, due);
  const pace = createSendPacer(1000 / DIGEST_SENDS_PER_SECOND);
  let sent = 0;
  let skipped = 0;
  for (const digest of due) {
    try {
      await pace();
      if (await sendDigest(env, digest, activityByWindow.get(windowKey(digest))!)) {
        skipped++;
      } else {
        sent++;
      }
    } catch (error) {
      logFailure(digest.recipient.watcher_user_id, error);
    }
  }

  console.info('digest run finished', { due: due.length, sent, skipped, failed });
  // Fail the invocation so it shows as an error in Cloudflare, rather than
  // completing cleanly when, say, every send was rejected.
  if (failed > 0) {
    throw new Error(`${failed} digest email(s) failed; see the logged errors`);
  }
}
