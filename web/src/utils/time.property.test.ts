// Property tests for the compact time display (WEB-023).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import fc from 'fast-check';
import { formatCompactRelativeTimestamp } from './time';

const NOW = new Date('2026-06-15T12:00:00.000Z').getTime();
const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
});
afterEach(() => vi.useRealTimers());

const UNIT_RANK = { now: 0, m: 1, h: 2, d: 3, w: 4, y: 5 } as const;

function parse(text: string) {
  const match = /^(?:(now)|(\d+)([mhdwy]) ago)$/.exec(text);
  if (!match) return null;
  return match[1]
    ? { unit: 'now' as const, amount: 0 }
    : { unit: match[3] as keyof typeof UNIT_RANK, amount: Number(match[2]) };
}

// Whole seconds, so rounding to the second does not matter.
const ageArb = fc.integer({ min: 0, max: 50 * 365 * 86_400 });

describe('formatCompactRelativeTimestamp (WEB-023)', () => {
  it('reads "now" under a minute and otherwise "<n><unit> ago" in the largest whole unit', () => {
    fc.assert(
      fc.property(ageArb, (seconds) => {
        const parsed = parse(formatCompactRelativeTimestamp(NOW - seconds * SECOND));
        expect(parsed).not.toBeNull();
        const ms = seconds * SECOND;
        if (ms < MINUTE) expect(parsed).toEqual({ unit: 'now', amount: 0 });
        else if (ms < HOUR) expect(parsed).toEqual({ unit: 'm', amount: Math.floor(ms / MINUTE) });
        else if (ms < DAY) expect(parsed).toEqual({ unit: 'h', amount: Math.floor(ms / HOUR) });
        else if (ms < 7 * DAY) expect(parsed).toEqual({ unit: 'd', amount: Math.floor(ms / DAY) });
        else if (ms < 365 * DAY)
          expect(parsed).toEqual({ unit: 'w', amount: Math.floor(ms / (7 * DAY)) });
        else expect(parsed).toEqual({ unit: 'y', amount: Math.floor(ms / (365 * DAY)) });
      }),
    );
  });

  it('never shows a smaller unit for an older time', () => {
    fc.assert(
      fc.property(ageArb, ageArb, (a, b) => {
        const [younger, older] = a <= b ? [a, b] : [b, a];
        const y = parse(formatCompactRelativeTimestamp(NOW - younger * SECOND))!;
        const o = parse(formatCompactRelativeTimestamp(NOW - older * SECOND))!;
        expect(UNIT_RANK[o.unit]).toBeGreaterThanOrEqual(UNIT_RANK[y.unit]);
        if (o.unit === y.unit) expect(o.amount).toBeGreaterThanOrEqual(y.amount);
      }),
    );
  });

  it('reads "now" for a time in the future', () => {
    fc.assert(
      fc.property(fc.integer({ min: 0, max: 10 * 365 * 86_400 }), (seconds) => {
        expect(formatCompactRelativeTimestamp(NOW + seconds * SECOND)).toBe('now');
      }),
    );
  });

  it('reads "Never" when there is no time', () => {
    expect(formatCompactRelativeTimestamp(null)).toBe('Never');
    expect(formatCompactRelativeTimestamp(0)).toBe('Never');
  });
});
