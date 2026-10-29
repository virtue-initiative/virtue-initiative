import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { env, fetchMock, SELF } from 'cloudflare:test';
import {
  BASE,
  authHeaders,
  clearDB,
  createDeviceForUser,
  listEmailDeliveries,
  passwordAuthFor,
  signupAndGetCookie,
  uuidToBytes,
  publicKeyFor,
} from './helpers';
import {
  bearer,
  cookieOnly,
  decodeJwt,
  expireDeviceSessions,
  recordBounce,
  snapshot,
} from './auth-helpers';
import { TEST_JWT_PUBLIC_KEY } from './jwt-test-keys';
import { getHashState, installHashServerMock, seedHashState } from './hash-server-mock';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

beforeEach(clearDB);

const EMAIL = 'owner@example.com';
const CALIFORNIA = { country: 'US', regionCode: 'CA' } as IncomingRequestCfProperties;

async function register(body: Record<string, unknown> = {}, init: RequestInit = {}) {
  return SELF.fetch(`${BASE}/d/device`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      email: EMAIL,
      password_auth: await passwordAuthFor('password123'),
      name: 'Laptop',
      platform: 'linux',
      ...body,
    }),
    ...init,
  });
}

function getSettings(token: string) {
  return SELF.fetch(`${BASE}/d/device`, { headers: bearer(token) });
}

function logout(token: string) {
  return SELF.fetch(`${BASE}/d/logout`, { method: 'POST', headers: bearer(token) });
}

async function verifiesWithTestKey(jwt: string) {
  const [header, payload, signature] = jwt.split('.') as [string, string, string];
  const spki = Buffer.from(TEST_JWT_PUBLIC_KEY.replace(/-----[^-]+-----|\s/g, ''), 'base64');
  const key = await crypto.subtle.importKey('spki', spki, { name: 'Ed25519' }, false, ['verify']);
  return crypto.subtle.verify(
    'Ed25519',
    key,
    Buffer.from(signature, 'base64url'),
    new TextEncoder().encode(`${header}.${payload}`),
  );
}

describe('POST /d/device (API-035)', () => {
  it('registers a device and returns a device refresh token and the device settings (API-035)', async () => {
    const { userId } = await signupAndGetCookie(EMAIL);
    const res = await register({ name: 'Work Laptop', platform: 'windows' });
    expect(res.status).toBe(200);
    expect(res.headers.get('set-cookie')).toBeNull();

    const body = (await res.json()) as { token: string; settings: Record<string, any> };
    expect(Object.keys(body).sort()).toEqual(['settings', 'token']);
    expect(body.token).toMatch(/^dst_/);
    expect(body.settings).toMatchObject({
      id: expect.stringMatching(/^[0-9a-f-]{36}$/),
      name: 'Work Laptop',
      platform: 'windows',
      wrapping_keys: [{ user_id: userId, pub_key: await publicKeyFor(EMAIL) }],
      hash_base_url: expect.stringMatching(/^https:\/\//),
      feature_flags: [],
    });

    const claims = decodeJwt(body.settings.hash_token);
    expect(claims).toMatchObject({ type: 'device', sub: body.settings.id });
    expect((claims.exp as number) - (claims.iat as number)).toBe(2 * 60 * 60);
    expect(await verifiesWithTestKey(body.settings.hash_token)).toBe(true);
  });

  it('returns settings that match what GET /d/device returns for the new token (API-035)', async () => {
    await signupAndGetCookie(EMAIL);
    const body = (await (await register()).json()) as {
      token: string;
      settings: Record<string, unknown>;
    };
    const later = (await (await getSettings(body.token)).json()) as Record<string, unknown>;
    const { hash_token: _a, ...registered } = body.settings;
    const { hash_token: _b, ...fetched } = later;
    expect(fetched).toEqual(registered);
  });

  it('lists the new device for its owner (API-035)', async () => {
    const { cookie, userId } = await signupAndGetCookie(EMAIL);
    const { settings } = (await (await register({ name: 'Desk', platform: 'macos' })).json()) as {
      settings: { id: string };
    };
    const list = await SELF.fetch(`${BASE}/device`, { headers: authHeaders(cookie) });
    expect(await list.json()).toEqual([
      expect.objectContaining({ id: settings.id, owner: userId, name: 'Desk', platform: 'macos' }),
    ]);
  });

  it('gives every registration its own device id and token (API-035)', async () => {
    await signupAndGetCookie(EMAIL);
    const first = await createDeviceForUser(EMAIL);
    const second = await createDeviceForUser(EMAIL);
    expect(second.id).not.toBe(first.id);
    expect(second.refresh_token).not.toBe(first.refresh_token);
    expect(((await (await getSettings(second.refresh_token)).json()) as { id: string }).id).toBe(
      second.id,
    );
  });

  it('matches the email case-insensitively (API-035)', async () => {
    await signupAndGetCookie(EMAIL);
    expect((await register({ email: 'OWNER@Example.COM' })).status).toBe(200);
  });

  it('answers a wrong password, an unknown email and a malformed password_auth identically with 401, and registers nothing (API-035)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const wrong = await snapshot(await register({ password_auth: await passwordAuthFor('wrong') }));
    const unknown = await snapshot(await register({ email: 'nobody@example.com' }));
    const short = await snapshot(
      await register({ password_auth: Buffer.alloc(8).toString('base64') }),
    );
    expect(wrong.status).toBe(401);
    expect(unknown).toEqual(wrong);
    expect(short).toEqual(wrong);

    const list = await SELF.fetch(`${BASE}/device`, { headers: authHeaders(cookie) });
    expect(await list.json()).toEqual([]);
  });

  it('refuses an unverified account with 403 and registers nothing (API-035)', async () => {
    const { cookie, userId } = await signupAndGetCookie(EMAIL);
    await env.DB.prepare('UPDATE users SET email_verified = 0 WHERE id = ?')
      .bind(uuidToBytes(userId))
      .run();
    const res = await register();
    expect(res.status).toBe(403);
    expect(await res.json()).toHaveProperty('error');
    expect(res.headers.get('set-cookie')).toBeNull();

    // Registration was refused, so the account has no device once it is verified again.
    const list = await SELF.fetch(`${BASE}/device`, { headers: authHeaders(cookie) });
    expect(await list.json()).toEqual([]);
  });

  // KNOWN-BUG: registering on an unverified account answers 403 but sends no verification email, where POST /login (which this must follow) sends one
  it.fails(
    'sends a verification email when the account is unverified, as POST /login does (API-035)',
    async () => {
      const { userId } = await signupAndGetCookie(EMAIL);
      await env.DB.prepare('UPDATE users SET email_verified = 0 WHERE id = ?')
        .bind(uuidToBytes(userId))
        .run();
      const before = (await listEmailDeliveries()).length;
      await register();
      const sent = (await listEmailDeliveries()).slice(before);
      expect(sent.map((d) => [d.kind, d.recipient_email])).toEqual([['email_verification', EMAIL]]);
    },
  );

  // KNOWN-BUG: registering on an unverified account whose address bounced answers a plain 403 without code email_bounced, where POST /login answers 403 with code email_bounced
  it.fails(
    'answers 403 email_bounced for an unverified account with a bounced address, as POST /login does (API-035)',
    async () => {
      const { userId } = await signupAndGetCookie(EMAIL);
      await env.DB.prepare('UPDATE users SET email_verified = 0 WHERE id = ?')
        .bind(uuidToBytes(userId))
        .run();
      await recordBounce(EMAIL);
      const res = await register();
      expect(res.status).toBe(403);
      expect(await res.json()).toMatchObject({ code: 'email_bounced' });
    },
  );

  it('fills a missing newsletter location from the registration request and keeps an existing one (API-035)', async () => {
    await signupAndGetCookie(EMAIL);
    await signupAndGetCookie('located@example.com');
    await env.DONATE_DB.batch([
      env.DONATE_DB.prepare('INSERT INTO newsletter_signups (email) VALUES (?)').bind(EMAIL),
      env.DONATE_DB.prepare(
        "INSERT INTO newsletter_signups (email, country, region) VALUES (?, 'CA', 'ON')",
      ).bind('located@example.com'),
    ]);

    for (const email of [EMAIL, 'located@example.com']) {
      const res = await register({ email }, { cf: CALIFORNIA });
      expect(res.status).toBe(200);
    }

    const row = (email: string) =>
      env.DONATE_DB.prepare('SELECT country, region FROM newsletter_signups WHERE email = ?')
        .bind(email)
        .first();
    expect(await row(EMAIL)).toEqual({ country: 'US', region: 'CA' });
    expect(await row('located@example.com')).toEqual({ country: 'CA', region: 'ON' });
  });

  it('does not let the device token act as a web session (API-035)', async () => {
    await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    expect(
      (await SELF.fetch(`${BASE}/device`, { headers: bearer(device.refresh_token) })).status,
    ).toBe(401);
  });
});

describe('GET /d/device (API-036)', () => {
  it.each([
    ['no token', () => ({})],
    ['a made-up token', () => bearer('not-a-token')],
    ['a made-up device token', () => bearer(`dst_${'x'.repeat(40)}`)],
  ])('answers %s with 401 (API-036)', async (_name, headers) => {
    expect((await SELF.fetch(`${BASE}/d/device`, { headers: headers() })).status).toBe(401);
  });

  it('answers a web token, in a header or a cookie, with 401 (API-036)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    for (const headers of [bearer(cookie), cookieOnly(cookie)]) {
      expect((await SELF.fetch(`${BASE}/d/device`, { headers })).status).toBe(401);
    }
  });

  it('answers the hash-server JWT it minted with 401 (API-036)', async () => {
    await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    expect((await getSettings(device.token)).status).toBe(401);
  });

  it('answers an expired device session with 401 (API-036)', async () => {
    await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    expect((await getSettings(device.refresh_token)).status).toBe(200);
    await expireDeviceSessions(device.id);
    expect((await getSettings(device.refresh_token)).status).toBe(401);
  });

  it('returns DeviceSettings with a fresh 2 hour hash-server token for this device (API-036)', async () => {
    const { userId } = await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL, 'password123', 'Desk', 'macos');
    await createDeviceForUser(EMAIL, 'password123', 'Other', 'linux');

    const res = await getSettings(device.refresh_token);
    expect(res.status).toBe(200);
    const settings = (await res.json()) as Record<string, any>;
    expect(settings).toMatchObject({
      id: device.id,
      name: 'Desk',
      platform: 'macos',
      wrapping_keys: [{ user_id: userId, pub_key: await publicKeyFor(EMAIL) }],
      hash_base_url: expect.stringMatching(/^https:\/\//),
      feature_flags: [],
    });

    const claims = decodeJwt(settings.hash_token);
    expect(claims).toMatchObject({ type: 'device', sub: device.id });
    expect((claims.exp as number) - (claims.iat as number)).toBe(2 * 60 * 60);
    expect(Math.abs((claims.iat as number) * 1000 - Date.now())).toBeLessThan(60_000);
    expect(await verifiesWithTestKey(settings.hash_token)).toBe(true);
  });

  it('reflects a rename made by the owner (API-036)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL, 'password123', 'Before', 'linux');
    const rename = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'PATCH',
      headers: authHeaders(cookie),
      body: JSON.stringify({ name: 'After' }),
    });
    expect(rename.status).toBeLessThan(300);
    expect(
      ((await (await getSettings(device.refresh_token)).json()) as { name: string }).name,
    ).toBe('After');
  });
});

describe('POST /d/logout (API-037)', () => {
  async function addWatcher(ownerCookie: string, watcherEmail: string, accept = true) {
    const { cookie } = await signupAndGetCookie(watcherEmail);
    const invite = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({ email: watcherEmail }),
    });
    expect(invite.status).toBe(200);
    if (accept) {
      const delivery = (await listEmailDeliveries())
        .filter((d) => d.kind === 'partner_invite' && d.recipient_email === watcherEmail)
        .at(-1)!;
      const { inviteToken } = JSON.parse(delivery.metadata) as { inviteToken: string };
      const accepted = await SELF.fetch(`${BASE}/partner/accept`, {
        method: 'POST',
        headers: authHeaders(cookie),
        body: JSON.stringify({ token: inviteToken }),
      });
      expect(accepted.status).toBe(200);
    }
    return cookie;
  }

  const logoutEmails = async () =>
    (await listEmailDeliveries())
      .filter((d) => d.kind === 'device_logout')
      .map((d) => d.recipient_email)
      .sort();

  it.each([
    ['no token', () => ({})],
    ['a made-up token', () => bearer('not-a-token')],
  ])('answers %s with 401 and logs nothing out (API-037)', async (_name, headers) => {
    await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    expect(
      (await SELF.fetch(`${BASE}/d/logout`, { method: 'POST', headers: headers() })).status,
    ).toBe(401);
    expect((await getSettings(device.refresh_token)).status).toBe(200);
    expect(await logoutEmails()).toEqual([]);
  });

  it('answers a web token with 401 and leaves the device and the session alone (API-037)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    for (const headers of [bearer(cookie), cookieOnly(cookie), bearer(device.token)]) {
      expect((await SELF.fetch(`${BASE}/d/logout`, { method: 'POST', headers })).status).toBe(401);
    }
    expect((await getSettings(device.refresh_token)).status).toBe(200);
    expect((await SELF.fetch(`${BASE}/user`, { headers: authHeaders(cookie) })).status).toBe(200);
  });

  it('revokes the token for good: a second logout and GET /d/device both answer 401 (API-037)', async () => {
    await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    expect((await logout(device.refresh_token)).status).toBe(204);
    expect((await logout(device.refresh_token)).status).toBe(401);
    expect((await getSettings(device.refresh_token)).status).toBe(401);
  });

  it('affects only the device that logged out (API-037)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const first = await createDeviceForUser(EMAIL, 'password123', 'First', 'linux');
    const second = await createDeviceForUser(EMAIL, 'password123', 'Second', 'linux');
    seedHashState(first.id, { hash: '09'.repeat(32), seq: 3, last_received: 1 });
    seedHashState(second.id, { hash: '0a'.repeat(32), seq: 4, last_received: 2 });

    expect((await logout(first.refresh_token)).status).toBe(204);

    expect((await getSettings(second.refresh_token)).status).toBe(200);
    expect(getHashState(first.id)).toMatchObject({ seq: 0, hash: '0'.repeat(64) });
    expect(getHashState(second.id)).toMatchObject({ seq: 4 });

    const list = (await (
      await SELF.fetch(`${BASE}/device`, { headers: authHeaders(cookie) })
    ).json()) as Array<{ id: string; status: string }>;
    expect(list.find((d) => d.id === first.id)?.status).toBe('logged_out');
    expect(list.find((d) => d.id === second.id)?.status).not.toBe('logged_out');
    expect((await SELF.fetch(`${BASE}/user`, { headers: authHeaders(cookie) })).status).toBe(200);
  });

  it('emails the owner and each accepted watcher, but not a pending invite (API-037)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    await addWatcher(cookie, 'watcher@example.com');
    await addWatcher(cookie, 'pending@example.com', false);
    const device = await createDeviceForUser(EMAIL, 'password123', 'Den PC', 'linux');

    expect((await logout(device.refresh_token)).status).toBe(204);

    expect(await logoutEmails()).toEqual([EMAIL, 'watcher@example.com']);
    const toWatcher = (await listEmailDeliveries()).find(
      (d) => d.kind === 'device_logout' && d.recipient_email === 'watcher@example.com',
    );
    expect(toWatcher?.text).toContain('Den PC');
  });

  it('skips a watcher whose email frequency is none, but still emails the owner (API-037)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const quiet = await addWatcher(cookie, 'quiet@example.com');
    await addWatcher(cookie, 'loud@example.com');
    const mute = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(quiet),
      body: JSON.stringify({ settings: { email_frequency: 'none' } }),
    });
    expect(mute.status).toBe(200);
    const device = await createDeviceForUser(EMAIL);

    await logout(device.refresh_token);

    expect(await logoutEmails()).toEqual(['loud@example.com', EMAIL]);
  });

  it('emails the owner even when the owner has turned email off (API-037)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(cookie),
      body: JSON.stringify({ settings: { email_frequency: 'none' } }),
    });
    const device = await createDeviceForUser(EMAIL);
    await logout(device.refresh_token);
    expect(await logoutEmails()).toEqual([EMAIL]);
  });
});
