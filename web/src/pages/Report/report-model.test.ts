import { describe, expect, it } from 'vitest';
import type { FeedLog } from '../Logs/types';
import {
  buildReport,
  explainAlert,
  reportWindow,
  shiftDateString,
  toDateString,
} from './report-model';

function log(id: string, type: string, ts: number, risk?: number, data = {}): FeedLog {
  return {
    id,
    device_id: 'device-1',
    ts,
    created_at: ts,
    type,
    data,
    risk,
    batch_status: 'verified',
    source: 'batch',
  };
}

describe('reportWindow', () => {
  it('covers one local day for a daily report', () => {
    const range = reportWindow('day', '2026-09-24');
    expect(range.start).toBe(new Date(2026, 8, 24).getTime());
    expect(range.end).toBe(new Date(2026, 8, 25).getTime() - 1);
    expect(range.firstDate).toBe('2026-09-24');
  });

  it('covers the seven days ending on the date for a weekly report', () => {
    const range = reportWindow('week', '2026-09-24');
    expect(range.firstDate).toBe('2026-09-18');
    expect(range.start).toBe(new Date(2026, 8, 18).getTime());
    expect(range.end).toBe(new Date(2026, 8, 25).getTime() - 1);
  });
});

describe('date strings', () => {
  it('formats in local time and shifts across month boundaries', () => {
    expect(toDateString(new Date(2026, 0, 5))).toBe('2026-01-05');
    expect(shiftDateString('2026-10-01', -1)).toBe('2026-09-30');
    expect(shiftDateString('2026-12-31', 1)).toBe('2027-01-01');
  });
});

describe('buildReport', () => {
  const range = reportWindow('day', '2026-09-24');
  const at = (hour: number) => new Date(2026, 8, 24, hour).getTime();

  it('buckets flagged events into high and medium alerts and screenshots', () => {
    const report = buildReport(
      [
        log('shot-safe', 'screenshot', at(9), 0),
        log('shot-alert', 'screenshot', at(10), 0.95),
        log('shot-high', 'screenshot', at(11), 0.75),
        log('shot-medium', 'screenshot', at(12), 0.5),
        log('stop', 'user_stop', at(13), 0.9),
        log('failed', 'capture_failed', at(14), 0.45),
        log('login', 'system_login', at(8)),
        // Outside the window.
        log('shot-yesterday', 'screenshot', at(-2), 0.99),
      ],
      range,
    );

    expect(report.high.screenshots.map((l) => l.id)).toEqual(['shot-alert', 'shot-high']);
    expect(report.high.alerts.map((l) => l.id)).toEqual(['stop']);
    expect(report.medium.screenshots.map((l) => l.id)).toEqual(['shot-medium']);
    expect(report.medium.alerts.map((l) => l.id)).toEqual(['failed']);
    expect(report.screenshotCount).toBe(4);
  });

  it('orders by concern, then most recent first', () => {
    const report = buildReport(
      [
        log('early', 'screenshot', at(9), 0.8),
        log('late', 'screenshot', at(15), 0.8),
        log('worst', 'screenshot', at(12), 0.97),
      ],
      range,
    );
    expect(report.high.screenshots.map((l) => l.id)).toEqual(['worst', 'late', 'early']);
  });
});

describe('explainAlert', () => {
  it('says when monitoring came back on after a stop', () => {
    const stop = log('stop', 'user_stop', 1_000_000, 0.9);
    const start = log('start', 'user_start', 1_000_000 + 25 * 60_000, 0);
    expect(explainAlert(stop, [stop, start])).toMatch(/turned back on 25 minutes later/);
  });

  it('flags a stop that was never undone', () => {
    const stop = log('stop', 'user_stop', 1_000_000, 0.9);
    expect(explainAlert(stop, [stop])).toMatch(/has not been turned back on/);
  });

  it('describes repeated restarts with their count and window', () => {
    const restarts = log('r', 'repeated_restarts', 0, 0.9, { count: 4, window_ms: 600_000 });
    expect(explainAlert(restarts, [])).toMatch(/restarted 4 times within 10 minutes/);
  });

  it('passes an alert message through', () => {
    expect(explainAlert(log('a', 'alert', 0, 0.8, { message: 'Custom alert.' }), [])).toBe(
      'Custom alert.',
    );
  });
});
