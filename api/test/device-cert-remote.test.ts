import { SELF } from 'cloudflare:test';
import { describe, expect, it } from 'vitest';
import { verifyJWT } from '../src/lib/jwt';
import { TEST_JWT_PUBLIC_KEY } from './jwt-test-keys';
import {
  BASE,
  createDeviceForUser,
  passwordAuthFor,
  signupAndGetCookie,
  validDevicePubkeyBase64,
} from './helpers';

// Run under vitest.config.remote-hash-server.ts, where HASH_SERVER_URL does
// not end in `/api` — buildDeviceState's remote (device-cert) branch is
// therefore always active here. POST /device and GET /device never call the
// remote hash-server themselves (that's only hashGet/hashReset, used by
// POST /batch and POST /logout), so they can be exercised without mocking
// the network.
describe('device-cert pubkey handling (remote mode)', () => {
  describe('POST /d/device', () => {
    it('mints a device-cert token embedding the presented pubkey', async () => {
      const email = `pubkey-register-${crypto.randomUUID()}@example.com`;
      await signupAndGetCookie(email);
      const pubkey = validDevicePubkeyBase64();

      const res = await SELF.fetch(`${BASE}/d/device`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'X-Device-Pubkey': pubkey },
        body: JSON.stringify({
          email,
          password_auth: await passwordAuthFor('password123'),
          name: 'Laptop',
          platform: 'linux',
        }),
      });

      expect(res.status).toBe(201);
      const body = (await res.json()) as { token: string; settings: { id: string } };
      const claims = await verifyJWT(body.token, TEST_JWT_PUBLIC_KEY);
      expect(claims.type).toBe('device-cert');
      expect(claims.sub).toBe(body.settings.id);
      expect(claims.pubkey).toBe(pubkey);
    });

    it('rejects registration missing the X-Device-Pubkey header with 400', async () => {
      const email = `pubkey-missing-${crypto.randomUUID()}@example.com`;
      await signupAndGetCookie(email);

      const res = await SELF.fetch(`${BASE}/d/device`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          email,
          password_auth: await passwordAuthFor('password123'),
          name: 'Laptop',
          platform: 'linux',
        }),
      });

      expect(res.status).toBe(400);
      const body = (await res.json()) as { error: string };
      expect(body.error).toBe('missing X-Device-Pubkey header');
    });
  });

  describe('GET /d/device', () => {
    it('mints a fresh device-cert token embedding the presented pubkey', async () => {
      const email = `pubkey-get-${crypto.randomUUID()}@example.com`;
      await signupAndGetCookie(email);
      const device = await createDeviceForUser(email);
      const pubkey = validDevicePubkeyBase64();

      const res = await SELF.fetch(`${BASE}/d/device`, {
        headers: { Authorization: `Bearer ${device.refresh_token}`, 'X-Device-Pubkey': pubkey },
      });

      expect(res.status).toBe(200);
      const body = (await res.json()) as { token: string };
      const claims = await verifyJWT(body.token, TEST_JWT_PUBLIC_KEY);
      expect(claims.type).toBe('device-cert');
      expect(claims.sub).toBe(device.id);
      expect(claims.pubkey).toBe(pubkey);
    });

    it('rejects a call missing X-Device-Pubkey with 400', async () => {
      const email = `pubkey-get-missing-${crypto.randomUUID()}@example.com`;
      await signupAndGetCookie(email);
      const device = await createDeviceForUser(email);

      const res = await SELF.fetch(`${BASE}/d/device`, {
        headers: { Authorization: `Bearer ${device.refresh_token}` },
      });

      expect(res.status).toBe(400);
      const body = (await res.json()) as { error: string };
      expect(body.error).toBe('missing X-Device-Pubkey header');
    });
  });
});
