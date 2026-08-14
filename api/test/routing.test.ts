import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import { BASE, clearDB } from './helpers';

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
});
