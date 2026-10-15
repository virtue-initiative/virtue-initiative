import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { fetchMock, SELF } from 'cloudflare:test';
import {
  TEST_APP_STORE_FORGED_LEAF_CERT,
  TEST_APP_STORE_FORGED_LEAF_PRIVATE_KEY,
  TEST_APP_STORE_INTERMEDIATE_CERT,
  TEST_APP_STORE_LEAF_CERT,
  TEST_APP_STORE_LEAF_PRIVATE_KEY,
  TEST_APP_STORE_ROOT_CERT,
} from './app-store-test-keys';
import { authHeaders, BASE, clearDB, createDeviceForUser, signupAndGetCookie } from './helpers';
import { installHashServerMock } from './hash-server-mock';

const BUNDLE_ID = 'org.virtueinitiative.virtueios';
const APP_TRANSACTION_ID = '2000000123456789';

function base64Url(bytes: Uint8Array) {
  return Buffer.from(bytes).toString('base64url');
}

async function signJws(
  payload: Record<string, unknown>,
  {
    leafCert = TEST_APP_STORE_LEAF_CERT,
    privateKey = TEST_APP_STORE_LEAF_PRIVATE_KEY,
  }: { leafCert?: string; privateKey?: string } = {},
) {
  const header = base64Url(
    new TextEncoder().encode(
      JSON.stringify({
        alg: 'ES256',
        x5c: [leafCert, TEST_APP_STORE_INTERMEDIATE_CERT, TEST_APP_STORE_ROOT_CERT],
      }),
    ),
  );
  const body = base64Url(new TextEncoder().encode(JSON.stringify(payload)));
  const key = await crypto.subtle.importKey(
    'pkcs8',
    Buffer.from(privateKey.replace(/-----[A-Z ]+-----|\s/g, ''), 'base64'),
    { name: 'ECDSA', namedCurve: 'P-256' },
    false,
    ['sign'],
  );
  const signature = await crypto.subtle.sign(
    { name: 'ECDSA', hash: 'SHA-256' },
    key,
    new TextEncoder().encode(`${header}.${body}`),
  );
  return `${header}.${body}.${base64Url(new Uint8Array(signature))}`;
}

async function rescindConsentPayload({
  appTransactionId = APP_TRANSACTION_ID,
  bundleId = BUNDLE_ID,
} = {}) {
  const signedAppTransactionInfo = await signJws({
    appTransactionId,
    bundleId,
    environment: 'Production',
    signedDate: Date.now(),
  });
  return {
    notificationType: 'RESCIND_CONSENT',
    notificationUUID: crypto.randomUUID(),
    version: '2.0',
    signedDate: Date.now(),
    appData: { bundleId, environment: 'Production', signedAppTransactionInfo },
  };
}

function postNotification(signedPayload: string) {
  return SELF.fetch(`${BASE}/app-store/notifications`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ signedPayload }),
  });
}

async function deviceStatus(cookie: string, deviceId: string) {
  const res = await SELF.fetch(`${BASE}/device`, { headers: authHeaders(cookie) });
  const devices = (await res.json()) as Array<{ id: string; status: string }>;
  return devices.find((device) => device.id === deviceId)?.status;
}

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

beforeEach(clearDB);

describe('POST /app-store/notifications', () => {
  it('logs out every device from the download a RESCIND_CONSENT names', async () => {
    const { cookie } = await signupAndGetCookie('teen@example.com');
    const iphone = await createDeviceForUser(
      'teen@example.com',
      'password123',
      'iPhone',
      'ios',
      APP_TRANSACTION_ID,
    );
    const laptop = await createDeviceForUser('teen@example.com', 'password123', 'Laptop', 'linux');

    const res = await postNotification(await signJws(await rescindConsentPayload()));
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, devices: 1 });

    const staleSession = await SELF.fetch(`${BASE}/d/device`, {
      headers: { Authorization: `Bearer ${iphone.refresh_token}` },
    });
    expect(staleSession.status).toBe(401);
    expect(await deviceStatus(cookie, iphone.id)).toBe('logged_out');
    expect(await deviceStatus(cookie, laptop.id)).not.toBe('logged_out');
  });

  it('acknowledges notification types it does not act on', async () => {
    const res = await postNotification(
      await signJws({ notificationType: 'TEST', signedDate: Date.now(), version: '2.0' }),
    );
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true });
  });

  it('ignores RESCIND_CONSENT for another bundle', async () => {
    const { cookie } = await signupAndGetCookie('other-bundle@example.com');
    const iphone = await createDeviceForUser(
      'other-bundle@example.com',
      'password123',
      'iPhone',
      'ios',
      APP_TRANSACTION_ID,
    );

    const res = await postNotification(
      await signJws(await rescindConsentPayload({ bundleId: 'com.example.other' })),
    );
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true });
    expect(await deviceStatus(cookie, iphone.id)).not.toBe('logged_out');
  });

  it('rejects a payload signed by a key outside the pinned chain', async () => {
    const res = await postNotification(
      await signJws(await rescindConsentPayload(), {
        leafCert: TEST_APP_STORE_FORGED_LEAF_CERT,
        privateKey: TEST_APP_STORE_FORGED_LEAF_PRIVATE_KEY,
      }),
    );
    expect(res.status).toBe(403);
  });

  it('rejects a payload changed after signing', async () => {
    const [header, , signature] = (await signJws(await rescindConsentPayload())).split('.');
    const forgedBody = base64Url(
      new TextEncoder().encode(
        JSON.stringify({ notificationType: 'RESCIND_CONSENT', signedDate: Date.now() }),
      ),
    );
    const res = await postNotification(`${header}.${forgedBody}.${signature}`);
    expect(res.status).toBe(403);
  });

  it('rejects a payload signed outside the certificates validity', async () => {
    const res = await postNotification(
      await signJws({ notificationType: 'TEST', signedDate: Date.UTC(2020, 0, 1) }),
    );
    expect(res.status).toBe(403);
  });

  it('rejects a body that is not a notification', async () => {
    const res = await SELF.fetch(`${BASE}/app-store/notifications`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ hello: 'world' }),
    });
    expect(res.status).toBe(400);
  });
});
