import type {
  AnalyticsSnapshot,
  Device,
  LockedPassword,
  User,
  WatcherPartner,
  WatchingPartner,
} from '../utils/api/api';

export const TEST_USER: User = {
  id: 'user-1',
  email: 'test@example.com',
  email_verified: true,
  email_bounced_at: null,
  name: 'Test User',
  pub_key: undefined,
  encrypted_priv_key: undefined,
  settings: {
    email_frequency: 'daily',
    timezone: 'UTC',
  },
};

export const TEST_DEVICES: Device[] = [
  {
    id: 'device-1',
    owner: 'user-1',
    name: 'My Laptop',
    platform: 'linux',
    status: 'offline',
    last_upload_at: Date.now() - 60_000,
    last_hash_at: Date.now() - 30_000,
    pending_count: 0,
  },
  {
    id: 'device-2',
    owner: 'user-1',
    name: 'My Phone',
    platform: 'android',
    status: 'online',
    last_upload_at: Date.now() - 5_000,
    last_hash_at: Date.now() - 2_000,
    pending_count: 0,
  },
];

export const TEST_WATCHER: WatcherPartner = {
  id: 'watcher-1',
  user: { id: 'watcher-user-1', name: 'Watcher Alice', email: 'alice@example.com' },
  status: 'accepted',
};

export const TEST_WATCHING: WatchingPartner = {
  id: 'watching-1',
  user: { id: 'watching-user-1', name: 'Bob', email: 'bob@example.com' },
  status: 'accepted',
  created_at: Date.now() - 86_400_000,
};

export const TEST_LOCKED_PASSWORD: LockedPassword = {
  id: 'password-1',
  label: 'Screen Time passcode',
  created_at: Date.now() - 3_600_000,
  accessed_at: null,
  deleted_at: null,
};

export const TEST_ANALYTICS_SNAPSHOT: AnalyticsSnapshot = {
  day: '2026-09-18',
  created_at: Date.now() - 3_600_000,
  metrics: {
    users: { total: 42, verified: 40, new_1d: 1, new_7d: 5, new_30d: 12 },
    active_users: { d1: 9, d7: 17, d30: 25 },
    active_devices: { d1: 12, d7: 34, d30: 40 },
    devices: {
      total: 63,
      owners: 42,
      by_platform: { linux: 30, android: 33 },
    },
    batches: { total: 9001, d1: 120, d7: 800 },
    partners: { total: 30, accepted: 24, pending: 6, watched_users: 20 },
    locked_passwords: { total: 7 },
  },
};
