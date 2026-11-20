import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import { isVersionBelow } from '../src/lib/announcements';
import type { Announcement, DeviceAnnouncement } from '../../shared-web/types';
import {
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  markUserAdmin,
  passwordAuthFor,
  signupAndGetCookie,
} from './helpers';

beforeEach(clearDB);

async function adminCookie() {
  const { cookie, userId } = await signupAndGetCookie('admin@example.com');
  await markUserAdmin(userId);
  return cookie;
}

function postAnnouncement(cookie: string, body: unknown) {
  return SELF.fetch(`${BASE}/admin/announcements`, {
    method: 'POST',
    headers: authHeaders(cookie),
    body: JSON.stringify(body),
  });
}

async function createAnnouncement(cookie: string, body: Record<string, unknown>) {
  const res = await postAnnouncement(cookie, { level: 'info', ...body });
  expect(res.status).toBe(200);
  return (await res.json()) as Announcement;
}

function deleteAnnouncement(cookie: string, id: string) {
  return SELF.fetch(`${BASE}/admin/announcements/${id}`, {
    method: 'DELETE',
    headers: authHeaders(cookie),
  });
}

async function deviceAnnouncement(refreshToken: string, version?: string) {
  const res = await SELF.fetch(`${BASE}/d/device`, {
    headers: {
      Authorization: `Bearer ${refreshToken}`,
      ...(version ? { 'X-Virtue-Version': version } : {}),
    },
  });
  expect(res.status).toBe(200);
  return ((await res.json()) as { announcement: DeviceAnnouncement | null }).announcement;
}

describe('version comparison (API-068)', () => {
  it.each([
    ['0.1.9', '0.2.0', true],
    ['0.2.0', '0.2.0', false],
    ['0.2', '0.2.0', false],
    ['0.10.0', '0.9.0', false],
    ['0.1.9-dev', '0.2.0', true],
    ['0.2.0-dev', '0.2.0', false],
    ['1', '0.9.9', false],
    [undefined, '0.0.1', true],
    ['', '0.0.1', true],
    ['dev', '0.0.1', true],
  ])('%s is below %s: %s', (version, bound, below) => {
    expect(isVersionBelow(version, bound)).toBe(below);
  });
});

describe('Announcement admin routes (API-069, API-070, API-071)', () => {
  it('reject non-admins (API-068)', async () => {
    const { cookie } = await signupAndGetCookie('user@example.com');
    const list = await SELF.fetch(`${BASE}/admin/announcements`, { headers: authHeaders(cookie) });
    expect(list.status).toBe(403);
    expect((await postAnnouncement(cookie, { message: 'Hi', level: 'info' })).status).toBe(403);
  });

  it('create and list announcements newest first (API-070, API-069)', async () => {
    const cookie = await adminCookie();
    const first = await createAnnouncement(cookie, { message: '  Old news  ' });
    expect(first).toMatchObject({
      message: 'Old news',
      url: null,
      level: 'info',
      platforms: null,
      below_version: null,
    });
    const second = await createAnnouncement(cookie, {
      message: 'Update by hand',
      url: 'https://virtueinitiative.org/download',
      level: 'urgent',
      platforms: ['windows', 'macos'],
      below_version: '0.2.0',
    });

    const res = await SELF.fetch(`${BASE}/admin/announcements`, { headers: authHeaders(cookie) });
    const list = (await res.json()) as Announcement[];
    expect(list.map((a) => a.id)).toEqual([second.id, first.id]);
    expect(list[0]).toEqual(second);
  });

  it('validate the message, link, platforms and version (API-070)', async () => {
    const cookie = await adminCookie();
    const bad = [
      { message: '   ', level: 'info' },
      { message: 'x'.repeat(281), level: 'info' },
      { message: 'Hi', level: 'loud' },
      { message: 'Hi', level: 'info', url: 'http://example.com' },
      { message: 'Hi', level: 'info', url: 'javascript:alert(1)' },
      { message: 'Hi', level: 'info', platforms: [] },
      { message: 'Hi', level: 'info', platforms: ['beos'] },
      { message: 'Hi', level: 'info', below_version: 'v0.2' },
      { message: 'Hi', level: 'info', below_version: '0.2.' },
    ];
    for (const body of bad) {
      expect((await postAnnouncement(cookie, body)).status, JSON.stringify(body)).toBe(400);
    }
    expect(
      (await postAnnouncement(cookie, { message: 'x'.repeat(280), level: 'info' })).status,
    ).toBe(200);
  });

  it('delete an announcement, and 404 for an unknown one (API-071)', async () => {
    const cookie = await adminCookie();
    const { id } = await createAnnouncement(cookie, { message: 'Hi' });
    expect((await deleteAnnouncement(cookie, id)).status).toBe(204);
    expect((await deleteAnnouncement(cookie, id)).status).toBe(404);
    expect((await deleteAnnouncement(cookie, 'not-a-uuid')).status).toBe(404);
  });
});

describe('Announcements in device settings (API-068)', () => {
  it('send null when there is none', async () => {
    await signupAndGetCookie('owner@example.com');
    const device = await createDeviceForUser('owner@example.com');
    expect(await deviceAnnouncement(device.refresh_token, '0.1.9')).toBeNull();
  });

  it('send the newest announcement that applies to the platform and version', async () => {
    const cookie = await adminCookie();
    const linux = await createDeviceForUser('admin@example.com', undefined, 'Laptop', 'linux');
    const windows = await createDeviceForUser('admin@example.com', undefined, 'PC', 'windows');

    await createAnnouncement(cookie, { message: 'For everyone' });
    await createAnnouncement(cookie, {
      message: 'Update Windows',
      level: 'urgent',
      url: 'https://virtueinitiative.org/download',
      platforms: ['windows'],
      below_version: '0.2.0',
    });

    expect(await deviceAnnouncement(windows.refresh_token, '0.1.9')).toEqual({
      message: 'Update Windows',
      url: 'https://virtueinitiative.org/download',
      level: 'urgent',
    });
    // An old client sends no version, so it counts as below every version.
    expect((await deviceAnnouncement(windows.refresh_token))?.message).toBe('Update Windows');
    // Up to date, or another platform: the older announcement for everyone.
    expect((await deviceAnnouncement(windows.refresh_token, '0.2.0'))?.message).toBe(
      'For everyone',
    );
    expect((await deviceAnnouncement(linux.refresh_token, '0.1.9'))?.message).toBe('For everyone');
  });

  it('stop sending an announcement once it is deleted', async () => {
    const cookie = await adminCookie();
    const device = await createDeviceForUser('admin@example.com');
    const { id } = await createAnnouncement(cookie, { message: 'Hi' });
    expect((await deviceAnnouncement(device.refresh_token, '0.1.9'))?.message).toBe('Hi');
    await deleteAnnouncement(cookie, id);
    expect(await deviceAnnouncement(device.refresh_token, '0.1.9')).toBeNull();
  });

  it('include it when a device registers', async () => {
    const cookie = await adminCookie();
    await createAnnouncement(cookie, { message: 'Hi', level: 'warning' });

    const res = await SELF.fetch(`${BASE}/d/device`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-Virtue-Version': '0.1.9' },
      body: JSON.stringify({
        email: 'admin@example.com',
        password_auth: await passwordAuthFor('password123'),
        name: 'Laptop',
        platform: 'linux',
      }),
    });
    expect(res.status).toBe(200);
    const registered = (await res.json()) as { settings: { announcement: DeviceAnnouncement } };
    expect(registered.settings.announcement).toEqual({
      message: 'Hi',
      url: null,
      level: 'warning',
    });
  });
});
