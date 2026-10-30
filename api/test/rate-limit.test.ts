import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import { BASE, clearDB, createDeviceForUser, signupAndGetCookie } from './helpers';

beforeEach(clearDB);

describe('Per-device rate limiting (API-034)', () => {
  it('returns 429 once a device exceeds the configured request rate', async () => {
    await signupAndGetCookie('rate-limited@example.com');
    const device = await createDeviceForUser(
      'rate-limited@example.com',
      'password123',
      'Rate Limited Device',
      'linux',
    );
    const headers = { Authorization: `Bearer ${device.refresh_token}` };

    // wrangler.json's staging RATE_LIMITER is configured for 60 requests/60s.
    //
    // Miniflare's local RATE_LIMITER simulator is a plain fixed-window counter
    // keyed by `Math.floor(Date.now() / periodMs)` against the real wall clock
    // (see node_modules/@cloudflare/vitest-pool-workers's bundled
    // miniflare/dist/src/workers/ratelimit/ratelimit.worker.js): if this burst
    // straddles a 60s boundary, the window flip clears the in-memory bucket
    // mid-burst, and a smaller-than-60 remainder on either side of the split
    // never trips the limit. Firing 130 (> 2x the limit) guarantees at least
    // one side of any single such split still exceeds 60, so the test can't
    // flake on that reset.
    const responses = await Promise.all(
      Array.from({ length: 130 }, () => SELF.fetch(`${BASE}/d/device`, { headers })),
    );

    const statuses = responses.map((res: Response) => res.status);
    expect(statuses).toContain(200);
    expect(statuses).toContain(429);

    const limited = responses.find((res: Response) => res.status === 429)!;
    expect(await limited.json()).toEqual({ error: 'Too many requests' });
  }, 20000); // 130 concurrent requests can outrun the default 10s timeout under CI load

  it('does not rate limit two different devices independently of each other', async () => {
    await signupAndGetCookie('two-devices@example.com');
    const deviceA = await createDeviceForUser(
      'two-devices@example.com',
      'password123',
      'Device A',
      'linux',
    );
    const deviceB = await createDeviceForUser(
      'two-devices@example.com',
      'password123',
      'Device B',
      'macos',
    );

    const resA = await SELF.fetch(`${BASE}/d/device`, {
      headers: { Authorization: `Bearer ${deviceA.refresh_token}` },
    });
    const resB = await SELF.fetch(`${BASE}/d/device`, {
      headers: { Authorization: `Bearer ${deviceB.refresh_token}` },
    });

    expect(resA.status).toBe(200);
    expect(resB.status).toBe(200);
  });
});

describe('Per-IP rate limiting of the account lookup routes (API-012, API-015)', () => {
  it('does not limit GET /user/login-material, which every login calls first (API-015)', async () => {
    const responses = await Promise.all(
      Array.from({ length: 130 }, () =>
        SELF.fetch(`${BASE}/user/login-material?email=nobody%40example.com`, {
          headers: { 'CF-Connecting-IP': 'shared-by-many-logins' },
        }),
      ),
    );
    await Promise.all(responses.map((res: Response) => res.arrayBuffer()));
    expect(responses.map((res: Response) => res.status)).not.toContain(429);
  });

  // Fires 130 for the reason given in the per-device test above.
  it.each([
    [
      'POST /password-reset',
      (ip: string) =>
        SELF.fetch(`${BASE}/password-reset`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json', 'CF-Connecting-IP': ip },
          body: JSON.stringify({ email: 'nobody@example.com' }),
        }),
      204,
    ],
  ] as const)(
    '%s answers 429 once one IP exceeds the rate, without limiting other IPs',
    async (name, send, okStatus) => {
      const ip = `limited-${name}`;
      const responses = await Promise.all(Array.from({ length: 130 }, () => send(ip)));
      const statuses = responses.map((res: Response) => res.status);
      expect(statuses).toContain(okStatus);
      expect(statuses).toContain(429);

      const limited = responses.find((res: Response) => res.status === 429)!;
      expect(await limited.json()).toEqual({ error: 'Too many requests' });

      expect((await send(`other-${name}`)).status).toBe(okStatus);
    },
    20000,
  );
});
