import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import { authHeaders, BASE, clearDB, createDeviceForUser, signupAndGetToken } from './helpers';

beforeEach(clearDB);

describe('Main device routes', () => {
  it('lists devices owned by the authenticated user', async () => {
    const { token } = await signupAndGetToken('alice@example.com');
    await createDeviceForUser(token, 'Work Laptop', 'linux');

    const res = await SELF.fetch(`${BASE}/device`, {
      headers: { Authorization: `Bearer ${token}` },
    });

    expect(res.status).toBe(200);
    const body = (await res.json()) as Array<{ name: string; platform: string; enabled: boolean }>;
    expect(body).toHaveLength(1);
    expect(body[0]).toMatchObject({ name: 'Work Laptop', platform: 'linux', enabled: true });
  });

  it('updates an owned device', async () => {
    const { token } = await signupAndGetToken('bob@example.com');
    const device = await createDeviceForUser(token, 'Old Name', 'macos');

    const res = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'PATCH',
      headers: authHeaders(token),
      body: JSON.stringify({ name: 'New Name', enabled: false }),
    });

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ id: device.id, updated: true });

    const listRes = await SELF.fetch(`${BASE}/device`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    const list = (await listRes.json()) as Array<{ id: string; name: string; enabled: boolean }>;
    expect(list.find((item) => item.id === device.id)).toMatchObject({
      name: 'New Name',
      enabled: false,
    });
  });

  it('forbids patching a device owned by another user', async () => {
    const { token: ownerToken } = await signupAndGetToken('owner@example.com');
    const { token: attackerToken } = await signupAndGetToken('attacker@example.com');
    const device = await createDeviceForUser(ownerToken);

    const res = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'PATCH',
      headers: authHeaders(attackerToken),
      body: JSON.stringify({ name: 'nope' }),
    });

    expect(res.status).toBe(404);
  });
});
