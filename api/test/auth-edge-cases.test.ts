import { beforeAll, beforeEach, describe, expect, it, onTestFinished, vi } from 'vitest';
import { createExecutionContext, env, fetchMock, SELF } from 'cloudflare:test';
import worker from '../src/index';
import {
  BASE,
  TEST_TURNSTILE_TOKEN,
  authHeaders,
  clearDB,
  createDeviceForUser,
  extractTokenFromDelivery,
  listEmailDeliveries,
  passwordAuthFor,
  passwordSaltFor,
  privateKeyFor,
  publicKeyFor,
  signupAndGetCookie,
} from './helpers';
import {
  bearer,
  cookieFrom,
  cookieOnly,
  expireWebSessions,
  login,
  loginCookie,
  postJson,
  postPasswordReset,
  recordBounce,
  requestPasswordResetToken,
  snapshot,
} from './auth-helpers';
import { installHashServerMock } from './hash-server-mock';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

beforeEach(clearDB);

const EMAIL = 'user@example.com';

function call(
  method: string,
  path: string,
  options: { headers?: Record<string, string>; body?: unknown } = {},
) {
  return SELF.fetch(`${BASE}${path}`, {
    method,
    headers: { 'Content-Type': 'application/json', ...options.headers },
    ...(options.body === undefined ? {} : { body: JSON.stringify(options.body) }),
  });
}

async function getUser(cookie: string) {
  const res = await call('GET', '/user', { headers: cookieOnly(cookie) });
  return { status: res.status, body: (await res.json()) as Record<string, any> };
}

async function passwordBody(email: string, tag = 'new', password = 'new-password') {
  return {
    current_password_auth: await passwordAuthFor('password123'),
    password_auth: await passwordAuthFor(password),
    password_salt: await passwordSaltFor(`${email}:${tag}`),
    encrypted_priv_key: privateKeyFor(`${email}:${tag}`),
  };
}

/** One request per authenticated user route; `auth` is added to each. */
const webRoutes: Array<
  [name: string, send: (headers: Record<string, string>) => Promise<Response>]
> = [
  ['GET /user', (headers) => call('GET', '/user', { headers })],
  ['PATCH /user', (headers) => call('PATCH', '/user', { headers, body: { name: 'Changed' } })],
  [
    'DELETE /user',
    (headers) => call('DELETE', `/user?confirm_email=${encodeURIComponent(EMAIL)}`, { headers }),
  ],
  [
    'POST /user/password',
    async (headers) => call('POST', '/user/password', { headers, body: await passwordBody(EMAIL) }),
  ],
];

describe('Authentication (API-003)', () => {
  it.each(webRoutes)('rejects %s without any token with 401 (API-003)', async (_name, send) => {
    await signupAndGetCookie(EMAIL);
    const res = await send({});
    expect(res.status).toBe(401);
    expect(await res.json()).toHaveProperty('error');
  });

  it.each([
    ['a made-up token', 'not-a-token'],
    ['a made-up token with the web-session prefix', `wst_${'x'.repeat(40)}`],
    ['an empty cookie', ''],
  ])('rejects %s in the cookie with 401 (API-003)', async (_name, token) => {
    await signupAndGetCookie(EMAIL);
    expect((await call('GET', '/user', { headers: cookieOnly(token) })).status).toBe(401);
  });

  it('accepts the web refresh token from the cookie and from the Authorization header (API-003)', async () => {
    const { cookie, userId } = await signupAndGetCookie(EMAIL);
    for (const headers of [cookieOnly(cookie), bearer(cookie)]) {
      const res = await call('GET', '/user', { headers });
      expect(res.status).toBe(200);
      expect(await res.json()).toMatchObject({ id: userId, email: EMAIL });
    }
  });

  it('grants web tokens with a wst_ prefix on login and signup, and device tokens with a dst_ prefix (API-003)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    expect(cookie).toMatch(/^wst_/);
    expect(await loginCookie(EMAIL)).toMatch(/^wst_/);
    expect((await createDeviceForUser(EMAIL)).refresh_token).toMatch(/^dst_/);
  });

  it('rejects an expired web session with 401 (API-003)', async () => {
    const { cookie, userId } = await signupAndGetCookie(EMAIL);
    expect((await getUser(cookie)).status).toBe(200);
    await expireWebSessions(userId);
    expect((await getUser(cookie)).status).toBe(401);
  });

  it('rejects a token belonging to another purpose, however it is presented (API-003)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    const resetToken = await requestPasswordResetToken(EMAIL);
    const foreign = [
      device.refresh_token, // a device token
      device.token, // the hash-server JWT
      resetToken, // an emailed token
    ];
    for (const token of foreign) {
      for (const headers of [cookieOnly(token), bearer(token)]) {
        expect((await call('GET', '/user', { headers })).status).toBe(401);
      }
    }
    expect((await getUser(cookie)).status).toBe(200);
  });

  it.each(webRoutes.filter(([name]) => name !== 'GET /user'))(
    'rejects a device token on %s with 401 and changes nothing (API-003)',
    async (_name, send) => {
      const { cookie } = await signupAndGetCookie(EMAIL, 'password123', 'Original');
      const device = await createDeviceForUser(EMAIL);

      for (const headers of [bearer(device.refresh_token), cookieOnly(device.refresh_token)]) {
        expect((await send(headers)).status).toBe(401);
      }
      expect(await getUser(cookie)).toMatchObject({
        status: 200,
        body: { email: EMAIL, name: 'Original' },
      });
    },
  );

  it('rejects device routes with a web token, a hash-server JWT or no token with 401 (API-003)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    for (const [method, path] of [
      ['GET', '/d/device'],
      ['POST', '/d/logout'],
    ] as const) {
      for (const headers of [
        {},
        bearer(cookie),
        bearer(device.token),
        cookieOnly(device.refresh_token),
      ]) {
        expect((await call(method, path, { headers })).status).toBe(401);
      }
    }
    // None of those attempts logged the device out.
    expect((await call('GET', '/d/device', { headers: bearer(device.refresh_token) })).status).toBe(
      200,
    );
  });
});

describe('Validation (API-004)', () => {
  const validKeys = {
    password_auth: Buffer.alloc(32, 1).toString('base64'),
    password_salt: Buffer.alloc(16, 2).toString('base64'),
    pub_key: Buffer.alloc(32, 3).toString('base64'),
    encrypted_priv_key: Buffer.alloc(48, 4).toString('base64'),
  };
  const dropped = <T extends Record<string, unknown>>(body: T, key: keyof T) => {
    const { [key]: _removed, ...rest } = body;
    return rest;
  };
  const withoutEach = (body: Record<string, unknown>, route: string) =>
    Object.keys(body).map(
      (key) => [route, `${key} missing`, dropped(body, key), false] as RouteCase,
    );

  type RouteCase = [route: string, what: string, body: unknown, auth: boolean];

  const signupFields = { verification_token: 'sut_x', ...validKeys };
  const resetFields = { token: 'prt_x', ...validKeys };
  const deviceFields = {
    email: EMAIL,
    password_auth: validKeys.password_auth,
    name: 'L',
    platform: 'linux',
  };
  const passwordFields = {
    current_password_auth: validKeys.password_auth,
    password_auth: validKeys.password_auth,
    password_salt: validKeys.password_salt,
    encrypted_priv_key: validKeys.encrypted_priv_key,
  };

  const cases: RouteCase[] = [
    ['POST /signup-request', 'no body fields', {}, false],
    ['POST /signup-request', 'email is not an address', { email: 'nope' }, false],
    ['POST /signup-request', 'email is not a string', { email: 5 }, false],
    ...withoutEach(signupFields, 'POST /signup'),
    [
      'POST /signup',
      'password_auth is not base64',
      { ...signupFields, password_auth: '***' },
      false,
    ],
    ['POST /signup', 'name is empty', { ...signupFields, name: '' }, false],
    [
      'POST /signup',
      'newsletter_opt_in is not a boolean',
      { ...signupFields, newsletter_opt_in: 'yes' },
      false,
    ],
    ['POST /signup/validate', 'token missing', {}, false],
    ['POST /signup/validate', 'token is empty', { token: '' }, false],
    ['POST /signup/validate', 'token is not a string', { token: 5 }, false],
    ['POST /login', 'no body fields', {}, false],
    ['POST /login', 'email missing', { password_auth: validKeys.password_auth }, false],
    ['POST /login', 'password_auth missing', { email: EMAIL }, false],
    [
      'POST /login',
      'email is not an address',
      { email: 'nope', password_auth: validKeys.password_auth },
      false,
    ],
    ['POST /login', 'password_auth is not base64', { email: EMAIL, password_auth: '***' }, false],
    [
      'POST /login',
      'timezone is not a string',
      { email: EMAIL, password_auth: validKeys.password_auth, timezone: 5 },
      false,
    ],
    ['POST /email-verification/validate', 'token missing', {}, false],
    ['POST /email-verification/validate', 'token is empty', { token: '' }, false],
    ['POST /password-reset', 'email missing', {}, false],
    ['POST /password-reset', 'email is not an address', { email: 'nope' }, false],
    ['POST /password-reset/validate', 'token missing', {}, false],
    ['POST /password-reset/validate', 'token is empty', { token: '' }, false],
    ...withoutEach(resetFields, 'POST /password-reset/finalize'),
    ['PATCH /user', 'no fields', {}, true],
    ['PATCH /user', 'email is not an address', { email: 'nope' }, true],
    ['PATCH /user', 'name is empty', { name: '' }, true],
    ['PATCH /user', 'name is not a string', { name: 5 }, true],
    ['PATCH /user', 'unknown email_frequency', { settings: { email_frequency: 'hourly' } }, true],
    ['PATCH /user', 'timezone is not a zone', { settings: { timezone: 'Mars/Base' } }, true],
    ['PATCH /user', 'settings is not an object', { settings: 'weekly' }, true],
    ...Object.keys(passwordFields).map(
      (key) =>
        [
          'POST /user/password',
          `${key} missing`,
          dropped(passwordFields, key as never),
          true,
        ] as RouteCase,
    ),
    [
      'POST /user/password',
      'password_auth is not base64',
      { ...passwordFields, password_auth: '***' },
      true,
    ],
    [
      'POST /user/password',
      'password_auth is the wrong length',
      { ...passwordFields, password_auth: Buffer.alloc(8).toString('base64') },
      true,
    ],
    [
      'POST /user/password',
      'password_salt is the wrong length',
      { ...passwordFields, password_salt: Buffer.alloc(8).toString('base64') },
      true,
    ],
    ...withoutEach(deviceFields, 'POST /d/device'),
    ['POST /d/device', 'email is not an address', { ...deviceFields, email: 'nope' }, false],
    ['POST /d/device', 'name is empty', { ...deviceFields, name: '' }, false],
    ['POST /d/device', 'platform is empty', { ...deviceFields, platform: '' }, false],
  ];

  it.each(cases)('%s with %s answers 400 (API-004)', async (route, _what, body, auth) => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const [method, path] = route.split(' ') as [string, string];
    const res = await call(method, path, { headers: auth ? cookieOnly(cookie) : {}, body });
    expect(res.status).toBe(400);
    expect(await res.json()).toMatchObject({ error: expect.any(String) });
  });

  it.each([
    ['GET /user/login-material?email=nope', 'GET', '/user/login-material?email=nope'],
    ['DELETE /user without confirm_email', 'DELETE', '/user'],
    [
      'DELETE /user with confirm_email that is not an address',
      'DELETE',
      '/user?confirm_email=nope',
    ],
  ])('%s answers 400 (API-004)', async (_name, method, path) => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const res = await call(method, path, { headers: cookieOnly(cookie) });
    expect(res.status).toBe(400);
    expect((await getUser(cookie)).status).toBe(200);
  });

  for (const path of ['/signup-request', '/login', '/password-reset']) {
    it(`POST ${path} with a body that is not JSON answers 400 (API-004)`, async () => {
      const res = await SELF.fetch(`${BASE}${path}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: '{not json',
      });
      expect(res.status).toBe(400);
      expect(((await res.json()) as { error: unknown }).error).toEqual(expect.any(String));
    });
  }

  it('sends nothing and creates nothing for a request that fails validation (API-004)', async () => {
    await postJson('/signup-request', { email: 'nope', turnstile_token: TEST_TURNSTILE_TOKEN });
    await postPasswordReset('nope');
    expect(await listEmailDeliveries()).toHaveLength(0);
  });
});

describe('Status codes (API-006)', () => {
  const setup = async () => {
    const { cookie, userId } = await signupAndGetCookie(EMAIL);
    return { cookie, userId, headers: cookieOnly(cookie) };
  };

  it.each([
    [
      'POST /signup-request',
      async () =>
        postJson('/signup-request', {
          email: 'new@example.com',
          turnstile_token: TEST_TURNSTILE_TOKEN,
        }),
    ],
    [
      'POST /signup-request for an existing account',
      async () => {
        await setup();
        return postJson('/signup-request', { email: EMAIL, turnstile_token: TEST_TURNSTILE_TOKEN });
      },
    ],
    ['POST /password-reset', async () => postPasswordReset(EMAIL)],
    [
      'POST /login',
      async () => {
        await setup();
        return login(EMAIL);
      },
    ],
    [
      'POST /logout',
      async () => {
        const { headers } = await setup();
        return call('POST', '/logout', { headers });
      },
    ],
    [
      'POST /user/password',
      async () => {
        const { headers } = await setup();
        return call('POST', '/user/password', { headers, body: await passwordBody(EMAIL) });
      },
    ],
    [
      'DELETE /user',
      async () => {
        const { headers } = await setup();
        return call('DELETE', `/user?confirm_email=${encodeURIComponent(EMAIL)}`, { headers });
      },
    ],
    [
      'POST /d/logout',
      async () => {
        await setup();
        const device = await createDeviceForUser(EMAIL);
        return call('POST', '/d/logout', { headers: bearer(device.refresh_token) });
      },
    ],
  ])('%s answers 204 with no body (API-006)', async (_name, send) => {
    const res = await send();
    expect(res.status).toBe(204);
    expect(await res.text()).toBe('');
  });

  it('answers 200, not 201, for requests that create a resource (API-006)', async () => {
    await postJson('/signup-request', {
      email: 'new@example.com',
      turnstile_token: TEST_TURNSTILE_TOKEN,
    });
    const delivery = (await listEmailDeliveries()).at(-1)!;
    const signup = await postJson('/signup', {
      verification_token: extractTokenFromDelivery(delivery, 'signup_token'),
      password_auth: await passwordAuthFor('pw'),
      password_salt: await passwordSaltFor('new@example.com'),
      pub_key: await publicKeyFor('new@example.com'),
      encrypted_priv_key: privateKeyFor('new@example.com'),
    });
    expect(signup.status).toBe(200);
    expect(await signup.json()).toHaveProperty('user');

    await signupAndGetCookie(EMAIL);
    const device = await postJson('/d/device', {
      email: EMAIL,
      password_auth: await passwordAuthFor('password123'),
      name: 'Laptop',
      platform: 'linux',
    });
    expect(device.status).toBe(200);
    expect(await device.json()).toHaveProperty('token');
  });

  it.each([
    ['GET /user', 'GET', '/user', undefined],
    ['PATCH /user', 'PATCH', '/user', { name: 'Changed' }],
    ['GET /user/login-material', 'GET', '/user/login-material', undefined],
  ])('%s answers 200 with a JSON body (API-006)', async (_name, method, path, body) => {
    const { headers } = await setup();
    const res = await call(method, path, { headers, body });
    expect(res.status).toBe(200);
    expect(res.headers.get('content-type')).toMatch(/application\/json/);
    expect(await res.json()).toBeTypeOf('object');
  });
});

describe('POST /logout (API-018)', () => {
  it('clears the cookie, logs and answers 500 when it cannot delete the session (API-018)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
    onTestFinished(() => logged.mockRestore());
    const failingDb = {
      prepare: () => {
        throw new Error('D1 is down');
      },
    };

    const res = await worker.fetch(
      new Request(`${BASE}/logout`, { method: 'POST', headers: cookieOnly(cookie) }),
      { ...env, DB: failingDb as unknown as D1Database },
      createExecutionContext(),
    );

    expect(res.status).toBe(500);
    expect(await res.json()).toEqual({ error: 'Internal server error' });
    expect(res.headers.get('set-cookie') ?? '').toMatch(/refresh_token=;/);
    expect(logged).toHaveBeenCalledWith('[logout] failed to delete session', expect.any(Error));
  });

  it('invalidates the session, clears the cookie and answers 204 (API-018)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const res = await call('POST', '/logout', { headers: cookieOnly(cookie) });
    expect(res.status).toBe(204);
    expect(await res.text()).toBe('');
    const setCookie = res.headers.get('set-cookie') ?? '';
    expect(setCookie).toMatch(/refresh_token=;/);
    expect(setCookie).toMatch(/Max-Age=0|Expires=Thu, 01 Jan 1970/i);
    expect((await getUser(cookie)).status).toBe(401);
  });

  it('revokes only the session that logged out (API-018)', async () => {
    const { cookie: first } = await signupAndGetCookie(EMAIL);
    const second = await loginCookie(EMAIL);
    const { cookie: other } = await signupAndGetCookie('other@example.com');
    const device = await createDeviceForUser(EMAIL);

    await call('POST', '/logout', { headers: cookieOnly(first) });

    expect((await getUser(first)).status).toBe(401);
    expect((await getUser(second)).status).toBe(200);
    expect((await getUser(other)).status).toBe(200);
    expect((await call('GET', '/d/device', { headers: bearer(device.refresh_token) })).status).toBe(
      200,
    );
  });

  it.each([
    ['a cookie', cookieOnly],
    ['the Authorization header', bearer],
  ])('does not log out a device when given a device token in %s (API-018)', async (_name, as) => {
    await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    await call('POST', '/logout', { headers: as(device.refresh_token) });
    expect((await call('GET', '/d/device', { headers: bearer(device.refresh_token) })).status).toBe(
      200,
    );
  });

  it('invalidates a session presented in the Authorization header (API-018)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const res = await call('POST', '/logout', { headers: bearer(cookie) });
    expect(res.status).toBe(204);
    expect((await getUser(cookie)).status).toBe(401);
  });

  it('invalidates the Bearer session even when the cookie names a dead one (API-018)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    await call('POST', '/logout', {
      headers: { ...cookieOnly('not-a-token'), ...bearer(cookie) },
    });
    expect((await getUser(cookie)).status).toBe(401);
  });

  it('leaves other sessions alone when given no web token, and still answers 204 (API-018)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    for (const headers of [
      {},
      cookieOnly('not-a-token'),
      bearer('not-a-token'),
      cookieOnly(device.refresh_token),
    ]) {
      expect((await call('POST', '/logout', { headers })).status).toBe(204);
    }
    expect((await getUser(cookie)).status).toBe(200);
  });
});

describe('GET /user (API-019)', () => {
  it('returns the authenticated user with the User shape (API-019)', async () => {
    const { cookie, userId } = await signupAndGetCookie(
      EMAIL,
      'password123',
      'Una User',
      'Europe/Paris',
    );
    const { status, body } = await getUser(cookie);
    expect(status).toBe(200);
    expect(body).toEqual({
      id: userId,
      email: EMAIL,
      email_verified: true,
      email_bounced_at: null,
      settings: { email_frequency: 'weekly', timezone: 'Europe/Paris' },
      name: 'Una User',
      pub_key: await publicKeyFor(EMAIL),
      encrypted_priv_key: privateKeyFor(EMAIL),
      feature_flags: [],
    });
  });

  it('omits name when the account has none (API-019)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    expect((await getUser(cookie)).body).not.toHaveProperty('name');
  });

  it('returns each session its own user (API-019)', async () => {
    const { cookie: a } = await signupAndGetCookie('a@example.com');
    const { cookie: b } = await signupAndGetCookie('b@example.com');
    expect((await getUser(a)).body.email).toBe('a@example.com');
    expect((await getUser(b)).body.email).toBe('b@example.com');
  });

  it('answers 401 once the account is deleted (API-019)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    await call('DELETE', `/user?confirm_email=${encodeURIComponent(EMAIL)}`, {
      headers: cookieOnly(cookie),
    });
    expect((await getUser(cookie)).status).toBe(401);
  });

  it('answers a device token with 401 (API-019)', async () => {
    await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    const res = await call('GET', '/user', { headers: bearer(device.refresh_token) });
    expect(res.status).toBe(401);
  });
});

describe('PATCH /user (API-020)', () => {
  const patch = (cookie: string, body: unknown) =>
    call('PATCH', '/user', { headers: cookieOnly(cookie), body });

  it('answers a profile-only change with exactly { ok: true } (API-020)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const res = await patch(cookie, { name: 'New Name' });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true });
    expect((await getUser(cookie)).body.name).toBe('New Name');
  });

  it.each(['none', 'alerts-only', 'daily', 'weekly'])(
    'sets email_frequency to %s without touching the timezone (API-020)',
    async (frequency) => {
      const { cookie } = await signupAndGetCookie(EMAIL, 'password123', undefined, 'Asia/Tokyo');
      expect((await patch(cookie, { settings: { email_frequency: frequency } })).status).toBe(200);
      expect((await getUser(cookie)).body.settings).toEqual({
        email_frequency: frequency,
        timezone: 'Asia/Tokyo',
      });
    },
  );

  it('sets the timezone without touching the email frequency (API-020)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    await patch(cookie, { settings: { email_frequency: 'daily' } });
    await patch(cookie, { settings: { timezone: 'America/Chicago' } });
    expect((await getUser(cookie)).body.settings).toEqual({
      email_frequency: 'daily',
      timezone: 'America/Chicago',
    });
  });

  it('applies nothing from a request that fails validation (API-020)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL, 'password123', 'Before');
    const res = await patch(cookie, {
      name: 'After',
      settings: { email_frequency: 'hourly' },
    });
    expect(res.status).toBe(400);
    expect((await getUser(cookie)).body).toMatchObject({
      name: 'Before',
      settings: { email_frequency: 'weekly' },
    });
  });

  it('does not accept key material or the verified flag in the body (API-020)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    await patch(cookie, {
      name: 'Changed',
      pub_key: await publicKeyFor('attacker'),
      encrypted_priv_key: privateKeyFor('attacker'),
      email_verified: false,
      password_auth: await passwordAuthFor('attacker'),
    });
    expect((await getUser(cookie)).body).toMatchObject({
      pub_key: await publicKeyFor(EMAIL),
      encrypted_priv_key: privateKeyFor(EMAIL),
      email_verified: true,
    });
    expect((await login(EMAIL, 'attacker')).status).toBe(401);
  });

  it('does not change the address, and sends the confirmation only to the new address, lowercased (API-020)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const before = (await listEmailDeliveries()).length;

    const res = await patch(cookie, { email: 'Moved@Example.COM' });
    expect(await res.json()).toEqual({
      ok: true,
      email_verification_required: true,
      pending_email: 'moved@example.com',
    });
    expect((await getUser(cookie)).body.email).toBe(EMAIL);

    const sent = (await listEmailDeliveries()).slice(before);
    expect(sent.map((d) => [d.kind, d.recipient_email])).toEqual([
      ['email_verification', 'moved@example.com'],
    ]);
  });

  it('answers an address already in use exactly like a free one, and tells only the owner (API-020)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    await signupAndGetCookie('owner@example.com');
    const before = (await listEmailDeliveries()).length;

    const free = await patch(cookie, { email: 'free@example.com' });
    const taken = await patch(cookie, { email: 'owner@example.com' });
    const freeBody = (await snapshot(free)).body.replace('free@example.com', 'X');
    const takenBody = (await snapshot(taken)).body.replace('owner@example.com', 'X');
    expect(taken.status).toBe(free.status);
    expect(takenBody).toBe(freeBody);

    const sent = (await listEmailDeliveries()).slice(before);
    expect(sent.map((d) => [d.kind, d.recipient_email])).toEqual([
      ['email_verification', 'free@example.com'],
      ['email_in_use_notice', 'owner@example.com'],
    ]);
  });

  it('still updates the other fields when the new address is in use (API-020)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    await signupAndGetCookie('owner@example.com');
    const res = await patch(cookie, {
      email: 'owner@example.com',
      name: 'Still Updated',
      settings: { email_frequency: 'none' },
    });
    expect(res.status).toBe(200);
    expect((await getUser(cookie)).body).toMatchObject({
      email: EMAIL,
      name: 'Still Updated',
      settings: { email_frequency: 'none' },
    });
  });

  it('flags email_bounced and sends nothing when the new address has a permanent bounce (API-020)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    await recordBounce('bounced@example.com');
    const before = (await listEmailDeliveries()).length;

    const res = await patch(cookie, { email: 'bounced@example.com' });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      ok: true,
      email_verification_required: true,
      pending_email: 'bounced@example.com',
      email_bounced: true,
    });
    expect(await listEmailDeliveries()).toHaveLength(before);
  });

  it('does not set email_bounced for a temporary bounce (API-020)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    await recordBounce('flaky@example.com', 'temporary');
    const res = await patch(cookie, { email: 'flaky@example.com' });
    expect(await res.json()).not.toHaveProperty('email_bounced');
  });

  it('requires a web token (API-020)', async () => {
    await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    expect((await call('PATCH', '/user', { body: { name: 'x' } })).status).toBe(401);
    expect(
      (await call('PATCH', '/user', { headers: bearer(device.refresh_token), body: { name: 'x' } }))
        .status,
    ).toBe(401);
  });
});

describe('DELETE /user (API-021)', () => {
  const remove = (cookie: string, confirm?: string) =>
    call(
      'DELETE',
      `/user${confirm === undefined ? '' : `?confirm_email=${encodeURIComponent(confirm)}`}`,
      { headers: cookieOnly(cookie) },
    );

  it.each([
    ['a different address', 'someone-else@example.com'],
    ['the address of another account', 'other@example.com'],
    ['nothing', undefined],
  ])(
    'answers 400 and keeps the account when confirm_email is %s (API-021)',
    async (_name, confirm) => {
      const { cookie } = await signupAndGetCookie(EMAIL);
      await signupAndGetCookie('other@example.com');
      const device = await createDeviceForUser(EMAIL);

      const res = await remove(cookie, confirm);
      expect(res.status).toBe(400);
      expect(res.headers.get('set-cookie')).toBeNull();
      expect((await getUser(cookie)).status).toBe(200);
      expect((await login(EMAIL)).status).toBe(204);
      expect(
        (await call('GET', '/d/device', { headers: bearer(device.refresh_token) })).status,
      ).toBe(200);
    },
  );

  it('deletes every session, device token and pending token of the account, and frees the address (API-021)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const otherSession = await loginCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    const resetToken = await requestPasswordResetToken(EMAIL);
    const { cookie: bystander } = await signupAndGetCookie('bystander@example.com');

    const res = await remove(cookie, EMAIL);
    expect(res.status).toBe(204);
    expect(res.headers.get('set-cookie')).toMatch(/refresh_token=;/);

    expect((await getUser(cookie)).status).toBe(401);
    expect((await getUser(otherSession)).status).toBe(401);
    expect((await call('GET', '/d/device', { headers: bearer(device.refresh_token) })).status).toBe(
      401,
    );
    expect((await login(EMAIL)).status).toBe(401);
    expect(
      (await postJson('/password-reset/validate', { token: resetToken })).status,
    ).toBeGreaterThanOrEqual(400);
    expect((await getUser(bystander)).status).toBe(200);

    // The address can be registered again from scratch.
    await postJson('/signup-request', { email: EMAIL, turnstile_token: TEST_TURNSTILE_TOKEN });
    expect((await listEmailDeliveries()).at(-1)).toMatchObject({
      kind: 'email_verification',
      recipient_email: EMAIL,
    });
  });

  it("removes the account's devices from the owner of nothing else (API-021)", async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const { cookie: other } = await signupAndGetCookie('other@example.com');
    await createDeviceForUser(EMAIL);
    const otherDevice = await createDeviceForUser('other@example.com');

    await remove(cookie, EMAIL);

    const list = await call('GET', '/device', { headers: cookieOnly(other) });
    expect(((await list.json()) as Array<{ id: string }>).map((d) => d.id)).toEqual([
      otherDevice.id,
    ]);
  });

  it('requires a web token (API-021)', async () => {
    await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    const path = `/user?confirm_email=${encodeURIComponent(EMAIL)}`;
    expect((await call('DELETE', path)).status).toBe(401);
    expect((await call('DELETE', path, { headers: bearer(device.refresh_token) })).status).toBe(
      401,
    );
    expect((await login(EMAIL)).status).toBe(204);
  });
});

describe('POST /user/password (API-050)', () => {
  const change = (cookie: string, body: unknown) =>
    call('POST', '/user/password', { headers: cookieOnly(cookie), body });

  it('answers 403, not 401, for a wrong current password, and leaves the session and keys alone (API-050)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const res = await change(cookie, {
      ...(await passwordBody(EMAIL)),
      current_password_auth: await passwordAuthFor('not-my-password'),
    });
    expect(res.status).toBe(403);
    expect(await res.json()).toHaveProperty('error');

    expect((await getUser(cookie)).status).toBe(200);
    expect((await getUser(cookie)).body).toMatchObject({
      encrypted_priv_key: privateKeyFor(EMAIL),
    });
    expect((await login(EMAIL)).status).toBe(204);
    expect((await login(EMAIL, 'new-password')).status).toBe(401);
  });

  it.each(['current_password_auth', 'password_auth', 'password_salt', 'encrypted_priv_key'])(
    'answers 400 and changes nothing when %s is not valid base64 (API-050)',
    async (field) => {
      const { cookie } = await signupAndGetCookie(EMAIL);
      const res = await change(cookie, { ...(await passwordBody(EMAIL)), [field]: '***' });
      expect(res.status).toBe(400);
      expect((await login(EMAIL)).status).toBe(204);
      expect((await login(EMAIL, 'new-password')).status).toBe(401);
    },
  );

  it('ignores a pub_key in the body and keeps the key pair (API-050)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const res = await change(cookie, {
      ...(await passwordBody(EMAIL)),
      pub_key: await publicKeyFor('attacker'),
    });
    expect(res.status).toBe(204);
    expect((await getUser(cookie)).body).toMatchObject({
      pub_key: await publicKeyFor(EMAIL),
      encrypted_priv_key: privateKeyFor(`${EMAIL}:new`),
    });
  });

  it('serves the new salt from login-material and accepts only the new password (API-050)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    expect((await change(cookie, await passwordBody(EMAIL))).status).toBe(204);

    const material = await call('GET', `/user/login-material?email=${encodeURIComponent(EMAIL)}`);
    expect(((await material.json()) as { password_salt: string }).password_salt).toBe(
      await passwordSaltFor(`${EMAIL}:new`),
    );
    expect((await login(EMAIL)).status).toBe(401);
    expect((await login(EMAIL, 'new-password')).status).toBe(204);
  });

  it('invalidates an outstanding password-reset link, so it can no longer replace the new password (API-050)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const resetToken = await requestPasswordResetToken(EMAIL);
    expect((await postJson('/password-reset/validate', { token: resetToken })).status).toBe(200);

    await change(cookie, await passwordBody(EMAIL));

    expect(
      (await postJson('/password-reset/validate', { token: resetToken })).status,
    ).toBeGreaterThanOrEqual(400);
    const finalize = await postJson('/password-reset/finalize', {
      token: resetToken,
      password_auth: await passwordAuthFor('attacker'),
      password_salt: await passwordSaltFor('attacker'),
      pub_key: await publicKeyFor('attacker'),
      encrypted_priv_key: privateKeyFor('attacker'),
    });
    expect(finalize.status).toBeGreaterThanOrEqual(400);
    expect((await login(EMAIL, 'new-password')).status).toBe(204);
    expect((await login(EMAIL, 'attacker')).status).toBe(401);
  });

  it('keeps every device session working and emails the user once (API-050)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    await change(cookie, await passwordBody(EMAIL));

    expect((await call('GET', '/d/device', { headers: bearer(device.refresh_token) })).status).toBe(
      200,
    );
    const notices = (await listEmailDeliveries()).filter((d) => d.kind === 'password_changed');
    expect(notices.map((d) => d.recipient_email)).toEqual([EMAIL]);
  });

  it('requires a web token (API-050)', async () => {
    const { cookie } = await signupAndGetCookie(EMAIL);
    const device = await createDeviceForUser(EMAIL);
    const body = await passwordBody(EMAIL);
    for (const headers of [{}, bearer(device.refresh_token), bearer(device.token)]) {
      expect((await call('POST', '/user/password', { headers, body })).status).toBe(401);
    }
    expect((await login(EMAIL)).status).toBe(204);
    expect(cookieFrom(await login(EMAIL, 'new-password'))).toBeUndefined();
    expect((await getUser(cookie)).status).toBe(200);
  });
});
