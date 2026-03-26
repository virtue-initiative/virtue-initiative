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

  it('serves the same JWKS with and without the configured base path', async () => {
    const [rootRes, prefixedRes] = await Promise.all([
      SELF.fetch(`${BASE}/.well-known/jwks.json`),
      SELF.fetch(`${BASE}/api/.well-known/jwks.json`),
    ]);

    expect(rootRes.status).toBe(200);
    expect(prefixedRes.status).toBe(200);

    const rootJson = (await rootRes.json()) as {
      keys: Array<Record<string, string>>;
    };

    expect(await prefixedRes.json()).toEqual(rootJson);
    expect(rootJson).toMatchObject({
      keys: [
        {
          alg: 'EdDSA',
          crv: 'Ed25519',
          kty: 'OKP',
          use: 'sig',
        },
      ],
    });
    expect(rootJson.keys).toHaveLength(1);
    expect(rootJson.keys[0]?.kid).toBeTruthy();
    expect(rootJson.keys[0]?.x).toBeTruthy();
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
