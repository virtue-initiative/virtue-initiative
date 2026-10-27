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

interface SignupRow {
  email: string;
  token: string;
  country: string | null;
  region: string | null;
  confirmed_at: number | null;
  unsubscribed_at: number | null;
}

beforeEach(clearDB);

async function signupRow(email: string) {
  return env.DONATE_DB.prepare('SELECT * FROM newsletter_signups WHERE email = ?')
    .bind(email)
    .first<SignupRow>();
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

function postForm(path: string, fields: Record<string, string>) {
  return SELF.fetch(`${BASE}${path}`, {
    method: 'POST',
    body: new URLSearchParams(fields),
    redirect: 'manual',
    cf: CALIFORNIA,
  });
}

function subscribe(email: string) {
  return postForm('/newsletter', { email, 'cf-turnstile-response': TEST_TURNSTILE_TOKEN });
}

async function confirmationDeliveries() {
  return (await listEmailDeliveries()).filter((d) => d.kind === 'newsletter_confirmation');
}

describe('newsletter opt-in at signup (API-010)', () => {
  it('records the email as confirmed, with the location of the signup', async () => {
    await signUp('reader@example.com', true);
    const row = await signupRow('reader@example.com');
    expect(row).toMatchObject({ country: 'US', region: 'CA', unsubscribed_at: null });
    expect(typeof row?.confirmed_at).toBe('number');
  });

  it('records nothing without the opt-in', async () => {
    await signUp('reader@example.com', false);
    expect(await signupRow('reader@example.com')).toBeNull();
  });

  it('confirms an unconfirmed row from the landing form', async () => {
    await env.DONATE_DB.prepare('INSERT INTO newsletter_signups (email) VALUES (?)')
      .bind('reader@example.com')
      .run();
    await signUp('reader@example.com', true);
    expect(typeof (await signupRow('reader@example.com'))?.confirmed_at).toBe('number');
  });

  it('never resubscribes an address that unsubscribed', async () => {
    await env.DONATE_DB.prepare(
      'INSERT INTO newsletter_signups (email, unsubscribed_at) VALUES (?, 1)',
    )
      .bind('reader@example.com')
      .run();
    await signUp('reader@example.com', true);
    expect(await signupRow('reader@example.com')).toMatchObject({
      confirmed_at: null,
      unsubscribed_at: 1,
    });
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
  it('adds the address unconfirmed and emails it a confirmation link', async () => {
    const res = await subscribe('Fan@Example.com');
    expect(res.status).toBe(303);
    expect(res.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/check-email`);

    const row = await signupRow('fan@example.com');
    expect(row).toMatchObject({ country: 'US', region: 'CA', confirmed_at: null });
    const [delivery] = await confirmationDeliveries();
    expect(delivery?.recipient_email).toBe('fan@example.com');
    expect(delivery?.text).toContain(`${LANDING_URL}/newsletter/confirm?token=${row?.token}`);
  });

  it('sends at most one confirmation an hour, and none to a confirmed address', async () => {
    await subscribe('fan@example.com');
    await subscribe('fan@example.com');
    expect(await confirmationDeliveries()).toHaveLength(1);

    await env.DONATE_DB.prepare(
      "UPDATE newsletter_signups SET confirmation_sent_at = unixepoch() - 3601 WHERE email = 'fan@example.com'",
    ).run();
    await subscribe('fan@example.com');
    expect(await confirmationDeliveries()).toHaveLength(2);

    await env.DONATE_DB.prepare(
      "UPDATE newsletter_signups SET confirmed_at = 1, confirmation_sent_at = 0 WHERE email = 'fan@example.com'",
    ).run();
    const res = await subscribe('fan@example.com');
    expect(res.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/check-email`);
    expect(await confirmationDeliveries()).toHaveLength(2);
  });

  it('lets an unsubscribed address resubscribe by confirming a fresh email', async () => {
    await env.DONATE_DB.prepare(
      "INSERT INTO newsletter_signups (email, token, confirmed_at, confirmation_sent_at, unsubscribed_at) VALUES ('fan@example.com', 'oldtoken', 1, 1, 2)",
    ).run();

    // The token from an old newsletter can't resubscribe anyone.
    const stale = await postForm('/newsletter/confirm', { token: 'oldtoken' });
    expect(stale.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/error`);

    const res = await subscribe('fan@example.com');
    expect(res.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/check-email`);
    const [delivery] = await confirmationDeliveries();
    const row = await signupRow('fan@example.com');
    // Subscribing alone changes nothing but the token, and the old one stops working.
    expect(row).toMatchObject({ confirmed_at: 1, unsubscribed_at: 2 });
    expect(row?.token).not.toBe('oldtoken');
    expect(delivery?.text).toContain(`token=${row?.token}`);
    const old = await postForm('/newsletter/confirm', { token: 'oldtoken' });
    expect(old.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/error`);

    const confirmed = await postForm('/newsletter/confirm', { token: row!.token });
    expect(confirmed.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/subscribed`);
    const after = await signupRow('fan@example.com');
    expect(after?.unsubscribed_at).toBeNull();
    expect(after?.confirmed_at).toBeGreaterThan(1);
  });

  it('does not resubscribe a row that unsubscribed after its confirmation email', async () => {
    await subscribe('fan@example.com');
    const { token } = (await signupRow('fan@example.com'))!;
    // A complaint lands after the confirmation email went out.
    await env.DONATE_DB.prepare(
      "UPDATE newsletter_signups SET unsubscribed_at = unixepoch() + 1 WHERE email = 'fan@example.com'",
    ).run();
    const res = await postForm('/newsletter/confirm', { token });
    expect(res.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/error`);
    expect(typeof (await signupRow('fan@example.com'))?.unsubscribed_at).toBe('number');
  });

  it('rejects a failed Turnstile check or a bad email without adding anything', async () => {
    for (const fields of [
      { email: 'fan@example.com', 'cf-turnstile-response': 'forged' },
      { email: 'fan@example.com' },
      { email: 'not-an-email', 'cf-turnstile-response': TEST_TURNSTILE_TOKEN },
    ]) {
      const res = await postForm('/newsletter', fields);
      expect(res.status).toBe(303);
      expect(res.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/error`);
    }
    const count = await env.DONATE_DB.prepare(
      'SELECT COUNT(*) AS n FROM newsletter_signups',
    ).first<{ n: number }>();
    expect(count?.n).toBe(0);
    expect(await confirmationDeliveries()).toHaveLength(0);
  });
});

describe('POST /newsletter/confirm (API-059)', () => {
  it('confirms the row the token belongs to', async () => {
    await subscribe('fan@example.com');
    const { token } = (await signupRow('fan@example.com'))!;

    const res = await postForm('/newsletter/confirm', { token });
    expect(res.status).toBe(303);
    expect(res.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/subscribed`);
    expect(typeof (await signupRow('fan@example.com'))?.confirmed_at).toBe('number');
  });

  it('rejects an unknown token, and a row that unsubscribed', async () => {
    await subscribe('fan@example.com');
    const { token } = (await signupRow('fan@example.com'))!;
    await postForm(`/newsletter/unsubscribe?token=${token}`, {});

    for (const fields of [{ token: 'nope' }, { token }, {}]) {
      const res = await postForm('/newsletter/confirm', fields);
      expect(res.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/error`);
    }
    expect((await signupRow('fan@example.com'))?.confirmed_at).toBeNull();
  });
});

describe('POST /newsletter/unsubscribe (API-060)', () => {
  async function confirmedToken(email: string) {
    await env.DONATE_DB.prepare(
      'INSERT INTO newsletter_signups (email, confirmed_at) VALUES (?, 1)',
    )
      .bind(email)
      .run();
    return (await signupRow(email))!.token;
  }

  it('unsubscribes from the landing page button and redirects', async () => {
    const token = await confirmedToken('fan@example.com');
    const res = await postForm('/newsletter/unsubscribe', { token });
    expect(res.status).toBe(303);
    expect(res.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/unsubscribed`);
    expect(typeof (await signupRow('fan@example.com'))?.unsubscribed_at).toBe('number');
  });

  it('answers an RFC 8058 one-click post with 200', async () => {
    const token = await confirmedToken('fan@example.com');
    const res = await postForm(`/newsletter/unsubscribe?token=${token}`, {
      'List-Unsubscribe': 'One-Click',
    });
    expect(res.status).toBe(200);
    expect(typeof (await signupRow('fan@example.com'))?.unsubscribed_at).toBe('number');

    // Repeating it is harmless.
    const again = await postForm(`/newsletter/unsubscribe?token=${token}`, {
      'List-Unsubscribe': 'One-Click',
    });
    expect(again.status).toBe(200);
  });

  it('rejects an unknown token', async () => {
    const oneClick = await postForm('/newsletter/unsubscribe?token=nope', {
      'List-Unsubscribe': 'One-Click',
    });
    expect(oneClick.status).toBe(400);
    const button = await postForm('/newsletter/unsubscribe', { token: 'nope' });
    expect(button.headers.get('Location')).toBe(`${LANDING_URL}/newsletter/error`);
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
