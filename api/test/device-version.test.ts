import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { env, fetchMock, SELF } from 'cloudflare:test';
import {
  BASE,
  batchMetadataForm,
  clearDB,
  createDeviceForUser,
  passwordAuthFor,
  signupAndGetCookie,
  uuidToBytes,
} from './helpers';
import { installHashServerMock, seedHashState } from './hash-server-mock';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

beforeEach(clearDB);

async function storedVersion(deviceId: string) {
  const row = await env.DB.prepare('SELECT app_version FROM devices WHERE id = ?')
    .bind(uuidToBytes(deviceId))
    .first<{ app_version: string | null }>();
  return row?.app_version;
}

async function register(email: string, version?: string) {
  const res = await SELF.fetch(`${BASE}/d/device`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(version !== undefined ? { 'X-Virtue-Version': version } : {}),
    },
    body: JSON.stringify({
      email,
      password_auth: await passwordAuthFor('password123'),
      name: 'Laptop',
      platform: 'linux',
    }),
  });
  expect(res.status).toBe(200);
  const body = (await res.json()) as { token: string; settings: { id: string } };
  return { id: body.settings.id, refresh_token: body.token };
}

function fetchSettings(refreshToken: string, version?: string) {
  return SELF.fetch(`${BASE}/d/device`, {
    headers: {
      Authorization: `Bearer ${refreshToken}`,
      ...(version !== undefined ? { 'X-Virtue-Version': version } : {}),
    },
  });
}

describe('POST /d/device (API-035)', () => {
  it('stores the version the client sends', async () => {
    await signupAndGetCookie('owner@example.com');
    const device = await register('owner@example.com', '0.1.9');
    expect(await storedVersion(device.id)).toBe('0.1.9');
  });

  it('stores none when the header is missing or invalid', async () => {
    await signupAndGetCookie('owner@example.com');
    const missing = await register('owner@example.com');
    expect(await storedVersion(missing.id)).toBeNull();
    const invalid = await register('owner@example.com', '0.1.9; DROP TABLE');
    expect(await storedVersion(invalid.id)).toBeNull();
    const tooLong = await register('owner@example.com', '1'.repeat(33));
    expect(await storedVersion(tooLong.id)).toBeNull();
  });
});

describe('GET /d/device (API-036)', () => {
  it('stores a changed version, such as after an update', async () => {
    await signupAndGetCookie('owner@example.com');
    const device = await register('owner@example.com', '0.1.9');

    expect((await fetchSettings(device.refresh_token, '0.2.0-dev')).status).toBe(200);
    expect(await storedVersion(device.id)).toBe('0.2.0-dev');
  });

  it('keeps the stored version when the header is missing or invalid', async () => {
    await signupAndGetCookie('owner@example.com');
    const device = await register('owner@example.com', '0.1.9');

    expect((await fetchSettings(device.refresh_token)).status).toBe(200);
    expect((await fetchSettings(device.refresh_token, 'not a version!')).status).toBe(200);
    expect(await storedVersion(device.id)).toBe('0.1.9');
  });

  it('does not write the device when the version is unchanged', async () => {
    await signupAndGetCookie('owner@example.com');
    const device = await register('owner@example.com', '0.1.9');
    // Any write to devices now fails the request.
    await env.DB.prepare(
      `CREATE TRIGGER no_device_writes BEFORE UPDATE ON devices
       BEGIN SELECT RAISE(ABORT, 'device written'); END`,
    ).run();
    try {
      expect((await fetchSettings(device.refresh_token, '0.1.9')).status).toBe(200);
      expect((await fetchSettings(device.refresh_token, '0.2.0')).status).toBe(500);
    } finally {
      await env.DB.prepare('DROP TRIGGER no_device_writes').run();
    }
  });
});

describe('POST /d/batch (API-036)', () => {
  it('does not change the stored version', async () => {
    const { userId } = await signupAndGetCookie('owner@example.com');
    const device = await createDeviceForUser('owner@example.com');
    seedHashState(device.id, { hash: '5a'.repeat(32), seq: 3, last_received: 1_700_000_000 });
    expect(await storedVersion(device.id)).toBeNull();

    const res = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.refresh_token}`, 'X-Virtue-Version': '0.2.0' },
      body: batchMetadataForm({
        start_time: 1_000,
        end_time: 2_000,
        access_keys: { [userId]: btoa('owner-envelope') },
        file: new File([new Uint8Array([1, 2, 3])], 'batch.enc'),
      }),
    });
    expect(res.status).toBe(200);
    expect(await storedVersion(device.id)).toBeNull();
  });
});
