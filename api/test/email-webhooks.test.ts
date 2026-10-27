import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { env, fetchMock, SELF } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  TEST_TURNSTILE_TOKEN,
  clearDB,
  latestEmailToken,
  listEmailDeliveries,
  markUserEmailVerified,
  passwordAuthFor,
  signupAndGetCookie,
  uuidToBytes,
} from './helpers';
import { TEST_OTHER_SNS_PRIVATE_KEY, TEST_SNS_CERT, TEST_SNS_PRIVATE_KEY } from './sns-test-keys';

const SNS_ORIGIN = 'https://sns.us-east-1.amazonaws.com';
// Both are in SNS_TOPIC_ARNS (vitest.config.ts).
const TOPIC_ARN = 'arn:aws:sns:us-east-1:222222222222:ses-events';
const OLD_TOPIC_ARN = 'arn:aws:sns:us-east-1:111111111111:old-ses-events';

type SnsFields = Record<string, string>;

let certCounter = 0;

// A fresh path per test: the API caches signing certs by URL across requests.
function nextCertPath() {
  certCounter += 1;
  return `/SimpleNotificationService-test${certCounter}.pem`;
}

function serveCert(path: string) {
  fetchMock.get(SNS_ORIGIN).intercept({ path }).reply(200, TEST_SNS_CERT);
}

async function sign(fields: SnsFields, privateKeyPem: string) {
  const signedNames =
    fields.Type === 'Notification'
      ? ['Message', 'MessageId', 'Subject', 'Timestamp', 'TopicArn', 'Type']
      : ['Message', 'MessageId', 'SubscribeURL', 'Timestamp', 'Token', 'TopicArn', 'Type'];
  const stringToSign = signedNames
    .filter((name) => name in fields)
    .map((name) => `${name}\n${fields[name]}\n`)
    .join('');

  const key = await crypto.subtle.importKey(
    'pkcs8',
    Buffer.from(privateKeyPem.replace(/-----[A-Z ]+-----|\s/g, ''), 'base64'),
    { name: 'RSASSA-PKCS1-v1_5', hash: fields.SignatureVersion === '2' ? 'SHA-256' : 'SHA-1' },
    false,
    ['sign'],
  );
  const signature = await crypto.subtle.sign(
    'RSASSA-PKCS1-v1_5',
    key,
    new TextEncoder().encode(stringToSign),
  );
  return Buffer.from(signature).toString('base64');
}

async function snsMessage(
  overrides: SnsFields,
  { certPath = nextCertPath(), privateKey = TEST_SNS_PRIVATE_KEY } = {},
) {
  const fields: SnsFields = {
    Type: 'Notification',
    MessageId: 'b6f0a7a2-5c6f-5b8e-9d7e-fb1c0e0c1a11',
    TopicArn: TOPIC_ARN,
    Message: '{}',
    Timestamp: '2026-10-10T12:00:00.000Z',
    SignatureVersion: '1',
    SigningCertURL: `${SNS_ORIGIN}${certPath}`,
    ...overrides,
  };
  return { ...fields, Signature: await sign(fields, privateKey) };
}

function bounceMessage(email: string) {
  return JSON.stringify({
    eventType: 'Bounce',
    bounce: { bounceType: 'Permanent', bouncedRecipients: [{ emailAddress: email }] },
  });
}

function postSns(body: unknown) {
  return SELF.fetch(`${BASE}/email/sns`, {
    method: 'POST',
    // SNS delivers its JSON with a text/plain content type.
    headers: { 'Content-Type': 'text/plain; charset=UTF-8' },
    body: typeof body === 'string' ? body : JSON.stringify(body),
  });
}

async function verifiedUser(email: string) {
  const { userId } = await signupAndGetCookie(email, 'pw');
  await markUserEmailVerified(userId);
  return userId;
}

async function userEmailState(userId: string) {
  return env.DB.prepare('SELECT email_verified, email_bounced_at FROM users WHERE id = ?')
    .bind(uuidToBytes(userId))
    .first<{ email_verified: number; email_bounced_at: number | null }>();
}

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
});

beforeEach(clearDB);

// Also proves rejected requests never fetched a cert or SubscribeURL they shouldn't have.
afterEach(() => fetchMock.assertNoPendingInterceptors());

async function postBounce(
  email: string,
  bounceType: 'Permanent' | 'Transient' | 'Undetermined',
  emailTokenId?: string,
) {
  const certPath = nextCertPath();
  serveCert(certPath);
  return postSns(
    await snsMessage(
      {
        Message: JSON.stringify({
          eventType: 'Bounce',
          mail: { tags: emailTokenId ? { email_token_id: [emailTokenId] } : {} },
          bounce: { bounceType, bouncedRecipients: [{ emailAddress: email }] },
        }),
      },
      { certPath },
    ),
  );
}

async function listBounces(email: string) {
  const { results } = await env.DB.prepare(
    'SELECT reason FROM email_bounces WHERE email = ? ORDER BY bounced_at',
  )
    .bind(email)
    .all<{ reason: string }>();
  return results.map((row) => row.reason);
}

async function tokenHasBounce(tokenId: string) {
  const row = await env.DB.prepare('SELECT bounce_id FROM email_tokens WHERE id = ?')
    .bind(uuidToBytes(tokenId))
    .first<{ bounce_id: ArrayBuffer | null }>();
  return row?.bounce_id != null;
}

async function invitePartner(cookie: string, email: string, replaceId?: string) {
  return SELF.fetch(`${BASE}/partner`, {
    method: 'POST',
    headers: authHeaders(cookie),
    body: JSON.stringify({ email, ...(replaceId ? { replace_id: replaceId } : {}) }),
  });
}

async function listWatchers(cookie: string) {
  const res = await SELF.fetch(`${BASE}/partner`, { headers: authHeaders(cookie) });
  const body = (await res.json()) as {
    watchers: Array<{ id: string; status: string; user: { email: string } }>;
  };
  return body.watchers;
}

describe('Email webhooks', () => {
  it('marks users unverified on SNS bounce notifications', async () => {
    const userId = await verifiedUser('bounce@example.com');
    const certPath = nextCertPath();
    serveCert(certPath);

    const res = await postSns(
      await snsMessage({ Message: bounceMessage('bounce@example.com') }, { certPath }),
    );

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, updated: 1 });

    const user = await userEmailState(userId);
    expect(user?.email_verified).toBe(0);
    expect(typeof user?.email_bounced_at).toBe('number');
  });

  it('marks users unverified on SNS complaint notifications', async () => {
    const userId = await verifiedUser('complaint@example.com');
    const certPath = nextCertPath();
    serveCert(certPath);

    const res = await postSns(
      await snsMessage(
        {
          Message: JSON.stringify({
            eventType: 'Complaint',
            complaint: { complainedRecipients: [{ emailAddress: 'complaint@example.com' }] },
          }),
        },
        { certPath },
      ),
    );

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, updated: 1 });

    const user = await userEmailState(userId);
    expect(user?.email_verified).toBe(0);
    expect(user?.email_bounced_at).toBeNull();
  });

  it('unsubscribes newsletter rows on a permanent bounce but not a transient one', async () => {
    await env.DONATE_DB.prepare(
      "INSERT INTO newsletter_signups (email, confirmed_at) VALUES ('gone@example.com', 1), ('soft@example.com', 1)",
    ).run();

    await postBounce('gone@example.com', 'Permanent');
    await postBounce('soft@example.com', 'Transient');

    const { results } = await env.DONATE_DB.prepare(
      'SELECT email, unsubscribed_at FROM newsletter_signups ORDER BY email',
    ).all<{ email: string; unsubscribed_at: number | null }>();
    expect(typeof results[0]?.unsubscribed_at).toBe('number');
    expect(results[1]?.unsubscribed_at).toBeNull();
  });

  it('unsubscribes on a newsletter complaint without unverifying the account', async () => {
    const userId = await verifiedUser('reader@example.com');
    await env.DONATE_DB.prepare(
      "INSERT INTO newsletter_signups (email, confirmed_at) VALUES ('reader@example.com', 1)",
    ).run();
    const certPath = nextCertPath();
    serveCert(certPath);

    const res = await postSns(
      await snsMessage(
        {
          Message: JSON.stringify({
            eventType: 'Complaint',
            mail: { tags: { 'ses:configuration-set': ['newsletter'] } },
            complaint: { complainedRecipients: [{ emailAddress: 'reader@example.com' }] },
          }),
        },
        { certPath },
      ),
    );

    expect(res.status).toBe(200);
    expect((await userEmailState(userId))?.email_verified).toBe(1);
    const row = await env.DONATE_DB.prepare(
      "SELECT unsubscribed_at FROM newsletter_signups WHERE email = 'reader@example.com'",
    ).first<{ unsubscribed_at: number | null }>();
    expect(typeof row?.unsubscribed_at).toBe('number');
  });

  it('accepts SignatureVersion 2, a Subject, and every allowlisted topic', async () => {
    const userId = await verifiedUser('bounce@example.com');
    const certPath = nextCertPath();
    serveCert(certPath);

    const res = await postSns(
      await snsMessage(
        {
          Message: bounceMessage('bounce@example.com'),
          Subject: 'Amazon SES Email Event Notification',
          SignatureVersion: '2',
          TopicArn: OLD_TOPIC_ARN,
        },
        { certPath },
      ),
    );

    expect(res.status).toBe(200);
    expect((await userEmailState(userId))?.email_verified).toBe(0);
  });

  it('fetches the signing cert once per URL', async () => {
    const certPath = nextCertPath();
    serveCert(certPath);

    expect((await postSns(await snsMessage({}, { certPath }))).status).toBe(200);
    // No second interceptor: a second fetch would fail under disableNetConnect.
    expect((await postSns(await snsMessage({}, { certPath }))).status).toBe(200);
  });

  it('rejects a body that is not an SNS message', async () => {
    const unsigned = {
      Type: 'Notification',
      Message: bounceMessage('bounce@example.com'),
    };

    expect((await postSns(unsigned)).status).toBe(400);
    expect((await postSns('not json')).status).toBe(400);
  });

  it('rejects a topic that is not allowlisted without fetching its cert', async () => {
    const userId = await verifiedUser('bounce@example.com');

    const res = await postSns(
      await snsMessage({
        Message: bounceMessage('bounce@example.com'),
        TopicArn: 'arn:aws:sns:us-east-1:333333333333:ses-events',
      }),
    );

    expect(res.status).toBe(403);
    expect((await userEmailState(userId))?.email_verified).toBe(1);
  });

  it('rejects a signature from another key', async () => {
    const userId = await verifiedUser('bounce@example.com');
    const certPath = nextCertPath();
    serveCert(certPath);

    const res = await postSns(
      await snsMessage(
        { Message: bounceMessage('bounce@example.com') },
        { certPath, privateKey: TEST_OTHER_SNS_PRIVATE_KEY },
      ),
    );

    expect(res.status).toBe(403);
    expect((await userEmailState(userId))?.email_verified).toBe(1);
  });

  it('rejects a message altered after signing', async () => {
    const userId = await verifiedUser('bounce@example.com');
    const certPath = nextCertPath();
    serveCert(certPath);

    const signed = await snsMessage({ Message: bounceMessage('other@example.com') }, { certPath });
    const res = await postSns({ ...signed, Message: bounceMessage('bounce@example.com') });

    expect(res.status).toBe(403);
    expect((await userEmailState(userId))?.email_verified).toBe(1);
  });

  it.each([
    'https://evil.example.com/cert.pem',
    'http://sns.us-east-1.amazonaws.com/cert.pem',
    'https://sns.us-east-1.amazonaws.com.evil.example.com/cert.pem',
    'https://evil.example.com/sns.us-east-1.amazonaws.com/cert.pem',
    'https://sns.us-east-1.amazonaws.com@evil.example.com/cert.pem',
  ])('rejects a signing cert at %s without fetching it', async (SigningCertURL) => {
    const userId = await verifiedUser('bounce@example.com');

    const res = await postSns(
      await snsMessage({ Message: bounceMessage('bounce@example.com'), SigningCertURL }),
    );

    expect(res.status).toBe(403);
    expect((await userEmailState(userId))?.email_verified).toBe(1);
  });

  it('confirms a subscription on an SNS host', async () => {
    const certPath = nextCertPath();
    serveCert(certPath);
    fetchMock
      .get(SNS_ORIGIN)
      .intercept({ path: '/?Action=ConfirmSubscription&Token=abc' })
      .reply(200, '<ConfirmSubscriptionResponse/>');

    const res = await postSns(
      await snsMessage(
        {
          Type: 'SubscriptionConfirmation',
          Message: 'You have chosen to subscribe to the topic.',
          Token: 'abc',
          SubscribeURL: `${SNS_ORIGIN}/?Action=ConfirmSubscription&Token=abc`,
        },
        { certPath },
      ),
    );

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, subscribed: true });
  });

  it('does not follow a SubscribeURL off SNS, even when correctly signed', async () => {
    const certPath = nextCertPath();
    serveCert(certPath);

    const res = await postSns(
      await snsMessage(
        {
          Type: 'SubscriptionConfirmation',
          Message: 'You have chosen to subscribe to the topic.',
          Token: 'abc',
          SubscribeURL: 'https://evil.example.com/?Action=ConfirmSubscription&Token=abc',
        },
        { certPath },
      ),
    );

    expect(res.status).toBe(403);
  });

  it('does not follow a SubscribeURL from an unsigned confirmation', async () => {
    const certPath = nextCertPath();
    serveCert(certPath);

    const res = await postSns(
      await snsMessage(
        {
          Type: 'SubscriptionConfirmation',
          Message: 'You have chosen to subscribe to the topic.',
          Token: 'abc',
          SubscribeURL: `${SNS_ORIGIN}/?Action=ConfirmSubscription&Token=abc`,
        },
        { certPath, privateKey: TEST_OTHER_SNS_PRIVATE_KEY },
      ),
    );

    expect(res.status).toBe(403);
  });

  it('ignores other signed message types', async () => {
    const certPath = nextCertPath();
    serveCert(certPath);

    const res = await postSns(
      await snsMessage(
        {
          Type: 'UnsubscribeConfirmation',
          Message: 'You have chosen to deactivate subscription.',
          Token: 'abc',
          SubscribeURL: `${SNS_ORIGIN}/?Action=ConfirmSubscription&Token=abc`,
        },
        { certPath },
      ),
    );

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true });
  });

  it('records a temporary bounce without unverifying the user or blocking the address', async () => {
    const { userId } = await signupAndGetCookie('soft@example.com', 'pw');
    await markUserEmailVerified(userId);

    expect((await postBounce('soft@example.com', 'Transient')).status).toBe(200);

    expect(await listBounces('soft@example.com')).toEqual(['temporary']);
    const user = await env.DB.prepare('SELECT email_verified FROM users WHERE id = ?')
      .bind(uuidToBytes(userId))
      .first<{ email_verified: number }>();
    expect(user?.email_verified).toBe(1);

    const before = (await listEmailDeliveries()).length;
    const resetRes = await SELF.fetch(`${BASE}/password-reset`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'soft@example.com' }),
    });
    expect(resetRes.status).toBe(204);
    expect((await listEmailDeliveries()).length).toBe(before + 1);
  });

  it('marks only the tagged token on a temporary bounce', async () => {
    const { cookie, userId } = await signupAndGetCookie('owner@example.com', 'pw');
    await markUserEmailVerified(userId);
    const { cookie: otherCookie, userId: otherId } = await signupAndGetCookie(
      'other@example.com',
      'pw',
    );
    await markUserEmailVerified(otherId);

    await invitePartner(cookie, 'friend@example.com');
    const firstToken = await latestEmailToken('partner_invite');
    await invitePartner(otherCookie, 'friend@example.com');

    await postBounce('friend@example.com', 'Transient', firstToken!.id);

    expect((await listWatchers(cookie))[0].status).toBe('invite_failed');
    expect((await listWatchers(otherCookie))[0].status).toBe('pending');
  });

  it('fails every outstanding invite to a permanently bounced address', async () => {
    const { cookie, userId } = await signupAndGetCookie('owner@example.com', 'pw');
    await markUserEmailVerified(userId);
    const { cookie: otherCookie, userId: otherId } = await signupAndGetCookie(
      'other@example.com',
      'pw',
    );
    await markUserEmailVerified(otherId);

    await invitePartner(cookie, 'gone@example.com');
    await invitePartner(otherCookie, 'gone@example.com');

    await postBounce('gone@example.com', 'Permanent');

    expect((await listWatchers(cookie))[0].status).toBe('invite_failed');
    expect((await listWatchers(otherCookie))[0].status).toBe('invite_failed');
  });

  it('does not send to a permanently bounced address and fails the new invite immediately', async () => {
    const { cookie, userId } = await signupAndGetCookie('owner@example.com', 'pw');
    await markUserEmailVerified(userId);
    await postBounce('gone@example.com', 'Permanent');

    const before = (await listEmailDeliveries()).length;
    const res = await invitePartner(cookie, 'gone@example.com');
    expect(res.status).toBe(200);
    expect(((await res.json()) as { status: string }).status).toBe('invite_failed');
    expect((await listEmailDeliveries()).length).toBe(before);

    const token = await latestEmailToken('partner_invite');
    expect(await tokenHasBounce(token!.id)).toBe(true);
    expect((await listWatchers(cookie))[0].status).toBe('invite_failed');
  });

  it('resends a failed invite to a new email by replacing the old one', async () => {
    const { cookie, userId } = await signupAndGetCookie('owner@example.com', 'pw');
    await markUserEmailVerified(userId);
    const created = (await (await invitePartner(cookie, 'typo@example.com')).json()) as {
      id: string;
    };
    await postBounce('typo@example.com', 'Permanent');

    const res = await invitePartner(cookie, 'right@example.com', created.id);
    expect(res.status).toBe(200);
    expect(((await res.json()) as { status: string }).status).toBe('pending');

    const watchers = await listWatchers(cookie);
    expect(watchers).toHaveLength(1);
    expect(watchers[0]).toMatchObject({ status: 'pending', user: { email: 'right@example.com' } });
  });

  it('resends to the same email after a temporary bounce', async () => {
    const { cookie, userId } = await signupAndGetCookie('owner@example.com', 'pw');
    await markUserEmailVerified(userId);
    const created = (await (await invitePartner(cookie, 'full@example.com')).json()) as {
      id: string;
    };
    const token = await latestEmailToken('partner_invite');
    await postBounce('full@example.com', 'Transient', token!.id);
    expect((await listWatchers(cookie))[0].status).toBe('invite_failed');

    const res = await invitePartner(cookie, 'full@example.com', created.id);
    expect(res.status).toBe(200);
    const watchers = await listWatchers(cookie);
    expect(watchers).toHaveLength(1);
    expect(watchers[0].status).toBe('pending');
  });

  it("rejects replace_id for someone else's or an accepted partnership", async () => {
    const { cookie, userId } = await signupAndGetCookie('owner@example.com', 'pw');
    await markUserEmailVerified(userId);
    const { cookie: otherCookie } = await signupAndGetCookie('other@example.com', 'pw');
    const created = (await (await invitePartner(cookie, 'friend@example.com')).json()) as {
      id: string;
    };

    const res = await invitePartner(otherCookie, 'new@example.com', created.id);
    expect(res.status).toBe(404);
    expect(await listWatchers(cookie)).toHaveLength(1);
  });

  it('tells signup-request that the address bounced instead of sending', async () => {
    await postBounce('gone@example.com', 'Permanent');

    const res = await SELF.fetch(`${BASE}/signup-request`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'gone@example.com', turnstile_token: TEST_TURNSTILE_TOKEN }),
    });
    expect(res.status).toBe(422);
    expect(((await res.json()) as { code: string }).code).toBe('email_bounced');
    expect(await listEmailDeliveries()).toHaveLength(0);
  });

  it('reports the bounce instead of resending verification at login', async () => {
    const { userId } = await signupAndGetCookie('gone@example.com', 'pw');
    await markUserEmailVerified(userId);
    await postBounce('gone@example.com', 'Permanent');

    const before = (await listEmailDeliveries()).length;
    const res = await SELF.fetch(`${BASE}/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email: 'gone@example.com',
        password_auth: await passwordAuthFor('pw'),
      }),
    });
    expect(res.status).toBe(403);
    expect(((await res.json()) as { code?: string }).code).toBe('email_bounced');
    expect((await listEmailDeliveries()).length).toBe(before);
    const token = await latestEmailToken('email_verification');
    expect(await tokenHasBounce(token!.id)).toBe(true);
  });

  it('reports the bounce when changing email to a permanently bounced address', async () => {
    const { cookie, userId } = await signupAndGetCookie('user@example.com', 'pw');
    await markUserEmailVerified(userId);
    await postBounce('gone@example.com', 'Permanent');

    const before = (await listEmailDeliveries()).length;
    const res = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(cookie),
      body: JSON.stringify({ email: 'gone@example.com' }),
    });
    expect(res.status).toBe(200);
    expect(await res.json()).toMatchObject({
      email_bounced: true,
      pending_email: 'gone@example.com',
    });
    expect((await listEmailDeliveries()).length).toBe(before);
  });

  it('clears bounces for an address once a verification link sent to it is used', async () => {
    const { userId } = await signupAndGetCookie('back@example.com', 'pw');
    await env.DB.prepare('UPDATE users SET email_verified = 0 WHERE id = ?')
      .bind(uuidToBytes(userId))
      .run();
    await SELF.fetch(`${BASE}/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email: 'back@example.com',
        password_auth: await passwordAuthFor('pw'),
      }),
    });
    const delivery = (await listEmailDeliveries()).reverse()[0];
    const verifyUrl = (JSON.parse(delivery.metadata) as { verifyUrl: string }).verifyUrl;
    const token = new URL(verifyUrl).searchParams.get('token');
    await postBounce('back@example.com', 'Permanent');

    const res = await SELF.fetch(`${BASE}/email-verification/validate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token }),
    });
    expect(res.status).toBe(200);
    expect(await listBounces('back@example.com')).toEqual([]);
  });
});
