import { beforeEach, describe, expect, it } from 'vitest';
import { env, SELF } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  listEmailDeliveries,
  markUserEmailVerified,
  signupAndGetToken,
  uuidToBytes,
} from './helpers';

beforeEach(clearDB);

describe('Notification routes and tamper alerts', () => {
  it('lists and updates owner notification preferences', async () => {
    const { token: ownerToken } = await signupAndGetToken('notify-owner@example.com', 'pw');
    const { token: partnerToken, userId: partnerUserId } = await signupAndGetToken(
      'notify-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const createRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({
        email: 'notify-partner@example.com',
        permissions: { view_data: true },
      }),
    });
    const created = (await createRes.json()) as { id: string };
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'notify-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/invite/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const listRes = await SELF.fetch(`${BASE}/notifications/preferences`, {
      headers: { Authorization: `Bearer ${partnerToken}` },
    });
    expect(listRes.status).toBe(200);
    const list = (await listRes.json()) as Array<{
      partnership_id: string;
      digest_cadence: string;
      immediate_tamper_severity: string;
      send_digest: boolean;
    }>;
    expect(list[0]).toMatchObject({
      partnership_id: created.id,
      monitored_user: { email: 'notify-owner@example.com' },
      digest_cadence: 'daily',
      immediate_tamper_severity: 'critical',
      send_digest: true,
    });

    const patchRes = await SELF.fetch(`${BASE}/notifications/preferences/${created.id}`, {
      method: 'PATCH',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({
        digest_cadence: 'twice_weekly',
        immediate_tamper_severity: 'warning',
        send_digest: false,
      }),
    });
    expect(patchRes.status).toBe(200);
    expect(await patchRes.json()).toEqual({
      partnership_id: created.id,
      digest_cadence: 'twice_weekly',
      immediate_tamper_severity: 'warning',
      send_digest: false,
    });
  });

  it('sends immediate tamper alerts for critical device log events', async () => {
    const { token: ownerToken } = await signupAndGetToken('alerts-owner@example.com', 'pw');
    const { token: partnerToken, userId: partnerUserId } = await signupAndGetToken(
      'alerts-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({
        email: 'alerts-partner@example.com',
        permissions: { view_data: true },
      }),
    });
    await inviteRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'alerts-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/invite/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const device = await createDeviceForUser(ownerToken, 'Workstation', 'linux');
    const logRes = await SELF.fetch(`${BASE}/d/log`, {
      method: 'POST',
      headers: authHeaders(device.access_token),
      body: JSON.stringify({ ts: Date.now(), type: 'service_stop', data: {} }),
    });

    expect(logRes.status).toBe(201);

    const storedLog = await env.DB.prepare(
      'SELECT risk FROM device_logs WHERE device_id = ? ORDER BY created_at DESC LIMIT 1',
    )
      .bind(uuidToBytes(device.id))
      .first<{ risk: number | null }>();
    expect(storedLog?.risk).toBe(1);

    const deliveries = await listEmailDeliveries();
    expect(deliveries.some((delivery) => delivery.kind === 'tamper_alert')).toBe(true);
    expect(
      deliveries.some((delivery) => delivery.recipient_email === 'alerts-partner@example.com'),
    ).toBe(true);
  });

  it('stops all partner emails when receive emails is disabled', async () => {
    const { token: ownerToken } = await signupAndGetToken('mute-owner@example.com', 'pw');
    const { token: partnerToken, userId: partnerUserId } = await signupAndGetToken(
      'mute-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const createRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({
        email: 'mute-partner@example.com',
        permissions: { view_data: true },
      }),
    });
    const created = (await createRes.json()) as { id: string };
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'mute-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/invite/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    await SELF.fetch(`${BASE}/notifications/preferences/${created.id}`, {
      method: 'PATCH',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ send_digest: false }),
    });

    const device = await createDeviceForUser(ownerToken, 'Muted Device', 'linux');
    const baselineCount = (await listEmailDeliveries()).length;
    const logRes = await SELF.fetch(`${BASE}/d/log`, {
      method: 'POST',
      headers: authHeaders(device.access_token),
      body: JSON.stringify({ ts: Date.now(), type: 'service_stop', data: {} }),
    });

    expect(logRes.status).toBe(201);
    const deliveries = await listEmailDeliveries();
    expect(deliveries).toHaveLength(baselineCount);
    expect(deliveries.some((delivery) => delivery.kind === 'tamper_alert')).toBe(false);
  });

  it('suppresses tamper alerts to unverified recipient accounts', async () => {
    const { token: ownerToken } = await signupAndGetToken('unverified-owner@example.com', 'pw');
    const { token: partnerToken } = await signupAndGetToken('unverified-partner@example.com', 'pw');

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({
        email: 'unverified-partner@example.com',
        permissions: { view_data: true },
      }),
    });
    await inviteRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'unverified-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/invite/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const device = await createDeviceForUser(ownerToken, 'Quiet Device', 'linux');
    const baselineCount = (await listEmailDeliveries()).length;
    const logRes = await SELF.fetch(`${BASE}/d/log`, {
      method: 'POST',
      headers: authHeaders(device.access_token),
      body: JSON.stringify({ ts: Date.now(), type: 'service_stop', data: {} }),
    });

    expect(logRes.status).toBe(201);
    expect(await listEmailDeliveries()).toHaveLength(baselineCount);
  });
});
