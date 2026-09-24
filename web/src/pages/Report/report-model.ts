import { getRiskLevel } from '@virtueinitiative/shared-web/risk';
import type { FeedLog } from '../Logs/types';

export type ReportPeriod = 'day' | 'week';

export interface ReportWindow {
  start: number;
  end: number;
  /** First day in the window, as YYYY-MM-DD in local time. */
  firstDate: string;
}

/** Local-time YYYY-MM-DD (unlike toISOString, which is UTC). */
export function toDateString(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function parseDateString(date: string): Date {
  const [y, m, d] = date.split('-').map(Number);
  return new Date(y, m - 1, d);
}

export function shiftDateString(date: string, days: number): string {
  const d = parseDateString(date);
  d.setDate(d.getDate() + days);
  return toDateString(d);
}

/** A day report covers `date`; a week report covers the seven days ending on `date`. */
export function reportWindow(period: ReportPeriod, date: string): ReportWindow {
  const firstDate = period === 'week' ? shiftDateString(date, -6) : date;
  const end = parseDateString(date);
  end.setDate(end.getDate() + 1);
  return {
    start: parseDateString(firstDate).getTime(),
    end: end.getTime() - 1,
    firstDate,
  };
}

export interface RiskGroup {
  /** Flagged events other than screenshots (monitoring stopped, missed captures…). */
  alerts: FeedLog[];
  screenshots: FeedLog[];
}

export interface Report {
  high: RiskGroup;
  medium: RiskGroup;
  /** Every screenshot captured in the window, flagged or not. */
  screenshotCount: number;
}

export function isScreenshot(log: FeedLog): boolean {
  return log.type === 'screenshot';
}

// Most concerning first; ties go to the most recent.
function byConcern(a: FeedLog, b: FeedLog): number {
  return (b.risk ?? 0) - (a.risk ?? 0) || b.ts - a.ts;
}

/** Buckets a window's logs by concern level, the way the report presents them. */
export function buildReport(logs: FeedLog[], window: ReportWindow): Report {
  const report: Report = {
    high: { alerts: [], screenshots: [] },
    medium: { alerts: [], screenshots: [] },
    screenshotCount: 0,
  };

  for (const log of logs) {
    if (log.ts < window.start || log.ts > window.end) continue;
    if (isScreenshot(log)) report.screenshotCount++;

    const level = getRiskLevel(log.risk);
    const group =
      level === 'alert' || level === 'high'
        ? report.high
        : level === 'medium'
          ? report.medium
          : null;
    if (!group) continue;
    (isScreenshot(log) ? group.screenshots : group.alerts).push(log);
  }

  for (const group of [report.high, report.medium]) {
    group.alerts.sort(byConcern);
    group.screenshots.sort(byConcern);
  }
  return report;
}

function minutesLabel(ms: number): string {
  const minutes = Math.max(1, Math.round(ms / 60_000));
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? '' : 's'}`;
  const hours = Math.round(minutes / 6) / 10;
  return `${hours} hour${hours === 1 ? '' : 's'}`;
}

/**
 * Plain-language context for a flagged alert: what it means and why a partner
 * might want to ask about it. `allLogs` lets a "monitoring stopped" alert say
 * when monitoring came back on.
 */
export function explainAlert(log: FeedLog, allLogs: FeedLog[]): string {
  const data = log.data ?? {};
  switch (log.type) {
    case 'user_stop': {
      const resumed = allLogs
        .filter((l) => l.type === 'user_start' && l.device_id === log.device_id && l.ts > log.ts)
        .sort((a, b) => a.ts - b.ts)[0];
      return resumed
        ? `Monitoring was turned off, then turned back on ${minutesLabel(resumed.ts - log.ts)} later. Nothing was recorded in between.`
        : 'Monitoring was turned off and has not been turned back on yet. Nothing is being recorded on this device.';
    }
    case 'repeated_restarts': {
      const count = typeof data.count === 'number' ? data.count : null;
      const window = typeof data.window_ms === 'number' ? data.window_ms : null;
      const what = count ? `restarted ${count} times` : 'restarted several times';
      const within = window ? ` within ${minutesLabel(window)}` : '';
      return `The monitoring app ${what}${within}. Updates can cause this, but so can someone trying to stop it.`;
    }
    case 'screenshot_missed':
      return 'Scheduled screenshots did not happen on time. The device may have been asleep, or monitoring may have been interrupted.';
    case 'capture_failed':
      return 'The device could not take screenshots for a while. This sometimes follows a permissions or settings change.';
    case 'alert':
      return typeof data.message === 'string' && data.message
        ? data.message
        : 'The monitoring app raised an alert on this device.';
    default:
      return 'This activity was flagged as unusual for this device.';
  }
}
