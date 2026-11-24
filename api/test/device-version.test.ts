import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { env, fetchMock, SELF } from 'cloudflare:test';
import {
  BASE,
  batchMetadataForm,
  clearDB,
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

function uploadBatch(
  device: { id: string; refresh_token: string },
  ownerId: string,
  version?: string,
) {
  seedHashState(device.id, { hash: '5a'.repeat(32), seq: 3, last_received: 1_700_000_000 });
  return SELF.fetch(`${BASE}/d/batch`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${device.refresh_token}`,
      ...(version !== undefined ? { 'X-Virtue-Version': version } : {}),
    },
    body: batchMetadataForm({
      start_time: 1_000,
      end_time: 2_000,
      access_keys: { [ownerId]: btoa('owner-envelope') },
      file: new File([new Uint8Array([1, 2, 3])], 'batch.enc'),
    }),
  });
}

// Counts writes to devices.app_version while `run` runs.
async function countVersionWrites(run: () => Promise<void>) {
  await env.DB.batch([
    env.DB.prepare('CREATE TABLE version_writes (n INTEGER)'),
    env.DB.prepare(
      `CREATE TRIGGER count_version_writes AFTER UPDATE OF app_version ON devices
       BEGIN INSERT INTO version_writes VALUES (1); END`,
    ),
  ]);
  try {
    await run();
    const row = await env.DB.prepare('SELECT COUNT(*) AS n FROM version_writes').first<{
      n: number;
    }>();
    return row!.n;
  } finally {
    await env.DB.batch([
      env.DB.prepare('DROP TRIGGER count_version_writes'),
      env.DB.prepare('DROP TABLE version_writes'),
    ]);
  }
}

// Makes every write to devices.app_version fail while `run` runs.
async function withFailingVersionWrites(run: () => Promise<void>) {
  await env.DB.prepare(
    `CREATE TRIGGER fail_version_writes BEFORE UPDATE OF app_version ON devices
     BEGIN SELECT RAISE(ABORT, 'version written'); END`,
  ).run();
  try {
    await run();
  } finally {
    await env.DB.prepare('DROP TRIGGER fail_version_writes').run();
  }
}

describe('POST /d/device (API-035)', () => {
  it('stores the version the client sends', async () => {
    await signupAndGetCookie('owner@example.com');
    const device = await register('owner@example.com', '0.1.9');
    expect(await storedVersion(device.id)).toBe('0.1.9');
  });

  it.each([
    ['missing', undefined],
    ['not a version', '0.1.9; DROP TABLE'],
    ['too long', '1'.repeat(33)],
    ['without a leading number', 'dev'],
    ['only a sign', '+'],
  ])('stores none when the header is %s', async (_, version) => {
    await signupAndGetCookie('owner@example.com');
    const device = await register('owner@example.com', version);
    expect(await storedVersion(device.id)).toBeNull();
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
    expect((await fetchSettings(device.refresh_token, 'dev')).status).toBe(200);
    expect(await storedVersion(device.id)).toBe('0.1.9');
  });

  it('writes the device only when the version changed', async () => {
    await signupAndGetCookie('owner@example.com');
    const device = await register('owner@example.com', '0.1.9');

    const unchanged = await countVersionWrites(async () => {
      expect((await fetchSettings(device.refresh_token, '0.1.9')).status).toBe(200);
    });
    expect(unchanged).toBe(0);

    const changed = await countVersionWrites(async () => {
      expect((await fetchSettings(device.refresh_token, '0.2.0')).status).toBe(200);
    });
    expect(changed).toBe(1);
  });

  it('still returns the settings when storing the version fails', async () => {
    await signupAndGetCookie('owner@example.com');
    const device = await register('owner@example.com', '0.1.9');

    await withFailingVersionWrites(async () => {
      expect((await fetchSettings(device.refresh_token, '0.2.0')).status).toBe(200);
    });
    expect(await storedVersion(device.id)).toBe('0.1.9');
  });
});

describe('POST /d/batch (API-036)', () => {
  it('stores a changed version, for a client that started offline', async () => {
    const { userId } = await signupAndGetCookie('owner@example.com');
    const device = await register('owner@example.com', '0.1.9');

    expect((await uploadBatch(device, userId, '0.2.0')).status).toBe(200);
    expect(await storedVersion(device.id)).toBe('0.2.0');
  });

  it('keeps the stored version when the header is missing or invalid', async () => {
    const { userId } = await signupAndGetCookie('owner@example.com');
    const device = await register('owner@example.com', '0.1.9');

    expect((await uploadBatch(device, userId)).status).toBe(200);
    expect((await uploadBatch(device, userId, 'dev')).status).toBe(200);
    expect(await storedVersion(device.id)).toBe('0.1.9');
  });

  it('writes the device only when the version changed', async () => {
    const { userId } = await signupAndGetCookie('owner@example.com');
    const device = await register('owner@example.com', '0.1.9');

    const writes = await countVersionWrites(async () => {
      expect((await uploadBatch(device, userId, '0.1.9')).status).toBe(200);
    });
    expect(writes).toBe(0);
  });

  it('still stores the batch when storing the version fails', async () => {
    const { userId } = await signupAndGetCookie('owner@example.com');
    const device = await register('owner@example.com', '0.1.9');

    await withFailingVersionWrites(async () => {
      expect((await uploadBatch(device, userId, '0.2.0')).status).toBe(200);
    });
    expect(await storedVersion(device.id)).toBe('0.1.9');
    const batches = await env.DB.prepare('SELECT COUNT(*) AS n FROM batches WHERE device_id = ?')
      .bind(uuidToBytes(device.id))
      .first<{ n: number }>();
    expect(batches!.n).toBe(1);
  });
});
