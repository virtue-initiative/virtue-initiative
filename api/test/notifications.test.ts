import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { env, fetchMock, SELF } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  listEmailDeliveries,
  markUserEmailVerified,
  signupAndGetCookie,
  uuidToBytes,
} from './helpers';
import { installHashServerMock } from './hash-server-mock';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

beforeEach(clearDB);

async function uploadBatchWithNotification(
  deviceRefreshToken: string,
  ownerUserId: string,
  notification: Record<string, unknown>,
) {
  const form = new FormData();
  form.set('start_time', '1710000000000');
  form.set('end_time', '1710003600000');
  form.set(
    'access_keys',
    JSON.stringify({ keys: { [ownerUserId]: Buffer.from('owner-envelope').toString('base64') } }),
  );
  form.set('notifications', JSON.stringify([notification]));
  form.set('file', new File([new Uint8Array([1, 2, 3])], 'batch.enc'));

  return SELF.fetch(`${BASE}/d/batch`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${deviceRefreshToken}` },
    body: form,
  });
}

describe('Notification routes and tamper alerts', () => {
  it('stores notification frequency on the user and reflects it across monitored partners', async () => {
    const { cookie: ownerCookie } = await signupAndGetCookie('notify-owner@example.com', 'pw');
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'notify-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const createRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({
        email: 'notify-partner@example.com',
      }),
    });
    const created = (await createRes.json()) as { id: string };
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'notify-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const listRes = await SELF.fetch(`${BASE}/partner`, {
      headers: authHeaders(partnerCookie),
    });
    expect(listRes.status).toBe(200);
    const list = (await listRes.json()) as {
      watching: Array<{
        id: string;
        user: { email: string };
        digest_cadence: string;
      }>;
    };
    expect(list.watching[0]).toMatchObject({
      id: created.id,
      user: { email: 'notify-owner@example.com' },
      digest_cadence: 'daily',
    });

    const patchRes = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({
        settings: { email_frequency: 'alerts-only' },
      }),
    });
    expect(patchRes.status).toBe(200);

    const userRes = await SELF.fetch(`${BASE}/user`, {
      headers: authHeaders(partnerCookie),
    });
    expect(userRes.status).toBe(200);
    expect((await userRes.json()) as { settings: { email_frequency: string } }).toMatchObject({
      settings: { email_frequency: 'alerts-only' },
    });

    const updatedRes = await SELF.fetch(`${BASE}/partner`, {
      headers: authHeaders(partnerCookie),
    });
    const updated = (await updatedRes.json()) as {
      watching: Array<{
        id: string;
        digest_cadence: string;
      }>;
    };
    expect(updated.watching[0]).toMatchObject({
      id: created.id,
      digest_cadence: 'alerts-only',
    });
  });

  it('sends immediate tamper alerts for high-risk device log events', async () => {
    const { cookie: ownerCookie, userId: ownerUserId } = await signupAndGetCookie(
      'alerts-owner@example.com',
      'pw',
    );
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'alerts-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({
        email: 'alerts-partner@example.com',
      }),
    });
    await inviteRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'alerts-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const device = await createDeviceForUser(
      'alerts-owner@example.com',
      'pw',
      'Workstation',
      'linux',
    );
    const logRes = await uploadBatchWithNotification(device.refresh_token, ownerUserId, {
      ts: Date.now(),
      type: 'service_stop',
      risk: 0.7,
    });

    expect(logRes.status).toBe(201);

    const deliveries = await listEmailDeliveries();
    expect(deliveries.some((delivery) => delivery.kind === 'tamper_alert')).toBe(true);
    expect(
      deliveries.some((delivery) => delivery.recipient_email === 'alerts-partner@example.com'),
    ).toBe(true);
    const tamperDelivery = deliveries.find((delivery) => delivery.kind === 'tamper_alert');
    expect(tamperDelivery?.text).toContain('Device: Workstation');
  });

  it('passes custom title/details through to the rendered tamper alert email', async () => {
    const { cookie: ownerCookie, userId: ownerUserId } = await signupAndGetCookie(
      'custom-owner@example.com',
      'pw',
    );
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'custom-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({
        email: 'custom-partner@example.com',
      }),
    });
    await inviteRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'custom-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const device = await createDeviceForUser(
      'custom-owner@example.com',
      'pw',
      'Workstation',
      'linux',
    );
    const logRes = await uploadBatchWithNotification(device.refresh_token, ownerUserId, {
      ts: Date.now(),
      type: 'service_stop',
      risk: 0.7,
      title: 'Custom alert title',
      details: 'Custom alert details',
    });

    expect(logRes.status).toBe(201);

    const tamperDelivery = (await listEmailDeliveries()).find(
      (delivery) => delivery.kind === 'tamper_alert',
    );
    expect(tamperDelivery?.subject).toContain('Custom alert title');
    expect(tamperDelivery?.text).toContain('Custom alert title');
    expect(tamperDelivery?.text).toContain('Custom alert details');
  });

  it('sends immediate tamper alerts for moderate-risk device log events', async () => {
    const { cookie: ownerCookie, userId: ownerUserId } = await signupAndGetCookie(
      'moderate-owner@example.com',
      'pw',
    );
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'moderate-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({
        email: 'moderate-partner@example.com',
      }),
    });
    await inviteRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'moderate-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const device = await createDeviceForUser('moderate-owner@example.com', 'pw', 'Laptop', 'linux');
    const logRes = await uploadBatchWithNotification(device.refresh_token, ownerUserId, {
      ts: Date.now(),
      type: 'service_stop',
      risk: 0.69,
    });

    expect(logRes.status).toBe(201);
    expect((await listEmailDeliveries()).some((delivery) => delivery.kind === 'tamper_alert')).toBe(
      true,
    );
  });

  it('sends an immediate tamper alert even for zero risk; requires the risk field', async () => {
    const { cookie: ownerCookie, userId: ownerUserId } = await signupAndGetCookie(
      'non-tamper-owner@example.com',
      'pw',
    );
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'non-tamper-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({
        email: 'non-tamper-partner@example.com',
      }),
    });
    await inviteRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'non-tamper-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const device = await createDeviceForUser(
      'non-tamper-owner@example.com',
      'pw',
      'Desktop',
      'linux',
    );

    // The notify path doesn't re-filter by risk — the client's uploader only
    // includes a notification for high-risk events, so any entry here should send an alert.
    const zeroRiskRes = await uploadBatchWithNotification(device.refresh_token, ownerUserId, {
      ts: Date.now(),
      type: 'heartbeat',
      risk: 0,
    });
    expect(zeroRiskRes.status).toBe(201);
    expect((await listEmailDeliveries()).some((delivery) => delivery.kind === 'tamper_alert')).toBe(
      true,
    );

    // Risk is still a required field on each notification entry.
    const missingRiskRes = await uploadBatchWithNotification(device.refresh_token, ownerUserId, {
      ts: Date.now(),
      type: 'heartbeat',
    });
    expect(missingRiskRes.status).toBe(400);
  });

  it('stops all partner emails when receive emails is disabled', async () => {
    const { cookie: ownerCookie, userId: ownerUserId } = await signupAndGetCookie(
      'mute-owner@example.com',
      'pw',
    );
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'mute-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const createRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({
        email: 'mute-partner@example.com',
      }),
    });
    await createRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'mute-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ settings: { email_frequency: 'none' } }),
    });

    const device = await createDeviceForUser(
      'mute-owner@example.com',
      'pw',
      'Muted Device',
      'linux',
    );
    const baselineCount = (await listEmailDeliveries()).length;
    const logRes = await uploadBatchWithNotification(device.refresh_token, ownerUserId, {
      ts: Date.now(),
      type: 'service_stop',
      risk: 1,
    });

    expect(logRes.status).toBe(201);
    const deliveries = await listEmailDeliveries();
    expect(deliveries).toHaveLength(baselineCount);
    expect(deliveries.some((delivery) => delivery.kind === 'tamper_alert')).toBe(false);
  });

  it('suppresses tamper alerts to unverified recipient accounts', async () => {
    const { cookie: ownerCookie, userId: ownerUserId } = await signupAndGetCookie(
      'unverified-owner@example.com',
      'pw',
    );
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'unverified-partner@example.com',
      'pw',
    );

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({
        email: 'unverified-partner@example.com',
      }),
    });
    await inviteRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'unverified-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    await env.DB.prepare('UPDATE users SET email_verified = 0 WHERE id = ?')
      .bind(uuidToBytes(partnerUserId))
      .run();

    const device = await createDeviceForUser(
      'unverified-owner@example.com',
      'pw',
      'Quiet Device',
      'linux',
    );
    const baselineCount = (await listEmailDeliveries()).length;
    const logRes = await uploadBatchWithNotification(device.refresh_token, ownerUserId, {
      ts: Date.now(),
      type: 'service_stop',
      risk: 1,
    });

    expect(logRes.status).toBe(201);
    expect(await listEmailDeliveries()).toHaveLength(baselineCount);
  });
});
