import { describe, expect, it } from 'vitest';
import {
  formatDate,
  formatDayHeading,
  formatRelativeTimestamp,
  formatTime,
  localDateKey,
} from './time';

const FIXED_TS = new Date('2024-03-15T14:30:45.000Z').getTime();

describe('formatRelativeTimestamp', () => {
  it('returns Never for null', () => {
    expect(formatRelativeTimestamp(null)).toBe('Never');
  });

  it('returns Never for 0', () => {
    expect(formatRelativeTimestamp(0)).toBe('Never');
  });

  it('returns a relative string for a valid timestamp', () => {
    const result = formatRelativeTimestamp(Date.now() - 60_000);
    expect(typeof result).toBe('string');
    expect(result.length).toBeGreaterThan(0);
  });
});

describe('formatDate', () => {
  it('returns a non-empty string', () => {
    const result = formatDate(FIXED_TS);
    expect(typeof result).toBe('string');
    expect(result.length).toBeGreaterThan(0);
  });

  it('includes year, month, and day parts', () => {
    // Intl.DateTimeFormat with numeric year/month/day produces something like "3/15/2024"
    const result = formatDate(FIXED_TS);
    expect(result).toMatch(/2024/);
  });
});

describe('formatTime', () => {
  it('returns a non-empty string', () => {
    const result = formatTime(FIXED_TS);
    expect(typeof result).toBe('string');
    expect(result.length).toBeGreaterThan(0);
  });
});

describe('localDateKey', () => {
  it('formats as YYYY-MM-DD in local time', () => {
    // Use a UTC midnight to get a predictable local date key on any TZ
    const ts = new Date('2024-06-01T00:00:00').getTime();
    const result = localDateKey(ts);
    expect(result).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(result).toContain('2024');
  });

  it('pads month and day with zeros', () => {
    const ts = new Date('2024-01-05T12:00:00').getTime();
    const result = localDateKey(ts);
    expect(result).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});

describe('formatDayHeading', () => {
  it('returns "Today" for a timestamp from today', () => {
    const startOfToday = new Date();
    startOfToday.setHours(12, 0, 0, 0);
    expect(formatDayHeading(startOfToday.getTime())).toBe('Today');
  });

  it('returns "Yesterday" for a timestamp from yesterday', () => {
    const yesterday = new Date();
    yesterday.setDate(yesterday.getDate() - 1);
    yesterday.setHours(12, 0, 0, 0);
    expect(formatDayHeading(yesterday.getTime())).toBe('Yesterday');
  });

  it('returns a formatted date string for older timestamps', () => {
    const old = new Date('2020-01-01T12:00:00').getTime();
    const result = formatDayHeading(old);
    expect(typeof result).toBe('string');
    expect(result).not.toBe('Today');
    expect(result).not.toBe('Yesterday');
    expect(result).toMatch(/2020/);
  });
});
