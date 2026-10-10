import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { env, fetchMock, SELF } from 'cloudflare:test';
import { BASE, clearDB, markUserEmailVerified, signupAndGetCookie, uuidToBytes } from './helpers';
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
    bounce: { bouncedRecipients: [{ emailAddress: email }] },
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
});
