import { DEFAULT_EMAIL_FREQUENCY, DigestFrequency, TamperSeverity } from './email-domain';
import { formatUtcDate, getDigestWindowForRun } from './digest-schedule';
import {
  listBatchWindowsForUser,
  listDeviceLogsForUser,
  listDigestEligiblePartnerships,
  listEnabledDevicesForUser,
  listRiskDeviceLogsForUser,
} from './db';
import { sendEmail } from './email';
import { renderPartnerDigestTemplate } from './email/templates';
import { riskToSeverity } from './tamper';
import { Env } from '../types/bindings';

const DEFAULT_CAPTURE_INTERVAL_MS = 300 * 1000;
const DAY_MS = 24 * 60 * 60 * 1000;

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

function summarizeTamperCounts(events: Array<{ risk: number | null }>) {
  const counts: Record<TamperSeverity, number> = { info: 0, warning: 0, critical: 0 };
  for (const event of events) {
    const severity = riskToSeverity(event.risk);
    if (severity) {
      counts[severity] += 1;
    }
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

function logFallsWithinWindow(log: { ts: number }, windowStart: number, windowEnd: number) {
  return log.ts >= windowStart && log.ts < windowEnd;
}

function hasActivityInWindow(
  deviceId: string,
  batches: Array<{ device_id: string; start_time: number; end_time: number }>,
  logs: Array<{ device_id: string; ts: number }>,
  windowStart: number,
  windowEnd: number,
) {
  return (
    batches.some(
      (batch) => batch.device_id === deviceId && batchOverlapsWindow(batch, windowStart, windowEnd),
    ) ||
    logs.some(
      (log) => log.device_id === deviceId && logFallsWithinWindow(log, windowStart, windowEnd),
    )
  );
}

function collectMissingLogPeriods(
  cadence: DigestFrequency,
  devices: Array<{ id: string; name: string; created_at: number }>,
  batches: Array<{ device_id: string; start_time: number; end_time: number }>,
  logs: Array<{ device_id: string; ts: number }>,
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
      if (!hasActivityInWindow(device.id, batches, logs, firstRelevantTime, windowEnd)) {
        missing.push(`${device.name}: no logs in the last 24 hours`);
      }
      continue;
    }

    for (let bucketStart = firstRelevantTime; bucketStart < windowEnd; bucketStart += DAY_MS) {
      const bucketEnd = Math.min(bucketStart + DAY_MS, windowEnd);
      if (!hasActivityInWindow(device.id, batches, logs, bucketStart, bucketEnd)) {
        missing.push(`${device.name}: no logs on ${formatUtcDate(bucketStart)}`);
      }
    }
  }

  return missing;
}

export async function runNotificationSchedule(env: Env, now = Date.now()) {
  const partnerships = await listDigestEligiblePartnerships(env.DB);
  const partnershipsByWatcher = new Map<string, typeof partnerships>();

  for (const partnership of partnerships) {
    const current = partnershipsByWatcher.get(partnership.watcher_user_id) ?? [];
    current.push(partnership);
    partnershipsByWatcher.set(partnership.watcher_user_id, current);
  }

  for (const watcherPartnerships of partnershipsByWatcher.values()) {
    const recipient = watcherPartnerships[0];
    if (!recipient) {
      continue;
    }

    const emailFrequency = recipient.email_frequency ?? DEFAULT_EMAIL_FREQUENCY;
    if (emailFrequency !== 'daily' && emailFrequency !== 'weekly') {
      continue;
    }

    const window = getDigestWindowForRun({
      cadence: emailFrequency,
      now,
      utcMinutes: recipient.email_digest_minutes_utc,
    });
    if (!window) {
      continue;
    }

    const partnerSummaries = await Promise.all(
      watcherPartnerships
        .slice()
        .sort((a, b) =>
          (a.watching_user_name ?? a.watching_user_email).localeCompare(
            b.watching_user_name ?? b.watching_user_email,
          ),
        )
        .map(async (partnership) => {
          const [batches, riskLogs, deviceLogs, devices] = await Promise.all([
            listBatchWindowsForUser(env.DB, partnership.watching_user_id, window.start, window.end),
            listRiskDeviceLogsForUser(
              env.DB,
              partnership.watching_user_id,
              window.start,
              window.end,
            ),
            listDeviceLogsForUser(env.DB, partnership.watching_user_id, window.start, window.end),
            listEnabledDevicesForUser(env.DB, partnership.watching_user_id),
          ]);

          return {
            partnershipId: partnership.partnership_id,
            ownerName: partnership.watching_user_name,
            ownerEmail: partnership.watching_user_email,
            approxScreenshotCount: countApproximateScreenshots(
              batches,
              DEFAULT_CAPTURE_INTERVAL_MS,
              window,
            ),
            tamperCounts: summarizeTamperCounts(riskLogs),
            missingLogDays: collectMissingLogPeriods(
              emailFrequency,
              devices,
              batches,
              deviceLogs,
              window.start,
              window.end,
            ),
          };
        }),
    );

    const email = renderPartnerDigestTemplate({
      cadence: emailFrequency,
      appName: env.APP_NAME,
      partnerSummaries,
      appUrl: env.APP_URL,
    });

    await sendEmail({
      env,
      db: env.DB,
      kind: emailFrequency === 'weekly' ? 'weekly_digest' : 'daily_digest',
      recipient: recipient.watcher_email,
      subject: email.subject,
      text: email.text,
      html: email.html,
      related_user_id: recipient.watcher_user_id,
      metadata: {
        email_frequency: emailFrequency,
        email_digest_minutes_utc: recipient.email_digest_minutes_utc,
        windowStart: window.start,
        windowEnd: window.end,
        partnershipIds: partnerSummaries.map((summary) => summary.partnershipId),
        watchedUserIds: watcherPartnerships.map((partnership) => partnership.watching_user_id),
      },
    });
  }
}
