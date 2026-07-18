import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import { BASE, clearDB, createDeviceForUser, signupAndGetCookie } from './helpers';

beforeEach(clearDB);

describe('Per-device rate limiting', () => {
  it('returns 429 once a device exceeds the configured request rate', async () => {
    const { cookie } = await signupAndGetCookie('rate-limited@example.com');
    const device = await createDeviceForUser(cookie, 'Rate Limited Device', 'linux');
    const headers = { Authorization: `Bearer ${device.refresh_token}` };

    // wrangler.json's staging RATE_LIMITER is configured for 60 requests/60s.
    const responses = await Promise.all(
      Array.from({ length: 65 }, () => SELF.fetch(`${BASE}/d/device`, { headers })),
    );

    const statuses = responses.map((res: Response) => res.status);
    expect(statuses).toContain(200);
    expect(statuses).toContain(429);

    const limited = responses.find((res: Response) => res.status === 429)!;
    expect(await limited.json()).toEqual({ error: 'Too many requests' });
  });

  it('does not rate limit two different devices independently of each other', async () => {
    const { cookie } = await signupAndGetCookie('two-devices@example.com');
    const deviceA = await createDeviceForUser(cookie, 'Device A', 'linux');
    const deviceB = await createDeviceForUser(cookie, 'Device B', 'macos');

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
