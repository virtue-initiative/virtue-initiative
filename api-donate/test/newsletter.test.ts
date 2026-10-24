import { env, fetchMock, SELF } from 'cloudflare:test';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import app from '../src/index';
import { confirmSubscriber, requestSubscription } from '../src/lib/db';
import { signNewsletterToken, verifyNewsletterToken } from '../src/lib/newsletter-token';
import { BASE, clearDB } from './helpers';

const SECRET = 'newsletter-test-secret';
const LANDING = 'http://localhost:4321';
const EMAIL = 'alice@example.com';

const row = (email = EMAIL) =>
  env.DB.prepare('SELECT * FROM newsletter_subscribers WHERE email = ?').bind(email).first();

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
});

beforeEach(clearDB);

// A leftover intercept would leak into the next test, so every one registered must be used.
afterEach(() => fetchMock.assertNoPendingInterceptors());

/** Expects one Turnstile verification and answers it. Returns what the worker sent. */
function turnstile(success: boolean, extra: Record<string, string> = {}) {
  const seen: { body?: Record<string, string> } = {};
  fetchMock
    .get('https://challenges.cloudflare.com')
    .intercept({ path: '/turnstile/v0/siteverify', method: 'POST' })
    .reply((opts) => {
      seen.body = JSON.parse(String(opts.body));
      return { statusCode: 200, data: JSON.stringify({ success, ...extra }) };
    });
  return seen;
}

// The per-IP limiter binding keeps its counts across tests, so each post comes from a new IP.
let ipCounter = 0;
const nextIp = () => `203.0.113.${++ipCounter}`;

function post(fields: Record<string, string>, ip = nextIp()) {
  return SELF.fetch(`${BASE}/newsletter`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded', 'CF-Connecting-IP': ip },
    body: new URLSearchParams(fields),
    redirect: 'manual',
  });
}

const signup = (email = EMAIL) => ({ email, 'cf-turnstile-response': 'token' });

/** Runs the app with some bindings replaced and waits for background work (waitUntil). */
async function postWith(overrides: Record<string, unknown>, fields = signup()) {
  const pending: Promise<unknown>[] = [];
  const ctx = {
    waitUntil: (p: Promise<unknown>) => pending.push(p),
    passThroughOnException() {},
  } as unknown as ExecutionContext;
  const res = await app.fetch(
    new Request(`${BASE}/newsletter`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/x-www-form-urlencoded',
        'CF-Connecting-IP': nextIp(),
      },
      body: new URLSearchParams(fields),
    }),
    { ...env, ...overrides },
    ctx,
  );
  await Promise.all(pending);
  return res;
}

const confirmToken = async (email = EMAIL) =>
  signNewsletterToken(SECRET, 'confirm', email, (await row(email))!.confirmation_sent_at as number);
const unsubscribeToken = (email = EMAIL) => signNewsletterToken(SECRET, 'unsubscribe', email);

const confirmPost = (token: string) =>
  SELF.fetch(`${BASE}/newsletter/confirm?token=${token}`, { method: 'POST', redirect: 'manual' });
const unsubscribePost = (token: string) =>
  SELF.fetch(`${BASE}/newsletter/unsubscribe?token=${token}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: 'List-Unsubscribe=One-Click',
  });

describe('newsletter tokens', () => {
  it('round-trips and is bound to its purpose, address and issue time', async () => {
    const token = await signNewsletterToken(SECRET, 'confirm', 'a+b@example.com', 1700000000);
    expect(await verifyNewsletterToken(SECRET, 'confirm', token)).toEqual({
      email: 'a+b@example.com',
      issuedAt: 1700000000,
    });
    expect(await verifyNewsletterToken(SECRET, 'unsubscribe', token)).toBeNull();
    expect(await verifyNewsletterToken('other-secret', 'confirm', token)).toBeNull();
    expect(await verifyNewsletterToken(SECRET, 'confirm', 'garbage')).toBeNull();

    // Changing the issue time invalidates the signature.
    const [email, , signature] = token.split('.');
    expect(
      await verifyNewsletterToken(SECRET, 'confirm', `${email}.1800000000.${signature}`),
    ).toBeNull();
  });
});

describe('POST /newsletter', () => {
  it('records the subscriber unconfirmed, checks Turnstile and redirects', async () => {
    const seen = turnstile(true);
    const res = await post(signup(' Alice@Example.com '), '198.51.100.9');

    expect(res.status).toBe(303);
    expect(res.headers.get('location')).toBe(`${LANDING}/newsletter/check-email`);
    expect(seen.body).toMatchObject({
      secret: 'turnstile-test-secret',
      response: 'token',
      remoteip: '198.51.100.9',
    });
    const r = await row();
    expect(r).toMatchObject({ source: 'landing', confirmed_at: null, unsubscribed_at: null });
    expect(r!.confirmation_sent_at).toBeTypeOf('number');
  });

  it('sends a bad address back to the form without touching Turnstile or the database', async () => {
    const res = await post(signup('not-an-email'));
    expect(res.status).toBe(303);
    expect(res.headers.get('location')).toBe(`${LANDING}/#newsletter`);
    expect(await row('not-an-email')).toBeNull();
  });

  it('rejects a post that fails Turnstile, without recording the address', async () => {
    turnstile(false);
    const res = await post(signup());
    expect(res.headers.get('location')).toBe(`${LANDING}/newsletter/error`);
    expect(await row()).toBeNull();
  });

  it('rejects a post with no Turnstile token, without asking Cloudflare', async () => {
    const res = await post({ email: EMAIL });
    expect(res.headers.get('location')).toBe(`${LANDING}/newsletter/error`);
    expect(await row()).toBeNull();
  });

  it('fails closed when the Turnstile secret is not configured', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const res = await postWith({ TURNSTILE_SECRET_KEY: undefined });
    expect(res.headers.get('location')).toBe(`${LANDING}/newsletter/error`);
    expect(await row()).toBeNull();
    error.mockRestore();
  });

  describe('with TURNSTILE_HOSTNAMES set', () => {
    const hostnames = { TURNSTILE_HOSTNAMES: 'virtueinitiative.org,staging.virtueinitiative.org' };
    const ok = { action: 'newsletter', hostname: 'staging.virtueinitiative.org' };

    it('accepts a token for the newsletter action on a landing host', async () => {
      turnstile(true, ok);
      const res = await postWith(hostnames);
      expect(res.headers.get('location')).toBe(`${LANDING}/newsletter/check-email`);
    });

    it.each([
      ['another action', { ...ok, action: 'login' }],
      ['another hostname', { ...ok, hostname: 'evil.example' }],
      ['no action or hostname', {}],
    ])('rejects a token with %s', async (_name, extra) => {
      turnstile(true, extra);
      const res = await postWith(hostnames);
      expect(res.headers.get('location')).toBe(`${LANDING}/newsletter/error`);
      expect(await row()).toBeNull();
    });
  });

  it('treats missing AWS credentials as a send failure outside local dev, logging no address', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const log = vi.spyOn(console, 'log').mockImplementation(() => {});
    turnstile(true);
    await postWith({ LANDING_URL: 'https://virtueinitiative.org' });
    expect((await row())!.confirmation_sent_at).toBeNull();
    const logged = JSON.stringify(
      [...error.mock.calls, ...log.mock.calls].map((c) => String(c[0])),
    );
    expect(logged).not.toContain(EMAIL);
    expect(logged).not.toContain('/newsletter/confirm');
    error.mockRestore();
    log.mockRestore();
  });

  it('rejects a post over the per-IP rate limit before doing anything else', async () => {
    const res = await postWith({
      NEWSLETTER_RATE_LIMITER: { limit: async () => ({ success: false }) },
    });
    expect(res.headers.get('location')).toBe(`${LANDING}/newsletter/error`);
    expect(await row()).toBeNull();
  });

  it('stops sending confirmations once the daily cap is reached', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    turnstile(true);
    const first = await postWith({ NEWSLETTER_DAILY_CONFIRMATION_CAP: '1' });
    expect(first.headers.get('location')).toBe(`${LANDING}/newsletter/check-email`);

    turnstile(true);
    const second = await postWith(
      { NEWSLETTER_DAILY_CONFIRMATION_CAP: '1' },
      signup('bob@example.com'),
    );
    expect(second.headers.get('location')).toBe(`${LANDING}/newsletter/error`);
    expect((await row('bob@example.com'))!.confirmation_sent_at).toBeNull();
    error.mockRestore();
  });

  it('redirects to the error page instead of returning a 500 when the database fails', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    turnstile(true);
    const res = await postWith({
      DB: {
        prepare: () => {
          throw new Error('D1 is down');
        },
      },
    });
    expect(res.status).toBe(303);
    expect(res.headers.get('location')).toBe(`${LANDING}/newsletter/error`);
    error.mockRestore();
  });

  it('gives the claim back when SES fails, so a retry is not blocked for an hour', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    fetchMock
      .get('https://email.us-east-1.amazonaws.com')
      .intercept({ path: '/v2/email/outbound-emails', method: 'POST' })
      .reply(400, JSON.stringify({ message: 'Email address is not verified.' }), {
        headers: { 'content-type': 'application/json', 'x-amzn-errortype': 'BadRequestException' },
      });
    turnstile(true);

    const res = await postWith({ AWS_ACCESS_KEY_ID: 'key', AWS_SECRET_ACCESS_KEY: 'secret' });

    expect(res.headers.get('location')).toBe(`${LANDING}/newsletter/check-email`);
    expect((await row())!.confirmation_sent_at).toBeNull();
    expect(error).toHaveBeenCalledWith(
      expect.stringContaining('failed to send confirmation'),
      expect.anything(),
    );
    error.mockRestore();
  });

  it('never emails a suppressed address, and does not look different to the visitor', async () => {
    await env.DB.prepare(
      "INSERT INTO newsletter_subscribers (email, source, confirmed_at, suppressed_at) VALUES (?, 'landing', 1, 2)",
    )
      .bind(EMAIL)
      .run();
    turnstile(true);
    const res = await post(signup());
    expect(res.headers.get('location')).toBe(`${LANDING}/newsletter/check-email`);
    expect((await row())!.confirmation_sent_at).toBeNull();
  });
});

describe('requestSubscription', () => {
  const sub = { email: EMAIL, country: 'US', regionCode: 'CA', region: 'California' };

  it('sends one confirmation per hour and keeps the first region', async () => {
    expect((await requestSubscription(env.DB, sub, 100)).status).toBe('send');
    expect(await requestSubscription(env.DB, { ...sub, regionCode: 'NY' }, 100)).toEqual({
      status: 'skipped',
    });
    expect(await row()).toMatchObject({ region_code: 'CA' });

    await env.DB.prepare(
      'UPDATE newsletter_subscribers SET confirmation_sent_at = unixepoch() - 7200',
    ).run();
    expect((await requestSubscription(env.DB, sub, 100)).status).toBe('send');
  });

  it('does not email an address that is already subscribed', async () => {
    await env.DB.prepare(
      "INSERT INTO newsletter_subscribers (email, source, confirmed_at) VALUES (?, 'landing', 1)",
    )
      .bind(EMAIL)
      .run();
    expect((await requestSubscription(env.DB, sub, 100)).status).toBe('skipped');
  });

  it('asks an address that unsubscribed to confirm again', async () => {
    await env.DB.prepare(
      "INSERT INTO newsletter_subscribers (email, source, confirmed_at, unsubscribed_at) VALUES (?, 'landing', 1, 2)",
    )
      .bind(EMAIL)
      .run();
    expect((await requestSubscription(env.DB, sub, 100)).status).toBe('send');
  });

  it('lets an unsubscribed address ask once right away, then waits a day', async () => {
    await env.DB.prepare(
      "INSERT INTO newsletter_subscribers (email, source, confirmed_at, unsubscribed_at, confirmation_sent_at) VALUES (?, 'landing', 1, unixepoch() - 60, unixepoch() - 600)",
    )
      .bind(EMAIL)
      .run();
    expect((await requestSubscription(env.DB, sub, 100)).status).toBe('send');
    expect((await requestSubscription(env.DB, sub, 100)).status).toBe('skipped');

    await env.DB.prepare(
      'UPDATE newsletter_subscribers SET unsubscribed_at = unixepoch() - 10000, confirmation_sent_at = unixepoch() - 7200',
    ).run();
    expect((await requestSubscription(env.DB, sub, 100)).status).toBe('skipped');

    await env.DB.prepare(
      'UPDATE newsletter_subscribers SET confirmation_sent_at = unixepoch() - 90000',
    ).run();
    expect((await requestSubscription(env.DB, sub, 100)).status).toBe('send');
  });

  it('keeps the original confirmed_at when the same link is confirmed again', async () => {
    await requestSubscription(env.DB, sub, 100);
    const sentAt = (await row())!.confirmation_sent_at as number;
    await env.DB.prepare('UPDATE newsletter_subscribers SET confirmed_at = 5').run();
    expect(await confirmSubscriber(env.DB, EMAIL, sentAt)).toBe(true);
    expect(await row()).toMatchObject({ confirmed_at: 5 });
  });

  it('never asks a suppressed address', async () => {
    await env.DB.prepare(
      "INSERT INTO newsletter_subscribers (email, source, confirmed_at, unsubscribed_at, suppressed_at) VALUES (?, 'landing', 1, 2, 3)",
    )
      .bind(EMAIL)
      .run();
    expect((await requestSubscription(env.DB, sub, 100)).status).toBe('skipped');
  });

  it('stops at the daily cap', async () => {
    expect((await requestSubscription(env.DB, sub, 1)).status).toBe('send');
    expect(
      (await requestSubscription(env.DB, { ...sub, email: 'bob@example.com' }, 1)).status,
    ).toBe('capped');
  });
});

describe('confirm links', () => {
  beforeEach(async () => {
    turnstile(true);
    await post(signup());
  });

  it('only shows a button on GET, so a mail scanner cannot confirm anyone', async () => {
    const token = await confirmToken();
    const res = await SELF.fetch(`${BASE}/newsletter/confirm?token=${token}`);
    expect(res.status).toBe(200);
    expect(await res.text()).toContain('<form method="post"');
    expect(await row()).toMatchObject({ confirmed_at: null });
  });

  it('confirms on POST and redirects to the landing site', async () => {
    const res = await confirmPost(await confirmToken());
    expect(res.status).toBe(303);
    expect(res.headers.get('location')).toBe(`${LANDING}/newsletter/confirmed`);
    expect((await row())!.confirmed_at).toBeTypeOf('number');
  });

  it('rejects garbage, and a confirm token used at the unsubscribe endpoint', async () => {
    expect((await confirmPost('nope')).status).toBe(400);
    expect((await SELF.fetch(`${BASE}/newsletter/confirm?token=nope`)).status).toBe(400);
    expect((await unsubscribePost(await confirmToken())).status).toBe(400);
    expect(await row()).toMatchObject({ confirmed_at: null, unsubscribed_at: null });
  });

  it('does not resubscribe someone who unsubscribed when their old link is opened again', async () => {
    const token = await confirmToken();
    await confirmPost(token);
    await unsubscribePost(await unsubscribeToken());
    expect((await row())!.unsubscribed_at).toBeTypeOf('number');

    // A mail scanner or a forward replays the original link.
    expect((await confirmPost(token)).status).toBe(400);
    const preview = await SELF.fetch(`${BASE}/newsletter/confirm?token=${token}`);
    expect(preview.status).toBe(400);
    expect((await row())!.unsubscribed_at).toBeTypeOf('number');
  });

  it('lets an unsubscribed address back in through a fresh confirmation email', async () => {
    await confirmPost(await confirmToken());
    await unsubscribePost(await unsubscribeToken());
    // Make the unsubscribe and the old confirmation older than the new email.
    await env.DB.prepare(
      'UPDATE newsletter_subscribers SET unsubscribed_at = unixepoch() - 7200, confirmation_sent_at = unixepoch() - 7300',
    ).run();

    turnstile(true);
    await post(signup());
    const fresh = await confirmToken();
    expect((await confirmPost(fresh)).status).toBe(303);
    expect(await row()).toMatchObject({ unsubscribed_at: null });
  });

  it('only honours the link from the latest confirmation email', async () => {
    // The first email went out two hours ago, so the form may send another.
    await env.DB.prepare(
      'UPDATE newsletter_subscribers SET confirmation_sent_at = unixepoch() - 7200',
    ).run();
    const older = await confirmToken();
    turnstile(true);
    await post(signup());

    expect((await confirmPost(older)).status).toBe(400);
    expect((await confirmPost(await confirmToken())).status).toBe(303);
  });

  it('cannot lift a hard-bounce or complaint suppression', async () => {
    const token = await confirmToken();
    await env.DB.prepare('UPDATE newsletter_subscribers SET suppressed_at = unixepoch()').run();
    expect((await confirmPost(token)).status).toBe(400);
    expect(await row()).toMatchObject({ confirmed_at: null });
  });
});

describe('unsubscribe links', () => {
  beforeEach(async () => {
    await env.DB.prepare(
      "INSERT INTO newsletter_subscribers (email, source, confirmed_at) VALUES (?, 'landing', 1)",
    )
      .bind(EMAIL)
      .run();
  });

  it('only unsubscribes on POST', async () => {
    const url = `${BASE}/newsletter/unsubscribe?token=${await unsubscribeToken()}`;
    const preview = await SELF.fetch(url);
    expect(preview.status).toBe(200);
    expect(await preview.text()).toContain('<form method="post"');
    expect(await row()).toMatchObject({ unsubscribed_at: null });

    const res = await unsubscribePost(await unsubscribeToken());
    expect(res.status).toBe(200);
    expect((await row())!.unsubscribed_at).toBeTypeOf('number');
  });

  it('rejects a bad token', async () => {
    expect((await unsubscribePost('bad.0.bad')).status).toBe(400);
  });
});
