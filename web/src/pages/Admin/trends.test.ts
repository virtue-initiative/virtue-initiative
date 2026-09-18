import { describe, expect, it } from 'vitest';
import { TEST_ANALYTICS_SNAPSHOT } from '../../mocks/fixtures';
import { formatTrendValue, niceCeil, seriesFor, trendVariables } from './trends';

describe('trendVariables', () => {
  it('adds one variable per platform seen in the snapshots', () => {
    const variables = trendVariables([TEST_ANALYTICS_SNAPSHOT]);
    const platformKeys = variables.filter((v) => v.key.startsWith('devices.by_platform.'));
    expect(platformKeys.map((v) => v.label)).toEqual(['Devices on android', 'Devices on linux']);
    expect(platformKeys[1].read(TEST_ANALYTICS_SNAPSHOT.metrics)).toBe(30);
  });

  it('derives ratios from the stored counts', () => {
    const byKey = new Map(trendVariables([]).map((v) => [v.key, v]));
    expect(byKey.get('derived.devices_per_person')!.read(TEST_ANALYTICS_SNAPSHOT.metrics)).toBe(
      1.5,
    );
    expect(
      byKey.get('derived.active_devices_per_active_user')!.read(TEST_ANALYTICS_SNAPSHOT.metrics),
    ).toBe(2);
  });
});

describe('seriesFor', () => {
  it('orders oldest first and positions by real date', () => {
    const users = trendVariables([]).find((v) => v.key === 'users.total')!;
    const series = seriesFor(users, [
      { ...TEST_ANALYTICS_SNAPSHOT, day: '2026-09-18' },
      {
        ...TEST_ANALYTICS_SNAPSHOT,
        day: '2026-09-15',
        metrics: {
          ...TEST_ANALYTICS_SNAPSHOT.metrics,
          users: { ...TEST_ANALYTICS_SNAPSHOT.metrics.users, total: 40 },
        },
      },
    ]);
    expect(series.map((p) => [p.day, p.value])).toEqual([
      ['2026-09-15', 40],
      ['2026-09-18', 42],
    ]);
    expect(series[1].t - series[0].t).toBe(3 * 24 * 60 * 60 * 1000);
  });
});

describe('niceCeil', () => {
  it('rounds up to 1, 2, or 5 times a power of ten', () => {
    expect(niceCeil(0)).toBe(1);
    expect(niceCeil(1)).toBe(1);
    expect(niceCeil(3)).toBe(5);
    expect(niceCeil(7)).toBe(10);
    expect(niceCeil(42)).toBe(50);
    expect(niceCeil(120)).toBe(200);
    expect(niceCeil(9001)).toBe(10000);
    expect(niceCeil(1.5)).toBe(2);
  });
});

describe('formatTrendValue', () => {
  it('keeps integers whole and ratios to two decimals', () => {
    expect(formatTrendValue(9001)).toBe('9,001');
    expect(formatTrendValue(1.5)).toBe('1.50');
  });
});
