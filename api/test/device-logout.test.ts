import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { fetchMock, SELF } from 'cloudflare:test';
import { BASE, authHeaders, clearDB, createDeviceForUser, signupAndGetCookie } from './helpers';
import { getHashState, installHashServerMock, seedHashState } from './hash-server-mock';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

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

    // Simulates the client having already uploaded a hash directly to the (mocked)
    // hash server — the API itself never POSTs there, so there's no request to make.
    seedHashState(device.id, { hash: '09'.repeat(32), seq: 1, last_received: 500 });

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

    expect(getHashState(device.id)).toMatchObject({ seq: 0, hash: '0'.repeat(64) });
  });

  it('rejects logout without a valid device session', async () => {
    const res = await SELF.fetch(`${BASE}/d/logout`, {
      method: 'POST',
      headers: { Authorization: 'Bearer not-a-real-token' },
    });
    expect(res.status).toBe(401);
  });
});
