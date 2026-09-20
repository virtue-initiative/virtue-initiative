import type { AnalyticsMetrics, AnalyticsSnapshot } from '../../utils/api/api';
import { perPersonAverage } from '@virtueinitiative/shared-web/types';

export type TrendVariable = {
  key: string;
  label: string;
  group: string;
  read: (metrics: AnalyticsMetrics) => number;
};

const fixed: TrendVariable[] = [
  { key: 'users.total', label: 'Users', group: 'Users', read: (m) => m.users.total },
  { key: 'users.verified', label: 'Verified users', group: 'Users', read: (m) => m.users.verified },
  { key: 'users.new_1d', label: 'New users, 1 day', group: 'Users', read: (m) => m.users.new_1d },
  { key: 'users.new_7d', label: 'New users, 7 days', group: 'Users', read: (m) => m.users.new_7d },
  {
    key: 'users.new_30d',
    label: 'New users, 30 days',
    group: 'Users',
    read: (m) => m.users.new_30d,
  },
  {
    key: 'active_users.d1',
    label: 'Active users, 1 day',
    group: 'Activity',
    read: (m) => m.active_users.d1,
  },
  {
    key: 'active_users.d7',
    label: 'Active users, 7 days',
    group: 'Activity',
    read: (m) => m.active_users.d7,
  },
  {
    key: 'active_users.d30',
    label: 'Active users, 30 days',
    group: 'Activity',
    read: (m) => m.active_users.d30,
  },
  {
    key: 'active_devices.d1',
    label: 'Active devices, 1 day',
    group: 'Activity',
    read: (m) => m.active_devices.d1,
  },
  {
    key: 'active_devices.d7',
    label: 'Active devices, 7 days',
    group: 'Activity',
    read: (m) => m.active_devices.d7,
  },
  {
    key: 'active_devices.d30',
    label: 'Active devices, 30 days',
    group: 'Activity',
    read: (m) => m.active_devices.d30,
  },
  {
    key: 'derived.active_devices_per_active_user',
    label: 'Active devices per active user, 7 days',
    group: 'Activity',
    read: (m) => perPersonAverage(m.active_devices.d7, m.active_users.d7),
  },
  { key: 'devices.total', label: 'Devices', group: 'Devices', read: (m) => m.devices.total },
  {
    key: 'devices.owners',
    label: 'Device owners',
    group: 'Devices',
    read: (m) => m.devices.owners,
  },
  {
    key: 'derived.devices_per_person',
    label: 'Devices per person',
    group: 'Devices',
    read: (m) => perPersonAverage(m.devices.total, m.devices.owners),
  },
  { key: 'batches.total', label: 'Batches', group: 'Batches', read: (m) => m.batches.total },
  { key: 'batches.d1', label: 'Batches, 1 day', group: 'Batches', read: (m) => m.batches.d1 },
  { key: 'batches.d7', label: 'Batches, 7 days', group: 'Batches', read: (m) => m.batches.d7 },
  {
    key: 'partners.total',
    label: 'Partnerships',
    group: 'Partners',
    read: (m) => m.partners.total,
  },
  {
    key: 'partners.accepted',
    label: 'Accepted partnerships',
    group: 'Partners',
    read: (m) => m.partners.accepted,
  },
  {
    key: 'partners.pending',
    label: 'Pending partnerships',
    group: 'Partners',
    read: (m) => m.partners.pending,
  },
  {
    key: 'partners.watched_users',
    label: 'People with a partner',
    group: 'Partners',
    read: (m) => m.partners.watched_users,
  },
  {
    key: 'derived.partners_per_person',
    label: 'Partners per person',
    group: 'Partners',
    read: (m) => perPersonAverage(m.partners.accepted, m.partners.watched_users),
  },
  {
    key: 'locked_passwords.total',
    label: 'Locked passwords',
    group: 'Locked passwords',
    read: (m) => m.locked_passwords.total,
  },
];

// Every fixed variable plus one per platform seen in any snapshot, so a new
// platform appears without a code change.
export function trendVariables(snapshots: AnalyticsSnapshot[]): TrendVariable[] {
  const platforms = new Set<string>();
  for (const snapshot of snapshots) {
    for (const platform of Object.keys(snapshot.metrics.devices.by_platform)) {
      platforms.add(platform);
    }
  }
  const byPlatform = [...platforms].sort().map<TrendVariable>((platform) => ({
    key: `devices.by_platform.${platform}`,
    label: `Devices on ${platform}`,
    group: 'Devices',
    read: (m) => m.devices.by_platform[platform] ?? 0,
  }));
  return [...fixed, ...byPlatform];
}

export type TrendPoint = { day: string; t: number; value: number };

export function dayToTime(day: string) {
  return Date.parse(`${day}T00:00:00Z`);
}

// Oldest first, positioned by real date so missing days leave a gap rather
// than compressing time.
export function seriesFor(variable: TrendVariable, snapshots: AnalyticsSnapshot[]): TrendPoint[] {
  return snapshots
    .map((snapshot) => ({
      day: snapshot.day,
      t: dayToTime(snapshot.day),
      value: variable.read(snapshot.metrics),
    }))
    .sort((a, b) => a.t - b.t);
}

// Rounds a maximum up to 1, 2, or 5 times a power of ten so axis ticks land
// on clean numbers.
export function niceCeil(value: number) {
  if (value <= 0) return 1;
  const magnitude = 10 ** Math.floor(Math.log10(value));
  for (const step of [1, 2, 5, 10]) {
    if (value <= step * magnitude) return step * magnitude;
  }
  return 10 * magnitude;
}

export function formatTrendValue(value: number) {
  return Number.isInteger(value) ? value.toLocaleString() : value.toFixed(2);
}

const shortDate = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
  timeZone: 'UTC',
});

export function formatTrendDay(day: string) {
  return shortDate.format(new Date(dayToTime(day)));
}
