import type { Device, User, WatcherPartner, WatchingPartner } from '../utils/api/api';

export const TEST_USER: User = {
  id: 'user-1',
  email: 'test@example.com',
  email_verified: true,
  email_bounced_at: null,
  name: 'Test User',
  pub_key: undefined,
  priv_key: undefined,
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
  },
  {
    id: 'device-2',
    owner: 'user-1',
    name: 'My Phone',
    platform: 'android',
    status: 'online',
    last_upload_at: Date.now() - 5_000,
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
  digest_cadence: 'daily',
  created_at: Date.now() - 86_400_000,
};
