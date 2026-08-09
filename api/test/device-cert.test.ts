import { SELF } from 'cloudflare:test';
import { describe, expect, it } from 'vitest';
import { verifyJWT } from '../src/lib/jwt';
import { TEST_JWT_PUBLIC_KEY } from './jwt-test-keys';
import { BASE, passwordAuthFor, signupAndGetCookie, validDevicePubkeyBase64 } from './helpers';

// Runs under the default (local-hash-server) test env — see
// device-cert-remote.test.ts for the device-cert/remote-mode branch, which
// needs HASH_SERVER_URL baked into a separate miniflare binding set rather
// than mutated at runtime (mutating the imported `env` object from
// cloudflare:test does not affect the bindings SELF.fetch's worker sees).

describe('device-cert pubkey handling (local mode)', () => {
  it('mints an unsigned hash-server token and ignores any pubkey header', async () => {
    const email = `pubkey-local-${crypto.randomUUID()}@example.com`;
    await signupAndGetCookie(email);

    const res = await SELF.fetch(`${BASE}/d/device`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-Device-Pubkey': validDevicePubkeyBase64() },
      body: JSON.stringify({
        email,
        password_auth: await passwordAuthFor('password123'),
        name: 'Laptop',
        platform: 'linux',
      }),
    });

    expect(res.status).toBe(201);
    const body = (await res.json()) as { token: string };
    const claims = await verifyJWT(body.token, TEST_JWT_PUBLIC_KEY);
    expect(claims.type).toBe('hash-server');
    expect(claims.pubkey).toBeUndefined();
  });

  it('rejects a malformed X-Device-Pubkey header (wrong byte length) with 400, even in local mode', async () => {
    const email = `pubkey-malformed-${crypto.randomUUID()}@example.com`;
    await signupAndGetCookie(email);

    const res = await SELF.fetch(`${BASE}/d/device`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Device-Pubkey': Buffer.from('too-short').toString('base64'),
      },
      body: JSON.stringify({
        email,
        password_auth: await passwordAuthFor('password123'),
        name: 'Laptop',
        platform: 'linux',
      }),
    });

    expect(res.status).toBe(400);
  });
});
