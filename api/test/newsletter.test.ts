import { beforeEach, describe, expect, it } from 'vitest';
import { env, SELF } from 'cloudflare:test';
import {
  BASE,
  clearDB,
  extractTokenFromDelivery,
  listEmailDeliveries,
  passwordAuthFor,
  passwordSaltFor,
  privateKeyFor,
  publicKeyFor,
  signupAndGetCookie,
  TEST_TURNSTILE_TOKEN,
} from './helpers';

const LANDING_URL = env.LANDING_URL;
const CALIFORNIA = { country: 'US', regionCode: 'CA' } as IncomingRequestCfProperties;

beforeEach(clearDB);

async function signupRow(email: string) {
  return env.DONATE_DB.prepare(
    'SELECT email, country, region, unsubscribed_at FROM newsletter_signups WHERE email = ?',
  )
    .bind(email)
    .first<{ email: string; country: string | null; region: string | null }>();
}

async function signUp(email: string, newsletterOptIn: boolean) {
  await SELF.fetch(`${BASE}/signup-request`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, turnstile_token: TEST_TURNSTILE_TOKEN }),
  });
  const [delivery] = await listEmailDeliveries();
  const res = await SELF.fetch(`${BASE}/signup`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      verification_token: extractTokenFromDelivery(delivery!, 'signup_token'),
      password_auth: await passwordAuthFor('pw'),
      password_salt: await passwordSaltFor(email),
      pub_key: await publicKeyFor(email),
      encrypted_priv_key: privateKeyFor(email),
      newsletter_opt_in: newsletterOptIn,
    }),
    cf: CALIFORNIA,
  });
  expect(res.status).toBe(200);
}

function postNewsletterForm(fields: Record<string, string>) {
  return SELF.fetch(`${BASE}/newsletter`, {
    method: 'POST',
    body: new URLSearchParams(fields),
    redirect: 'manual',
    cf: CALIFORNIA,
  });
}

describe('newsletter opt-in at signup (API-010)', () => {
  it('records the email with the location of the signup', async () => {
    await signUp('reader@example.com', true);
    expect(await signupRow('reader@example.com')).toMatchObject({
      country: 'US',
      region: 'CA',
      unsubscribed_at: null,
    });
  });

  it('records nothing without the opt-in', async () => {
    await signUp('reader@example.com', false);
    expect(await signupRow('reader@example.com')).toBeNull();
  });

  it('never resubscribes an address that unsubscribed', async () => {
    await env.DONATE_DB.prepare(
      'INSERT INTO newsletter_signups (email, unsubscribed_at) VALUES (?, 1)',
    )
      .bind('reader@example.com')
      .run();
    await signUp('reader@example.com', true);
    expect(await signupRow('reader@example.com')).toMatchObject({ unsubscribed_at: 1 });
  });
});

describe('location fill on login (API-016)', () => {
  it('fills a missing location and leaves an existing one alone', async () => {
    await signupAndGetCookie('imported@example.com', 'pw');
    await signupAndGetCookie('located@example.com', 'pw');
    await env.DONATE_DB.batch([
      env.DONATE_DB.prepare('INSERT INTO newsletter_signups (email) VALUES (?)').bind(
        'imported@example.com',
      ),
      env.DONATE_DB.prepare(
        "INSERT INTO newsletter_signups (email, country, region) VALUES (?, 'CA', 'ON')",
      ).bind('located@example.com'),
    ]);

    for (const email of ['imported@example.com', 'located@example.com']) {
      const res = await SELF.fetch(`${BASE}/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password_auth: await passwordAuthFor('pw') }),
        cf: CALIFORNIA,
      });
      expect(res.status).toBe(204);
    }

    expect(await signupRow('imported@example.com')).toMatchObject({ country: 'US', region: 'CA' });
    expect(await signupRow('located@example.com')).toMatchObject({ country: 'CA', region: 'ON' });
  });
});

describe('POST /newsletter (API-058)', () => {
  it('adds the address with its location and redirects to the subscribed page', async () => {
    const res = await postNewsletterForm({
      email: 'Fan@Example.com',
      'cf-turnstile-response': TEST_TURNSTILE_TOKEN,
    });
    expect(res.status).toBe(303);
    expect(res.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/subscribed`);
    expect(await signupRow('fan@example.com')).toMatchObject({ country: 'US', region: 'CA' });
  });

  it('only fills a missing location for an address already on the list', async () => {
    await env.DONATE_DB.prepare(
      "INSERT INTO newsletter_signups (email, country, region) VALUES ('fan@example.com', 'CA', 'ON')",
    ).run();
    const res = await postNewsletterForm({
      email: 'fan@example.com',
      'cf-turnstile-response': TEST_TURNSTILE_TOKEN,
    });
    expect(res.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/subscribed`);
    expect(await signupRow('fan@example.com')).toMatchObject({ country: 'CA', region: 'ON' });
  });

  it('rejects a failed Turnstile check or a bad email without adding anything', async () => {
    for (const fields of [
      { email: 'fan@example.com', 'cf-turnstile-response': 'forged' },
      { email: 'fan@example.com' },
      { email: 'not-an-email', 'cf-turnstile-response': TEST_TURNSTILE_TOKEN },
    ]) {
      const res = await postNewsletterForm(fields);
      expect(res.status).toBe(303);
      expect(res.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/error`);
    }
    const count = await env.DONATE_DB.prepare(
      'SELECT COUNT(*) AS n FROM newsletter_signups',
    ).first<{ n: number }>();
    expect(count?.n).toBe(0);
  });
});

describe('Turnstile on /signup-request (API-009)', () => {
  it('refuses to send a signup email without a valid token', async () => {
    for (const token of [undefined, 'forged']) {
      const res = await SELF.fetch(`${BASE}/signup-request`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email: 'bot@example.com', turnstile_token: token }),
      });
      expect(res.status).toBe(403);
      expect(await res.json()).toMatchObject({ code: 'turnstile_failed' });
    }
    expect(await listEmailDeliveries()).toHaveLength(0);
  });
});
