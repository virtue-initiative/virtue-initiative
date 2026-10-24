import { env, fetchMock, SELF } from 'cloudflare:test';
import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { BASE, clearDB } from './helpers';
import { TEST_OTHER_SNS_PRIVATE_KEY, TEST_SNS_CERT, TEST_SNS_PRIVATE_KEY } from './sns-test-keys';

const SNS_ORIGIN = 'https://sns.us-east-1.amazonaws.com';
const TOPIC_ARN = 'arn:aws:sns:us-east-1:222222222222:newsletter-events';

type SnsFields = Record<string, string>;

let certCounter = 0;

// A fresh path per test: the worker caches signing certs by URL.
function nextCertPath() {
  certCounter += 1;
  return `/SimpleNotificationService-test${certCounter}.pem`;
}

async function sign(fields: SnsFields, privateKeyPem: string) {
  const names =
    fields.Type === 'Notification'
      ? ['Message', 'MessageId', 'Subject', 'Timestamp', 'TopicArn', 'Type']
      : ['Message', 'MessageId', 'SubscribeURL', 'Timestamp', 'Token', 'TopicArn', 'Type'];
  const stringToSign = names
    .filter((name) => name in fields)
    .map((name) => `${name}\n${fields[name]}\n`)
    .join('');
  const key = await crypto.subtle.importKey(
    'pkcs8',
    Buffer.from(privateKeyPem.replace(/-----[A-Z ]+-----|\s/g, ''), 'base64'),
    { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-1' },
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

async function post(
  overrides: SnsFields,
  { privateKey = TEST_SNS_PRIVATE_KEY, serve = true } = {},
) {
  const certPath = nextCertPath();
  if (serve) fetchMock.get(SNS_ORIGIN).intercept({ path: certPath }).reply(200, TEST_SNS_CERT);
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
  return SELF.fetch(`${BASE}/newsletter/sns`, {
    method: 'POST',
    headers: { 'Content-Type': 'text/plain; charset=UTF-8' },
    body: JSON.stringify({ ...fields, Signature: await sign(fields, privateKey) }),
  });
}

const bounce = (type: string, email: string) =>
  JSON.stringify({
    eventType: 'Bounce',
    bounce: { bounceType: type, bouncedRecipients: [{ emailAddress: email }] },
  });
const complaint = (email: string) =>
  JSON.stringify({
    eventType: 'Complaint',
    complaint: { complainedRecipients: [{ emailAddress: email }] },
  });

const unsubscribedAt = async (email: string) =>
  (
    await env.DB.prepare('SELECT unsubscribed_at FROM newsletter_subscribers WHERE email = ?')
      .bind(email)
      .first<{ unsubscribed_at: number | null }>()
  )?.unsubscribed_at;

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
});

beforeEach(async () => {
  await clearDB();
  await env.DB.prepare(
    "INSERT INTO newsletter_subscribers (email, source, confirmed_at) VALUES ('a@example.com', 'landing', 1), ('b@example.com', 'landing', 1)",
  ).run();
});

describe('POST /newsletter/sns', () => {
  it('suppresses a permanent bounce and nobody else', async () => {
    const res = await post({ Message: bounce('Permanent', 'A@Example.com') });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, updated: 1 });
    expect(await unsubscribedAt('a@example.com')).toBeTypeOf('number');
    expect(await unsubscribedAt('b@example.com')).toBeNull();
  });

  it('suppresses a complaint', async () => {
    await post({ Message: complaint('b@example.com') });
    expect(await unsubscribedAt('b@example.com')).toBeTypeOf('number');
  });

  it('ignores a transient bounce', async () => {
    const res = await post({ Message: bounce('Transient', 'a@example.com') });
    expect(await res.json()).toEqual({ ok: true, updated: 0 });
    expect(await unsubscribedAt('a@example.com')).toBeNull();
  });

  it('rejects a topic that is not allowlisted', async () => {
    const res = await post(
      { TopicArn: 'arn:aws:sns:us-east-1:1:other', Message: complaint('a@example.com') },
      { serve: false },
    );
    expect(res.status).toBe(403);
    expect(await unsubscribedAt('a@example.com')).toBeNull();
  });

  it('rejects a signature from the wrong key', async () => {
    const res = await post(
      { Message: complaint('a@example.com') },
      { privateKey: TEST_OTHER_SNS_PRIVATE_KEY },
    );
    expect(res.status).toBe(403);
    expect(await unsubscribedAt('a@example.com')).toBeNull();
  });

  it('rejects a certificate URL that is not on amazonaws.com', async () => {
    const res = await post(
      { Message: complaint('a@example.com'), SigningCertURL: 'https://evil.example.com/cert.pem' },
      { serve: false },
    );
    expect(res.status).toBe(403);
  });

  it('confirms the subscription by fetching SubscribeURL', async () => {
    fetchMock.get(SNS_ORIGIN).intercept({ path: '/confirm?token=t' }).reply(200, 'ok');
    const res = await post({
      Type: 'SubscriptionConfirmation',
      Token: 't',
      SubscribeURL: `${SNS_ORIGIN}/confirm?token=t`,
    });
    expect(await res.json()).toEqual({ ok: true, subscribed: true });
  });

  it('rejects a body that is not an SNS message', async () => {
    const res = await SELF.fetch(`${BASE}/newsletter/sns`, { method: 'POST', body: '{}' });
    expect(res.status).toBe(400);
  });
});
