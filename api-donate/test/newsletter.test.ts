import { env, SELF } from 'cloudflare:test';
import { beforeEach, describe, expect, it } from 'vitest';
import { requestSubscription } from '../src/lib/db';
import { signNewsletterToken, verifyNewsletterToken } from '../src/lib/newsletter-token';
import { BASE, clearDB } from './helpers';

beforeEach(clearDB);

const SECRET = 'newsletter-test-secret';
const row = (email = 'alice@example.com') =>
  env.DB.prepare('SELECT * FROM newsletter_subscribers WHERE email = ?').bind(email).first();

function post(body: Record<string, string>) {
  return SELF.fetch(`${BASE}/newsletter`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams(body),
    redirect: 'manual',
  });
}

describe('newsletter tokens', () => {
  it('round-trips and is bound to its purpose', async () => {
    const token = await signNewsletterToken(SECRET, 'confirm', 'a+b@example.com');
    expect(await verifyNewsletterToken(SECRET, 'confirm', token)).toBe('a+b@example.com');
    expect(await verifyNewsletterToken(SECRET, 'unsubscribe', token)).toBeNull();
    expect(await verifyNewsletterToken('other-secret', 'confirm', token)).toBeNull();
    expect(await verifyNewsletterToken(SECRET, 'confirm', 'garbage')).toBeNull();
  });
});

describe('POST /newsletter', () => {
  it('records the subscriber unconfirmed and redirects', async () => {
    const res = await post({ email: ' Alice@Example.com ' });

    expect(res.status).toBe(303);
    expect(res.headers.get('location')).toMatch(/\/newsletter\/check-email$/);
    expect(await row()).toMatchObject({
      source: 'landing',
      confirmed_at: null,
      unsubscribed_at: null,
    });
  });

  it('rejects an invalid email', async () => {
    const res = await post({ email: 'not-an-email' });
    expect(res.status).toBe(400);
    expect(await row('not-an-email')).toBeNull();
  });
});

describe('requestSubscription', () => {
  const sub = { email: 'alice@example.com', country: 'US', regionCode: 'CA', region: 'California' };

  it('sends one confirmation per hour and keeps the first region', async () => {
    expect(await requestSubscription(env.DB, sub)).toBe(true);
    expect(await requestSubscription(env.DB, { ...sub, regionCode: 'NY' })).toBe(false);
    expect(await row()).toMatchObject({ region_code: 'CA' });

    await env.DB.prepare(
      'UPDATE newsletter_subscribers SET confirmation_sent_at = unixepoch() - 7200',
    ).run();
    expect(await requestSubscription(env.DB, sub)).toBe(true);
  });

  it('does not email an address that is already subscribed', async () => {
    await requestSubscription(env.DB, sub);
    await env.DB.prepare(
      'UPDATE newsletter_subscribers SET confirmed_at = unixepoch(), confirmation_sent_at = NULL',
    ).run();
    expect(await requestSubscription(env.DB, sub)).toBe(false);
  });

  it('asks an unsubscribed address to confirm again', async () => {
    await requestSubscription(env.DB, sub);
    await env.DB.prepare(
      'UPDATE newsletter_subscribers SET confirmed_at = unixepoch(), unsubscribed_at = unixepoch(), confirmation_sent_at = NULL',
    ).run();
    expect(await requestSubscription(env.DB, sub)).toBe(true);
  });
});

describe('confirm and unsubscribe links', () => {
  beforeEach(async () => {
    await post({ email: 'alice@example.com' });
  });

  it('confirms with a valid token and rejects a bad one', async () => {
    const bad = await SELF.fetch(`${BASE}/newsletter/confirm?token=nope`, { redirect: 'manual' });
    expect(bad.status).toBe(400);
    expect(await row()).toMatchObject({ confirmed_at: null });

    const token = await signNewsletterToken(SECRET, 'confirm', 'alice@example.com');
    const res = await SELF.fetch(`${BASE}/newsletter/confirm?token=${token}`, {
      redirect: 'manual',
    });
    expect(res.status).toBe(303);
    expect(res.headers.get('location')).toMatch(/\/newsletter\/confirmed$/);
    expect((await row())?.confirmed_at).toBeTypeOf('number');
  });

  it('only unsubscribes on POST, and re-confirming resubscribes', async () => {
    const confirm = await signNewsletterToken(SECRET, 'confirm', 'alice@example.com');
    await SELF.fetch(`${BASE}/newsletter/confirm?token=${confirm}`, { redirect: 'manual' });
    const token = await signNewsletterToken(SECRET, 'unsubscribe', 'alice@example.com');
    const url = `${BASE}/newsletter/unsubscribe?token=${token}`;

    const preview = await SELF.fetch(url);
    expect(preview.status).toBe(200);
    expect(await preview.text()).toContain('<form method="post"');
    expect(await row()).toMatchObject({ unsubscribed_at: null });

    // RFC 8058 one-click body.
    const res = await SELF.fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: 'List-Unsubscribe=One-Click',
    });
    expect(res.status).toBe(200);
    expect((await row())?.unsubscribed_at).toBeTypeOf('number');

    await SELF.fetch(`${BASE}/newsletter/confirm?token=${confirm}`, { redirect: 'manual' });
    expect(await row()).toMatchObject({ unsubscribed_at: null });
  });

  it('rejects a confirm token at the unsubscribe endpoint', async () => {
    const confirm = await signNewsletterToken(SECRET, 'confirm', 'alice@example.com');
    const res = await SELF.fetch(`${BASE}/newsletter/unsubscribe?token=${confirm}`, {
      method: 'POST',
    });
    expect(res.status).toBe(400);
  });
});
