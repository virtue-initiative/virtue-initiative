import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import {
  BASE,
  authHeaders,
  clearDB,
  createDeviceForUser,
  createServerToken,
  signupAndGetCookie,
} from './helpers';

beforeEach(clearDB);

describe('POST /d/logout', () => {
  it('revokes the device session, soft-deletes the device, and resets its hash state', async () => {
    const { cookie } = await signupAndGetCookie('logout@example.com');
    const device = await createDeviceForUser(
      'logout@example.com',
      'password123',
      'Laptop',
      'linux',
    );

    const hashUploadRes = await SELF.fetch(`${BASE}/hash`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.token}` },
      body: new Uint8Array(32).fill(9),
    });
    expect(hashUploadRes.status).toBe(200);

    const logoutRes = await SELF.fetch(`${BASE}/d/logout`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.refresh_token}` },
    });
    expect(logoutRes.status).toBe(204);

    const staleSessionRes = await SELF.fetch(`${BASE}/d/device`, {
      headers: { Authorization: `Bearer ${device.refresh_token}` },
    });
    expect(staleSessionRes.status).toBe(401);

    const listRes = await SELF.fetch(`${BASE}/device`, {
      headers: authHeaders(cookie),
    });
    expect(listRes.status).toBe(200);
    const list = (await listRes.json()) as Array<{ id: string; status: string }>;
    expect(list.find((item) => item.id === device.id)).toMatchObject({
      status: 'logged_out',
    });

    const serverToken = await createServerToken(device.id);
    const infoRes = await SELF.fetch(`${BASE}/hash/info`, {
      headers: { Authorization: `Bearer ${serverToken}` },
    });
    expect(infoRes.status).toBe(200);
    const info = (await infoRes.json()) as { count: number };
    expect(info.count).toBe(0);
  });

  it('rejects logout without a valid device session', async () => {
    const res = await SELF.fetch(`${BASE}/d/logout`, {
      method: 'POST',
      headers: { Authorization: 'Bearer not-a-real-token' },
    });
    expect(res.status).toBe(401);
  });
});
