import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { env, fetchMock, SELF } from 'cloudflare:test';
import { authHeaders, BASE, clearDB, signupAndGetCookie } from './helpers';
import { installHashServerMock } from './hash-server-mock';
import {
  ALPHABET,
  formatUserCode,
  generateUserCode,
  normalizeUserCode,
} from '../src/lib/device-codes';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

beforeEach(clearDB);

type StartResponse = {
  user_code: string;
  device_code: string;
  expires_at: number;
  interval: number;
};

async function startPairing(name = 'Work Laptop', platform = 'linux') {
  const res = await SELF.fetch(`${BASE}/d/device-code`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, platform }),
  });

  expect(res.status).toBe(200);
  return (await res.json()) as StartResponse;
}

function poll(deviceCode: string) {
  return SELF.fetch(`${BASE}/d/device-code/poll`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ device_code: deviceCode }),
  });
}

function lookup(cookie: string, userCode: string) {
  return SELF.fetch(`${BASE}/device-code/lookup`, {
    method: 'POST',
    headers: authHeaders(cookie),
    body: JSON.stringify({ user_code: userCode }),
  });
}

function approve(cookie: string, userCode: string) {
  return SELF.fetch(`${BASE}/device-code/approve`, {
    method: 'POST',
    headers: authHeaders(cookie),
    body: JSON.stringify({ user_code: userCode }),
  });
}

/** Rewinds a pairing's expiry so the expired branches can be exercised. */
async function expirePairing(userCode: string) {
  await env.DB.prepare('UPDATE device_auth_codes SET expires_at = ? WHERE user_code = ?')
    .bind(Date.now() - 1000, normalizeUserCode(userCode))
    .run();
}

describe('Device pairing codes', () => {
  it('signs a device in end to end: start, lookup, approve, poll', async () => {
    const { cookie, userId } = await signupAndGetCookie('alice@example.com');
    const start = await startPairing('Work Laptop', 'linux');

    expect(start.user_code).toMatch(
      /^[23456789ABCDEFGHJKMNPQRSTVWXYZ]{3}-[23456789ABCDEFGHJKMNPQRSTVWXYZ]{3}$/,
    );
    expect(start.device_code.startsWith('dpc_')).toBe(true);
    expect(start.interval).toBe(5);
    expect(start.expires_at).toBeGreaterThan(Date.now());

    const lookupRes = await lookup(cookie, start.user_code);
    expect(lookupRes.status).toBe(200);
    expect(await lookupRes.json()).toMatchObject({ name: 'Work Laptop', platform: 'linux' });

    const approveRes = await approve(cookie, start.user_code);
    expect(approveRes.status).toBe(200);
    expect(await approveRes.json()).toEqual({ name: 'Work Laptop', platform: 'linux' });

    const pollRes = await poll(start.device_code);
    expect(pollRes.status).toBe(200);
    const body = (await pollRes.json()) as {
      token: string;
      account_email: string;
      settings: { id: string; name: string; platform: string; hash_token: string };
    };
    expect(body.account_email).toBe('alice@example.com');
    expect(body.token.startsWith('dst_')).toBe(true);
    expect(body.settings).toMatchObject({ name: 'Work Laptop', platform: 'linux' });
    expect(body.settings.hash_token).toBeTruthy();

    // The token is a genuine device session: it authenticates GET /d/device.
    const settingsRes = await SELF.fetch(`${BASE}/d/device`, {
      headers: { Authorization: `Bearer ${body.token}` },
    });
    expect(settingsRes.status).toBe(200);

    const listRes = await SELF.fetch(`${BASE}/device`, { headers: authHeaders(cookie) });
    const devices = (await listRes.json()) as Array<{ id: string; owner: string; name: string }>;
    expect(devices).toHaveLength(1);
    expect(devices[0]).toMatchObject({ id: body.settings.id, owner: userId, name: 'Work Laptop' });
  });

  it('returns 202 while the pairing is still waiting for approval', async () => {
    const start = await startPairing();

    const res = await poll(start.device_code);
    expect(res.status).toBe(202);
    expect(await res.json()).toEqual({ status: 'pending' });
  });

  it('creates the device only once: a second poll is 410', async () => {
    const { cookie } = await signupAndGetCookie('alice@example.com');
    const start = await startPairing();
    await approve(cookie, start.user_code);

    expect((await poll(start.device_code)).status).toBe(200);

    const second = await poll(start.device_code);
    expect(second.status).toBe(410);
    expect(await second.json()).toEqual({ error: 'expired' });

    const listRes = await SELF.fetch(`${BASE}/device`, { headers: authHeaders(cookie) });
    expect((await listRes.json()) as unknown[]).toHaveLength(1);
  });

  it('rejects an unknown device code with 410', async () => {
    const res = await poll('dpc_nope');
    expect(res.status).toBe(410);
  });

  it('expires a pairing on both ends after its TTL', async () => {
    const { cookie } = await signupAndGetCookie('alice@example.com');
    const start = await startPairing();
    await expirePairing(start.user_code);

    const pollRes = await poll(start.device_code);
    expect(pollRes.status).toBe(410);

    const lookupRes = await lookup(cookie, start.user_code);
    expect(lookupRes.status).toBe(404);
    expect(await lookupRes.json()).toEqual({
      error: 'That code is not valid. It may have expired.',
    });

    const approveRes = await approve(cookie, start.user_code);
    expect(approveRes.status).toBe(404);
  });

  it('rejects a second approval with 409', async () => {
    const { cookie } = await signupAndGetCookie('alice@example.com');
    const other = await signupAndGetCookie('mallory@example.com');
    const start = await startPairing();

    expect((await approve(cookie, start.user_code)).status).toBe(200);

    const again = await approve(cookie, start.user_code);
    expect(again.status).toBe(409);

    const stolen = await approve(other.cookie, start.user_code);
    expect(stolen.status).toBe(409);

    // The device still lands on the account that approved it first.
    const body = (await (await poll(start.device_code)).json()) as { account_email: string };
    expect(body.account_email).toBe('alice@example.com');
  });

  it('hides an approved pairing from lookup', async () => {
    const { cookie } = await signupAndGetCookie('alice@example.com');
    const start = await startPairing();
    await approve(cookie, start.user_code);

    const res = await lookup(cookie, start.user_code);
    expect(res.status).toBe(404);
  });

  it('gives the same generic error for a code that never existed', async () => {
    const { cookie } = await signupAndGetCookie('alice@example.com');

    const res = await lookup(cookie, 'ZZZ-ZZZ');
    expect(res.status).toBe(404);
    expect(await res.json()).toEqual({ error: 'That code is not valid. It may have expired.' });
  });

  it('requires a web session for lookup and approve', async () => {
    const start = await startPairing();

    for (const path of ['lookup', 'approve']) {
      const res = await SELF.fetch(`${BASE}/device-code/${path}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ user_code: start.user_code }),
      });
      expect(res.status).toBe(401);
    }
  });

  it('does not show one user the device another user paired', async () => {
    const alice = await signupAndGetCookie('alice@example.com');
    const bob = await signupAndGetCookie('bob@example.com');
    const start = await startPairing();

    await approve(alice.cookie, start.user_code);
    await poll(start.device_code);

    const res = await SELF.fetch(`${BASE}/device`, { headers: authHeaders(bob.cookie) });
    expect(res.status).toBe(200);
    expect((await res.json()) as unknown[]).toHaveLength(0);
  });

  it('accepts the code however the user typed it', async () => {
    const { cookie } = await signupAndGetCookie('alice@example.com');
    const start = await startPairing();
    const bare = start.user_code.replace('-', '');

    for (const typed of [bare.toLowerCase(), `${bare.slice(0, 3)} ${bare.slice(3)}`, bare]) {
      const res = await lookup(cookie, typed);
      expect(res.status).toBe(200);
    }
  });
});

describe('User code encoding', () => {
  it('excludes the ambiguous glyphs', () => {
    expect(ALPHABET).toHaveLength(30);
    for (const char of ['I', 'L', 'O', 'U', '0', '1']) {
      expect(ALPHABET).not.toContain(char);
    }
  });

  it('generates six characters drawn only from the alphabet', () => {
    for (let i = 0; i < 200; i += 1) {
      const code = generateUserCode();
      expect(code).toHaveLength(6);
      for (const char of code) {
        expect(ALPHABET).toContain(char);
      }
    }
  });

  it('samples the alphabet without the modulo bias', () => {
    // Plain `byte % 30` would leave the first 16 letters ~7% likelier. Over this
    // many draws that gap is far larger than the sampling noise this bound allows.
    const counts = new Map<string, number>();
    const draws = 20000;

    for (let i = 0; i < draws; i += 1) {
      for (const char of generateUserCode()) {
        counts.set(char, (counts.get(char) ?? 0) + 1);
      }
    }

    const expected = (draws * 6) / ALPHABET.length;
    for (const char of ALPHABET) {
      expect(counts.get(char) ?? 0).toBeGreaterThan(expected * 0.85);
      expect(counts.get(char) ?? 0).toBeLessThan(expected * 1.15);
    }
  });

  it('formats for display as XXX-XXX', () => {
    expect(formatUserCode('K7RM3X')).toBe('K7R-M3X');
  });

  it('normalizes whatever shape the user typed', () => {
    expect(normalizeUserCode('k7r m3x')).toBe('K7RM3X');
    expect(normalizeUserCode('K7R-M3X')).toBe('K7RM3X');
    expect(normalizeUserCode('k7rm3x')).toBe('K7RM3X');
    expect(normalizeUserCode('  K7R.M3X  ')).toBe('K7RM3X');
  });

  it('rejects anything that is not six alphabet characters', () => {
    expect(normalizeUserCode('K7RM3')).toBeNull();
    expect(normalizeUserCode('K7RM3XY')).toBeNull();
    expect(normalizeUserCode('')).toBeNull();
    // I and L are outside the alphabet, so they are stripped rather than counted:
    // a user who typed a lookalike still gets six characters, and a wrong guess.
    expect(normalizeUserCode('K7RM3XIL')).toBe('K7RM3X');
  });
});
