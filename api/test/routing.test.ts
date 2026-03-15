import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import { authHeaders, BASE, clearDB, signupAndGetToken } from './helpers';

beforeEach(clearDB);

describe('API base path routing', () => {
  it('serves the same health payload with and without the configured base path', async () => {
    const [rootRes, prefixedRes] = await Promise.all([
      SELF.fetch(`${BASE}/`),
      SELF.fetch(`${BASE}/api`),
    ]);

    expect(rootRes.status).toBe(200);
    expect(prefixedRes.status).toBe(200);
    expect(await prefixedRes.json()).toEqual(await rootRes.json());
  });

  it('preserves the /api base path in device hash_base_url responses', async () => {
    const { token } = await signupAndGetToken('prefixed-device@example.com', 'pw');

    const createDeviceRes = await SELF.fetch(`${BASE}/api/d/device`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ name: 'Laptop', platform: 'linux' }),
    });

    expect(createDeviceRes.status).toBe(201);

    const createdDevice = (await createDeviceRes.json()) as { access_token: string };
    const settingsRes = await SELF.fetch(`${BASE}/api/d/device`, {
      headers: { Authorization: `Bearer ${createdDevice.access_token}` },
    });

    expect(settingsRes.status).toBe(200);
    expect(await settingsRes.json()).toMatchObject({
      hash_base_url: `${BASE}/api`,
    });
  });
});
