import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { createExecutionContext, fetchMock, SELF, env } from 'cloudflare:test';
import worker from '../src/index';
import type { Env } from '../src/types/bindings';
import {
  BASE,
  TEST_TURNSTILE_TOKEN,
  authHeaders,
  clearDB,
  extractTokenFromDelivery,
  listEmailDeliveries,
  passwordAuthFor,
  passwordSaltFor,
  publicKeyFor,
  privateKeyFor,
  signupAndGetCookie,
  uuidToBytes,
} from './helpers';
import {
  cookieFrom,
  expireEmailTokens,
  login,
  loginCookie,
  newPasswordMaterial,
  postJson,
  postPasswordReset,
  recordBounce,
  requestPasswordResetToken,
  requestSignupToken,
  signupBody,
  snapshot,
} from './auth-helpers';
import { installHashServerMock } from './hash-server-mock';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

beforeEach(clearDB);

async function deliveriesOfKind(kind: string) {
  return (await listEmailDeliveries()).filter((d) => d.kind === kind);
}

async function canLogIn(email: string, password = 'password123') {
  return (await login(email, password)).status === 204;
}

/** Marks the account unverified and returns the token from the verification email a login sends. */
async function unverifiedUserToken(email: string) {
  const { userId } = await signupAndGetCookie(email);
  await env.DB.prepare('UPDATE users SET email_verified = 0 WHERE id = ?')
    .bind(uuidToBytes(userId))
    .run();
  const res = await login(email);
  expect(res.status).toBe(403);
  const delivery = (await deliveriesOfKind('email_verification')).at(-1)!;
  return extractTokenFromDelivery(delivery, 'token')!;
}

/** Token-making steps for tokens that must be refused wherever a given kind is required. */
type TokenCase = [name: string, make: () => Promise<string>];

const unknownTokens: TokenCase[] = [
  ['an unknown token', async () => 'not-a-real-token'],
  ['an unknown token with a real prefix', async () => `sut_${'x'.repeat(40)}`],
];

const signupLinkToken = async (email: string) => requestSignupToken(email);
const resetLinkToken = async (email: string) => {
  await signupAndGetCookie(email);
  return requestPasswordResetToken(email);
};

describe('POST /signup-request (API-009)', () => {
  const request = (body: unknown) => postJson('/signup-request', body);

  it('refuses a missing or unverified Turnstile token with 403 turnstile_failed and sends nothing (API-009)', async () => {
    for (const body of [
      { email: 'new@example.com' },
      { email: 'new@example.com', turnstile_token: '' },
      { email: 'new@example.com', turnstile_token: 'not-the-test-token' },
    ]) {
      const res = await request(body);
      expect(res.status).toBe(403);
      expect(await res.json()).toMatchObject({ code: 'turnstile_failed' });
    }
    expect(await listEmailDeliveries()).toHaveLength(0);
    expect(await canLogIn('new@example.com')).toBe(false);
  });

  it('checks Turnstile before the account lookup and the bounce check (API-009)', async () => {
    await signupAndGetCookie('taken@example.com');
    await recordBounce('bounced@example.com');
    const before = (await listEmailDeliveries()).length;

    for (const email of ['taken@example.com', 'bounced@example.com', 'new@example.com']) {
      const res = await request({ email, turnstile_token: 'bad' });
      expect(res.status).toBe(403);
      expect(await res.json()).toMatchObject({ code: 'turnstile_failed' });
    }
    expect(await listEmailDeliveries()).toHaveLength(before);
  });

  it('sends one signup link to a new address, and a notification to an existing one, with identical responses (API-009)', async () => {
    await signupAndGetCookie('taken@example.com');
    const before = (await listEmailDeliveries()).length;

    const fresh = await request({
      email: 'fresh@example.com',
      turnstile_token: TEST_TURNSTILE_TOKEN,
    });
    const existing = await request({
      email: 'taken@example.com',
      turnstile_token: TEST_TURNSTILE_TOKEN,
    });

    expect(await snapshot(fresh)).toEqual(await snapshot(existing));
    expect(fresh.status).toBe(204);

    const sent = (await listEmailDeliveries()).slice(before);
    expect(sent.map((d) => [d.kind, d.recipient_email])).toEqual([
      ['email_verification', 'fresh@example.com'],
      ['account_exists_notice', 'taken@example.com'],
    ]);
  });

  it('matches addresses case-insensitively (API-009)', async () => {
    await signupAndGetCookie('taken@example.com');

    await request({ email: 'TAKEN@Example.COM', turnstile_token: TEST_TURNSTILE_TOKEN });
    expect((await deliveriesOfKind('account_exists_notice')).map((d) => d.recipient_email)).toEqual(
      ['taken@example.com'],
    );

    // A mixed-case request for a new address yields a link that signs up the lowercase address.
    const res = await request({
      email: 'Mixed@Example.COM',
      turnstile_token: TEST_TURNSTILE_TOKEN,
    });
    expect(res.status).toBe(204);
    const delivery = (await deliveriesOfKind('email_verification')).at(-1)!;
    expect(delivery.recipient_email).toBe('mixed@example.com');
    const validate = await postJson('/signup/validate', {
      token: extractTokenFromDelivery(delivery, 'signup_token'),
    });
    expect(await validate.json()).toEqual({ email: 'mixed@example.com' });
  });

  it('accepts a partner_invite_token alongside the email (API-009)', async () => {
    const res = await request({
      email: 'invited@example.com',
      partner_invite_token: 'pit_whatever',
      turnstile_token: TEST_TURNSTILE_TOKEN,
    });
    expect(res.status).toBe(204);
    expect(await deliveriesOfKind('email_verification')).toHaveLength(1);
  });

  it.each([
    ['an address with a new account', undefined],
    ['an address with an existing account', 'exists'],
  ])(
    'refuses a permanently bounced address with 422 email_bounced and sends nothing: %s (API-009)',
    async (_name, existing) => {
      if (existing) await signupAndGetCookie('bounced@example.com');
      await recordBounce('bounced@example.com');
      const before = (await listEmailDeliveries()).length;

      const res = await request({
        email: 'bounced@example.com',
        turnstile_token: TEST_TURNSTILE_TOKEN,
      });
      expect(res.status).toBe(422);
      expect(await res.json()).toMatchObject({ code: 'email_bounced' });
      expect(await listEmailDeliveries()).toHaveLength(before);
    },
  );

  it('answers a bounced address identically whether or not an account exists (API-009)', async () => {
    await signupAndGetCookie('taken@example.com');
    await recordBounce('taken@example.com');
    await recordBounce('fresh@example.com');

    const existing = await request({
      email: 'taken@example.com',
      turnstile_token: TEST_TURNSTILE_TOKEN,
    });
    const fresh = await request({
      email: 'fresh@example.com',
      turnstile_token: TEST_TURNSTILE_TOKEN,
    });
    expect(await snapshot(existing)).toEqual(await snapshot(fresh));
  });

  it('does not treat a temporary bounce as permanent (API-009)', async () => {
    await recordBounce('flaky@example.com', 'temporary');
    const res = await request({
      email: 'flaky@example.com',
      turnstile_token: TEST_TURNSTILE_TOKEN,
    });
    expect(res.status).toBe(204);
    expect(await deliveriesOfKind('email_verification')).toHaveLength(1);
  });
});

describe('POST /signup (API-010)', () => {
  it('returns a session cookie that works, and omits name when none was given (API-010)', async () => {
    const token = await requestSignupToken('cookie@example.com');
    const res = await postJson('/signup', await signupBody('cookie@example.com', token));
    expect(res.status).toBe(200);

    const setCookie = res.headers.get('set-cookie') ?? '';
    expect(setCookie).toMatch(/refresh_token=wst_/);
    expect(setCookie).toMatch(/HttpOnly/i);

    const body = (await res.json()) as { user: Record<string, unknown> };
    expect(Object.keys(body)).toEqual(['user']);
    expect(body.user).toEqual({
      id: expect.stringMatching(/^[0-9a-f-]{36}$/),
      email: 'cookie@example.com',
      email_verified: true,
    });

    const me = await SELF.fetch(`${BASE}/user`, { headers: authHeaders(cookieFrom(res)!) });
    expect(await me.json()).toMatchObject({ id: body.user.id, email: 'cookie@example.com' });
  });

  it('creates the account with weekly email frequency and the stored key material (API-010)', async () => {
    const token = await requestSignupToken('fresh@example.com');
    const res = await postJson('/signup', await signupBody('fresh@example.com', token));
    const me = await SELF.fetch(`${BASE}/user`, { headers: authHeaders(cookieFrom(res)!) });
    expect(await me.json()).toMatchObject({
      email_verified: true,
      settings: { email_frequency: 'weekly', timezone: 'UTC' },
      pub_key: await publicKeyFor('fresh@example.com'),
      encrypted_priv_key: privateKeyFor('fresh@example.com'),
    });
    expect(await canLogIn('fresh@example.com')).toBe(true);
  });

  it('accepts the optional email_digest_minutes_utc field (API-010)', async () => {
    const token = await requestSignupToken('digest@example.com');
    const res = await postJson(
      '/signup',
      await signupBody('digest@example.com', token, { email_digest_minutes_utc: 480 }),
    );
    expect(res.status).toBe(200);
  });

  it('signs up the lowercase form of the requested address (API-010)', async () => {
    const token = await requestSignupToken('Mixed.Case@Example.COM');
    const res = await postJson('/signup', await signupBody('mixed.case@example.com', token));
    expect(((await res.json()) as { user: { email: string } }).user.email).toBe(
      'mixed.case@example.com',
    );
    expect(await canLogIn('MIXED.CASE@example.com')).toBe(true);
  });

  const tokenCases: TokenCase[] = [
    ...unknownTokens,
    ['a password-reset token', async () => resetLinkToken('someone@example.com')],
    [
      'an expired token',
      async () => {
        const token = await requestSignupToken('new@example.com');
        await expireEmailTokens('new@example.com');
        return token;
      },
    ],
    [
      'a token whose email has since registered',
      async () => {
        const token = await requestSignupToken('new@example.com');
        await signupAndGetCookie('new@example.com', 'someone-else');
        return token;
      },
    ],
    [
      'an already used token',
      async () => {
        const token = await requestSignupToken('new@example.com');
        const first = await postJson('/signup', await signupBody('new@example.com', token));
        expect(first.status).toBe(200);
        return token;
      },
    ],
  ];

  it.each(tokenCases)(
    'rejects %s with 400 and creates no account (API-010)',
    async (_name, make) => {
      const token = await make();
      const res = await postJson(
        '/signup',
        await signupBody('new@example.com', token, {
          password_auth: await passwordAuthFor('mine'),
        }),
      );
      expect(res.status).toBe(400);
      expect(res.headers.get('set-cookie')).toBeNull();
      expect(await canLogIn('new@example.com', 'mine')).toBe(false);
    },
  );

  it('burns a token used on a successful signup so a second signup cannot take over the account (API-010)', async () => {
    const token = await requestSignupToken('once@example.com');
    expect((await postJson('/signup', await signupBody('once@example.com', token))).status).toBe(
      200,
    );
    const second = await postJson(
      '/signup',
      await signupBody('once@example.com', token, { password_auth: await passwordAuthFor('evil') }),
    );
    expect(second.status).toBeGreaterThanOrEqual(400);
    expect(await canLogIn('once@example.com')).toBe(true);
    expect(await canLogIn('once@example.com', 'evil')).toBe(false);
  });

  it.each([
    ['password_auth that is not 32 bytes', { password_auth: Buffer.alloc(16).toString('base64') }],
    ['password_auth that is not base64', { password_auth: '***' }],
    ['password_salt of the wrong length', { password_salt: Buffer.alloc(8).toString('base64') }],
    ['pub_key of the wrong length', { pub_key: Buffer.alloc(31).toString('base64') }],
    ['an empty encrypted_priv_key', { encrypted_priv_key: '' }],
  ])('rejects %s with 400 and creates no account (API-010)', async (_name, override) => {
    const token = await requestSignupToken('badkeys@example.com');
    const res = await postJson('/signup', await signupBody('badkeys@example.com', token, override));
    expect(res.status).toBe(400);
    expect(res.headers.get('set-cookie')).toBeNull();
    expect(await canLogIn('badkeys@example.com')).toBe(false);
  });
});

describe('POST /signup/validate (API-011)', () => {
  const validate = (token: string) => postJson('/signup/validate', { token });

  it('returns the lowercase email and does not consume the token (API-011)', async () => {
    const token = await requestSignupToken('Pending@Example.com');
    for (let i = 0; i < 2; i += 1) {
      const res = await validate(token);
      expect(res.status).toBe(200);
      expect(await res.json()).toEqual({ email: 'pending@example.com' });
    }
    const signup = await postJson('/signup', await signupBody('pending@example.com', token));
    expect(signup.status).toBe(200);
  });

  it.each([
    ...unknownTokens,
    ['a password-reset token', async () => resetLinkToken('someone@example.com')],
    [
      'an expired token',
      async () => {
        const token = await requestSignupToken('new@example.com');
        await expireEmailTokens('new@example.com');
        return token;
      },
    ],
    [
      'an already used token',
      async () => {
        const token = await requestSignupToken('new@example.com');
        await postJson('/signup', await signupBody('new@example.com', token));
        return token;
      },
    ],
  ] as TokenCase[])('answers %s with 400 (API-011)', async (_name, make) => {
    const res = await validate(await make());
    expect(res.status).toBe(400);
    expect(await res.json()).toMatchObject({ error: expect.any(String) });
  });
});

describe('POST /password-reset (API-012)', () => {
  it('emails a reset link to an existing account, matching the address case-insensitively (API-012)', async () => {
    await signupAndGetCookie('reset@example.com');
    const res = await postPasswordReset('RESET@Example.com');
    expect(res.status).toBe(204);
    expect(await res.text()).toBe('');
    const sent = await deliveriesOfKind('password_reset');
    expect(sent.map((d) => d.recipient_email)).toEqual(['reset@example.com']);
  });

  it('answers an unknown address with the same response and sends no reset link (API-012)', async () => {
    await signupAndGetCookie('reset@example.com');
    const known = await postPasswordReset('reset@example.com');
    const unknown = await postPasswordReset('nobody@example.com');
    expect(await snapshot(unknown)).toEqual(await snapshot(known));
    expect(
      (await deliveriesOfKind('password_reset')).map((delivery) => delivery.recipient_email),
    ).toEqual(['reset@example.com']);
  });

  it('sends no email of any kind to an unknown address (API-012)', async () => {
    await postPasswordReset('nobody@example.com');
    expect(await listEmailDeliveries()).toEqual([]);
  });

  it('answers the same when the reset email cannot be sent, and logs the failure (API-012)', async () => {
    await signupAndGetCookie('reset@example.com');
    const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
    // There is no network in tests, so a real SES send fails.
    const failed = await postPasswordReset('reset@example.com', { EMAIL_DELIVERY_MODE: 'ses' });
    const unknown = await postPasswordReset('nobody@example.com');
    expect(await snapshot(failed)).toEqual(await snapshot(unknown));
    expect(logged).toHaveBeenCalledWith(
      '[password-reset] failed to send reset email',
      expect.anything(),
    );
    logged.mockRestore();
  });
});

/** Tokens that must not work as a password-reset token. */
const notResetTokens: TokenCase[] = [
  ...unknownTokens,
  ['an unknown token with the reset prefix', async () => `prt_${'x'.repeat(40)}`],
  ['a signup token', async () => signupLinkToken('new@example.com')],
  ['an email-verification token', async () => unverifiedUserToken('unverified@example.com')],
  [
    'an expired token',
    async () => {
      await signupAndGetCookie('reset@example.com');
      const token = await requestPasswordResetToken('reset@example.com');
      await expireEmailTokens('reset@example.com');
      return token;
    },
  ],
  [
    'a token for an account that was deleted',
    async () => {
      const { cookie } = await signupAndGetCookie('reset@example.com');
      const token = await requestPasswordResetToken('reset@example.com');
      await SELF.fetch(`${BASE}/user?confirm_email=reset%40example.com`, {
        method: 'DELETE',
        headers: authHeaders(cookie),
      });
      return token;
    },
  ],
];

describe('POST /password-reset/validate (API-013)', () => {
  const validate = (token: string) => postJson('/password-reset/validate', { token });

  it('returns the account email and does not consume the token (API-013)', async () => {
    await signupAndGetCookie('reset@example.com');
    const token = await requestPasswordResetToken('reset@example.com');
    for (let i = 0; i < 2; i += 1) {
      const res = await validate(token);
      expect(res.status).toBe(200);
      expect(await res.json()).toMatchObject({ email: 'reset@example.com' });
    }
  });

  const usedToken: TokenCase = [
    'a token that was already used',
    async () => {
      await signupAndGetCookie('reset@example.com');
      const token = await requestPasswordResetToken('reset@example.com');
      const done = await postJson('/password-reset/finalize', {
        token,
        ...(await newPasswordMaterial('reset@example.com', 'brand-new')),
      });
      expect(done.status).toBeLessThan(300);
      return token;
    },
  ];

  it.each([...notResetTokens, usedToken])('answers %s with 400 (API-013)', async (_name, make) => {
    const res = await validate(await make());
    expect(res.status).toBe(400);
    expect(await res.json()).not.toHaveProperty('email');
  });
});

describe('POST /password-reset/finalize (API-014)', () => {
  const finalize = async (token: string, email = 'reset@example.com', password = 'brand-new') =>
    postJson('/password-reset/finalize', {
      token,
      ...(await newPasswordMaterial(email, password)),
    });

  it('sets the new password, salt and key pair, and the old password stops working (API-014)', async () => {
    await signupAndGetCookie('reset@example.com', 'old-password');
    const token = await requestPasswordResetToken('reset@example.com');
    const res = await finalize(token);
    expect(res.status).toBeLessThan(300);

    expect(await canLogIn('reset@example.com', 'old-password')).toBe(false);
    expect(await canLogIn('reset@example.com', 'brand-new')).toBe(true);

    const material = await SELF.fetch(`${BASE}/user/login-material?email=reset%40example.com`);
    expect(((await material.json()) as { password_salt: string }).password_salt).toBe(
      (await newPasswordMaterial('reset@example.com', 'brand-new')).password_salt,
    );
    const me = await SELF.fetch(`${BASE}/user`, {
      headers: authHeaders(await loginCookie('reset@example.com', 'brand-new')),
    });
    expect(await me.json()).toMatchObject({
      pub_key: await publicKeyFor('reset@example.com:new'),
      encrypted_priv_key: privateKeyFor('reset@example.com:new'),
    });
  });

  it('responds 204 with no body (API-014)', async () => {
    await signupAndGetCookie('reset@example.com');
    const res = await finalize(await requestPasswordResetToken('reset@example.com'));
    expect(res.status).toBe(204);
    expect(await res.text()).toBe('');
  });

  it('can be used once: a second finalize with the same token changes nothing (API-014)', async () => {
    await signupAndGetCookie('reset@example.com', 'old-password');
    const token = await requestPasswordResetToken('reset@example.com');
    expect((await finalize(token, 'reset@example.com', 'first-reset')).status).toBe(204);

    const second = await finalize(token, 'reset@example.com', 'attacker');
    expect(second.status).toBe(400);
    expect(await canLogIn('reset@example.com', 'first-reset')).toBe(true);
    expect(await canLogIn('reset@example.com', 'attacker')).toBe(false);
  });

  it.each(notResetTokens)(
    'answers %s with 400 and changes nothing (API-014)',
    async (_name, make) => {
      await signupAndGetCookie('victim@example.com', 'old-password');
      const res = await postJson('/password-reset/finalize', {
        token: await make(),
        ...(await newPasswordMaterial('victim@example.com', 'attacker')),
      });
      expect(res.status).toBe(400);
      expect(await canLogIn('victim@example.com', 'old-password')).toBe(true);
      expect(await canLogIn('victim@example.com', 'attacker')).toBe(false);
    },
  );

  it.each([
    ['password_auth that is not 32 bytes', { password_auth: Buffer.alloc(16).toString('base64') }],
    ['password_salt of the wrong length', { password_salt: Buffer.alloc(8).toString('base64') }],
    ['pub_key of the wrong length', { pub_key: Buffer.alloc(31).toString('base64') }],
    ['an empty encrypted_priv_key', { encrypted_priv_key: '' }],
  ])('rejects %s with 400 and changes nothing (API-014)', async (_name, override) => {
    await signupAndGetCookie('reset@example.com', 'old-password');
    const token = await requestPasswordResetToken('reset@example.com');
    const res = await postJson('/password-reset/finalize', {
      token,
      ...(await newPasswordMaterial('reset@example.com', 'brand-new')),
      ...override,
    });
    expect(res.status).toBe(400);
    expect(await canLogIn('reset@example.com', 'old-password')).toBe(true);
    expect(await canLogIn('reset@example.com', 'brand-new')).toBe(false);
  });
});

describe('GET /user/login-material (API-015)', () => {
  const material = (email?: string) =>
    SELF.fetch(
      `${BASE}/user/login-material${email === undefined ? '' : `?email=${encodeURIComponent(email)}`}`,
    );

  it('returns the stored salt for an existing account, matching the email case-insensitively (API-015)', async () => {
    await signupAndGetCookie('alice@example.com');
    const salt = await passwordSaltFor('alice@example.com');
    for (const email of ['alice@example.com', 'ALICE@Example.COM']) {
      const body = (await (await material(email)).json()) as { password_salt: string };
      expect(body.password_salt).toBe(salt);
    }
  });

  it('gives an existing and an unknown email the same status, shape and parameters (API-015)', async () => {
    await signupAndGetCookie('alice@example.com');
    const existing = await material('alice@example.com');
    const unknown = await material('nobody@example.com');
    const bare = await material();

    expect(unknown.status).toBe(existing.status);
    expect(unknown.headers.get('content-type')).toBe(existing.headers.get('content-type'));
    const a = (await existing.json()) as { password_salt: string; params: object };
    const b = (await unknown.json()) as { password_salt: string; params: object };
    expect(Object.keys(b).sort()).toEqual(Object.keys(a).sort());
    expect(b.params).toEqual(a.params);
    expect(b.password_salt).toHaveLength(a.password_salt.length);
    expect(((await bare.json()) as { params: object }).params).toEqual(a.params);
  });

  const saltOf = async (res: Response) =>
    ((await res.json()) as { password_salt: string }).password_salt;

  /** GET /user/login-material straight to the worker, with some env vars overridden. */
  const materialWith = (email: string, bindings: Partial<Env>) =>
    worker.fetch(
      new Request(`${BASE}/user/login-material?email=${encodeURIComponent(email)}`),
      { ...env, ...bindings },
      createExecutionContext(),
    );

  it('returns the same made-up salt each time for an unknown email (API-015)', async () => {
    const first = await saltOf(await material('nobody@example.com'));
    expect(await saltOf(await material('nobody@example.com'))).toBe(first);
  });

  it('matches an unknown email case-insensitively, as for a real account (API-015)', async () => {
    const salt = await saltOf(await material('nobody@example.com'));
    expect(await saltOf(await material('NOBODY@Example.COM'))).toBe(salt);
  });

  it('gives different unknown emails different made-up salts (API-015)', async () => {
    expect(await saltOf(await material('nobody@example.com'))).not.toBe(
      await saltOf(await material('someone@example.com')),
    );
  });

  it('derives the made-up salt from the normalized email and LOGIN_SALT_SECRET (API-015)', async () => {
    const key = await crypto.subtle.importKey(
      'raw',
      new TextEncoder().encode('secret-a'),
      { name: 'HMAC', hash: 'SHA-256' },
      false,
      ['sign'],
    );
    const mac = await crypto.subtle.sign(
      'HMAC',
      key,
      new TextEncoder().encode('nobody@example.com'),
    );
    const salt = await saltOf(
      await materialWith('NOBODY@example.com', { LOGIN_SALT_SECRET: 'secret-a' }),
    );
    expect(salt).toBe(Buffer.from(mac).subarray(0, 16).toString('base64'));
    expect(
      await saltOf(await materialWith('nobody@example.com', { LOGIN_SALT_SECRET: 'secret-b' })),
    ).not.toBe(salt);
  });

  it('still answers and logs an error when LOGIN_SALT_SECRET is not configured (API-015)', async () => {
    await signupAndGetCookie('alice@example.com');
    const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
    const unconfigured = { LOGIN_SALT_SECRET: undefined };

    const unknown = await materialWith('nobody@example.com', unconfigured);
    expect(unknown.status).toBe(200);
    expect(Buffer.from(await saltOf(unknown), 'base64')).toHaveLength(16);

    const existing = await materialWith('alice@example.com', unconfigured);
    expect(existing.status).toBe(200);
    expect(await saltOf(existing)).toBe(await passwordSaltFor('alice@example.com'));

    expect(logged).toHaveBeenCalled();
    logged.mockRestore();
  });

  it('rejects an email that is not an email address with 400 (API-015)', async () => {
    expect((await material('not-an-email')).status).toBe(400);
  });
});

describe('POST /login (API-016)', () => {
  it('answers an unknown email, a wrong password and a malformed password_auth identically (API-016)', async () => {
    await signupAndGetCookie('bob@example.com');
    const wrongPassword = await snapshot(await login('bob@example.com', 'wrong'));
    const unknownEmail = await snapshot(await login('nobody@example.com'));
    const shortAuth = await snapshot(
      await postJson('/login', {
        email: 'bob@example.com',
        password_auth: Buffer.alloc(8).toString('base64'),
      }),
    );
    expect(wrongPassword.status).toBe(401);
    expect(unknownEmail).toEqual(wrongPassword);
    expect(shortAuth).toEqual(wrongPassword);
  });

  it('sets no cookie when the login fails (API-016)', async () => {
    await signupAndGetCookie('bob@example.com');
    expect((await login('bob@example.com', 'wrong')).headers.get('set-cookie')).toBeNull();
  });

  it('matches the email case-insensitively (API-016)', async () => {
    await signupAndGetCookie('bob@example.com');
    expect((await login('BOB@Example.COM')).status).toBe(204);
  });

  it('returns an empty 204 with a fresh HttpOnly web session cookie each time (API-016)', async () => {
    const { cookie: signupCookie } = await signupAndGetCookie('bob@example.com');
    const first = await login('bob@example.com');
    const second = await login('bob@example.com');
    expect(first.status).toBe(204);
    expect(await first.text()).toBe('');
    expect(first.headers.get('set-cookie')).toMatch(/HttpOnly/i);

    const cookies = [signupCookie, cookieFrom(first)!, cookieFrom(second)!];
    expect(cookies[1]).toMatch(/^wst_/);
    expect(new Set(cookies).size).toBe(3);
    for (const cookie of cookies) {
      expect((await SELF.fetch(`${BASE}/user`, { headers: authHeaders(cookie) })).status).toBe(200);
    }
  });

  it('does not save the timezone when the password is wrong (API-016)', async () => {
    const { cookie } = await signupAndGetCookie('tz@example.com', 'password123', undefined, 'UTC');
    await login('tz@example.com', 'wrong', { timezone: 'Asia/Tokyo' });
    const me = await SELF.fetch(`${BASE}/user`, { headers: authHeaders(cookie) });
    expect(((await me.json()) as { settings: { timezone: string } }).settings.timezone).toBe('UTC');
  });

  it('refuses an unverified account with 403, a session-less response and a verification email (API-016)', async () => {
    const token = await unverifiedUserToken('unverified@example.com');
    expect(token).toBeTruthy();
    const res = await login('unverified@example.com');
    expect(res.status).toBe(403);
    expect(res.headers.get('set-cookie')).toBeNull();
    expect(await res.json()).toEqual({
      error: 'Please verify your email before logging in. A verification email has been sent.',
    });
    const verification = (await deliveriesOfKind('email_verification')).at(-1)!;
    expect(verification.recipient_email).toBe('unverified@example.com');
  });

  it('does not reveal that an account is unverified to someone with the wrong password (API-016)', async () => {
    await unverifiedUserToken('unverified@example.com');
    const before = (await listEmailDeliveries()).length;
    const wrong = await login('unverified@example.com', 'wrong');
    expect(wrong.status).toBe(401);
    expect(await listEmailDeliveries()).toHaveLength(before);
  });

  it('answers an unverified account with a permanently bounced address with 403 email_bounced (API-016)', async () => {
    await unverifiedUserToken('unverified@example.com');
    await recordBounce('unverified@example.com');
    const before = (await listEmailDeliveries()).length;

    const res = await login('unverified@example.com');
    expect(res.status).toBe(403);
    expect(await res.json()).toMatchObject({ code: 'email_bounced', error: expect.any(String) });
    expect(await listEmailDeliveries()).toHaveLength(before);
  });
});

describe('POST /email-verification/validate (API-017)', () => {
  const verify = (token: string) => postJson('/email-verification/validate', { token });

  async function emailChangeToken(from: string, to: string) {
    const { cookie } = await signupAndGetCookie(from);
    const res = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(cookie),
      body: JSON.stringify({ email: to }),
    });
    expect(res.status).toBe(200);
    return extractTokenFromDelivery(
      (await deliveriesOfKind('email_verification')).at(-1)!,
      'token',
    )!;
  }

  const refused: TokenCase[] = [
    ['an unknown token', async () => 'not-a-real-token'],
    ['an unknown email-verification token', async () => `evt_${'x'.repeat(40)}`],
    ['an unknown email-change token', async () => `ect_${'x'.repeat(40)}`],
    ['a signup token', async () => signupLinkToken('new@example.com')],
    ['a password-reset token', async () => resetLinkToken('someone@example.com')],
    [
      'an expired email-verification token',
      async () => {
        const token = await unverifiedUserToken('unverified@example.com');
        await expireEmailTokens('unverified@example.com');
        return token;
      },
    ],
    [
      'an expired email-change token',
      async () => {
        const token = await emailChangeToken('old@example.com', 'next@example.com');
        await expireEmailTokens('next@example.com');
        return token;
      },
    ],
    [
      'an already used email-verification token',
      async () => {
        const token = await unverifiedUserToken('unverified@example.com');
        expect((await verify(token)).status).toBe(200);
        return token;
      },
    ],
    [
      'an already used email-change token',
      async () => {
        const token = await emailChangeToken('old@example.com', 'next@example.com');
        expect((await verify(token)).status).toBe(200);
        return token;
      },
    ],
  ];

  it.each(refused)('answers %s with 400 and issues no session (API-017)', async (_name, make) => {
    const res = await verify(await make());
    expect(res.status).toBe(400);
    expect(res.headers.get('set-cookie')).toBeNull();
  });

  it('moves the account to the new address on an email change, and only the new one logs in (API-017)', async () => {
    const token = await emailChangeToken('old@example.com', 'New.Address@Example.com');
    const res = await verify(token);
    expect(res.status).toBe(200);
    expect(await res.json()).toMatchObject({
      email: 'new.address@example.com',
      purpose: 'email_change',
    });
    expect(await canLogIn('new.address@example.com')).toBe(true);
    expect(await canLogIn('old@example.com')).toBe(false);
  });

  it('does not change the address when someone registered it after the change was requested (API-017)', async () => {
    const token = await emailChangeToken('old@example.com', 'contested@example.com');
    await signupAndGetCookie('contested@example.com', 'their-password');

    const res = await verify(token);
    expect(res.status).toBeGreaterThanOrEqual(400);
    expect(res.status).toBeLessThan(500);
    expect(await canLogIn('old@example.com')).toBe(true);
    expect(await canLogIn('contested@example.com', 'their-password')).toBe(true);
    expect(await canLogIn('contested@example.com')).toBe(false);
  });

  it('marks an unverified account verified and reports the verification purpose (API-017)', async () => {
    const token = await unverifiedUserToken('unverified@example.com');
    const res = await verify(token);
    expect(res.status).toBe(200);
    expect(await res.json()).toMatchObject({
      email: 'unverified@example.com',
      purpose: 'email_verification',
    });
    const me = await SELF.fetch(`${BASE}/user`, { headers: authHeaders(cookieFrom(res)!) });
    expect(await me.json()).toMatchObject({ email_verified: true });
  });
});
