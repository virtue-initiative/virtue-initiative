import { DEFAULT_EMAIL_FREQUENCY, DigestFrequency, TamperSeverity } from './email-domain';
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

function getCaptureIntervalMs(env: Env) {
  const seconds = Number.parseInt(env.DEFAULT_CAPTURE_INTERVAL_SECONDS, 10);
  return (Number.isFinite(seconds) && seconds > 0 ? seconds : 300) * 1000;
}

function startOfUtcDay(timestamp: number) {
  const date = new Date(timestamp);
  return Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate());
}

function getDailyWindow(now: number) {
  const end = startOfUtcDay(now);
  return { start: end - 24 * 60 * 60 * 1000, end };
}

function getWeeklyWindow(now: number) {
  const end = startOfUtcDay(now);
  return { start: end - 7 * 24 * 60 * 60 * 1000, end };
}

function isWeeklyRun(now: number) {
  return new Date(now).getUTCDay() === 1;
}

function countApproximateScreenshots(
  batchCount: number,
  tamperAlertCount: number,
  captureIntervalMs: number,
) {
  const screenshotsPerBlock = Math.max(1, Math.round((60 * 60 * 1000) / captureIntervalMs));
  return batchCount * screenshotsPerBlock + tamperAlertCount;
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

function getCadenceWindow(cadence: DigestFrequency, now: number) {
  if (cadence === 'weekly') {
    return isWeeklyRun(now) ? getWeeklyWindow(now) : null;
  }

  return getDailyWindow(now);
}

function collectMissingLogDays(
  devices: Array<{ id: string; name: string; created_at: number }>,
  logs: Array<{ device_id: string; ts: number }>,
  windowStart: number,
  windowEnd: number,
) {
  const seenDaysByDevice = new Map<string, Set<number>>();
  for (const log of logs) {
    const dayStart = startOfUtcDay(log.ts);
    const current = seenDaysByDevice.get(log.device_id) ?? new Set<number>();
    current.add(dayStart);
    seenDaysByDevice.set(log.device_id, current);
  }

  const missing: string[] = [];
  for (const device of devices) {
    const firstRelevantDay = startOfUtcDay(Math.max(windowStart, device.created_at));
    const seenDays = seenDaysByDevice.get(device.id) ?? new Set<number>();
    for (let dayStart = firstRelevantDay; dayStart < windowEnd; dayStart += 24 * 60 * 60 * 1000) {
      if (!seenDays.has(dayStart)) {
        missing.push(`${device.name}: no logs on ${new Date(dayStart).toISOString().slice(0, 10)}`);
      }
    }
  }

  return missing;
}

export async function runNotificationSchedule(env: Env, now = Date.now()) {
  const captureIntervalMs = getCaptureIntervalMs(env);
  const partnerships = await listDigestEligiblePartnerships(env.DB);

  for (const partnership of partnerships) {
    const emailFrequency = partnership.email_frequency ?? DEFAULT_EMAIL_FREQUENCY;
    if (emailFrequency !== 'daily' && emailFrequency !== 'weekly') {
      continue;
    }

    const window = getCadenceWindow(emailFrequency, now);
    if (!window) {
      continue;
    }

    const [batches, riskLogs, deviceLogs, devices] = await Promise.all([
      listBatchWindowsForUser(env.DB, partnership.user_id, window.start, window.end),
      listRiskDeviceLogsForUser(env.DB, partnership.user_id, window.start, window.end),
      listDeviceLogsForUser(env.DB, partnership.user_id, window.start, window.end),
      listEnabledDevicesForUser(env.DB, partnership.user_id),
    ]);

    const approxScreenshotCount = countApproximateScreenshots(
      batches.length,
      riskLogs.length,
      captureIntervalMs,
    );
    const tamperCounts = summarizeTamperCounts(riskLogs);
    const missingLogDays = collectMissingLogDays(devices, deviceLogs, window.start, window.end);
    const email = renderPartnerDigestTemplate({
      cadence: emailFrequency,
      appName: env.APP_NAME,
      ownerName: partnership.owner_name,
      ownerEmail: partnership.owner_email,
      approxScreenshotCount,
      tamperCounts,
      missingLogDays,
      appUrl: env.APP_URL,
    });

    await sendEmail({
      env,
      db: env.DB,
      kind: emailFrequency === 'weekly' ? 'weekly_digest' : 'daily_digest',
      recipient: partnership.partner_email,
      subject: email.subject,
      text: email.text,
      html: email.html,
      related_user_id: partnership.user_id,
      related_partnership_id: partnership.partnership_id,
      metadata: {
        email_frequency: emailFrequency,
        windowStart: window.start,
        windowEnd: window.end,
        missingLogDays,
      },
    });
  }
}
