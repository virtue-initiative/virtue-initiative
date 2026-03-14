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
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const listRes = await SELF.fetch(`${BASE}/partner`, {
      headers: { Authorization: `Bearer ${partnerToken}` },
    });
    expect(listRes.status).toBe(200);
    const list = (await listRes.json()) as {
      watching: Array<{
        id: string;
        user: { email: string };
        digest_cadence: string;
        immediate_tamper_severity: string;
      }>;
    };
    expect(list.watching[0]).toMatchObject({
      id: created.id,
      user: { email: 'notify-owner@example.com' },
      digest_cadence: 'daily',
      immediate_tamper_severity: 'critical',
    });

    const patchRes = await SELF.fetch(`${BASE}/partner/watching/${created.id}`, {
      method: 'PATCH',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({
        digest_cadence: 'alerts-only',
        immediate_tamper_severity: 'warning',
      }),
    });
    expect(patchRes.status).toBe(204);

    const updatedRes = await SELF.fetch(`${BASE}/partner`, {
      headers: { Authorization: `Bearer ${partnerToken}` },
    });
    const updated = (await updatedRes.json()) as {
      watching: Array<{
        id: string;
        digest_cadence: string;
        immediate_tamper_severity: string;
      }>;
    };
    expect(updated.watching[0]).toMatchObject({
      id: created.id,
      digest_cadence: 'alerts-only',
      immediate_tamper_severity: 'warning',
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
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const device = await createDeviceForUser(ownerToken, 'Workstation', 'linux');
    const logRes = await SELF.fetch(`${BASE}/d/log`, {
      method: 'POST',
      headers: authHeaders(device.access_token),
      body: JSON.stringify({ ts: Date.now(), type: 'service_stop', risk: 1, data: {} }),
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
      }),
    });
    const created = (await createRes.json()) as { id: string };
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'mute-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    await SELF.fetch(`${BASE}/partner/watching/${created.id}`, {
      method: 'PATCH',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ digest_cadence: 'none' }),
    });

    const device = await createDeviceForUser(ownerToken, 'Muted Device', 'linux');
    const baselineCount = (await listEmailDeliveries()).length;
    const logRes = await SELF.fetch(`${BASE}/d/log`, {
      method: 'POST',
      headers: authHeaders(device.access_token),
      body: JSON.stringify({ ts: Date.now(), type: 'service_stop', risk: 1, data: {} }),
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
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const device = await createDeviceForUser(ownerToken, 'Quiet Device', 'linux');
    const baselineCount = (await listEmailDeliveries()).length;
    const logRes = await SELF.fetch(`${BASE}/d/log`, {
      method: 'POST',
      headers: authHeaders(device.access_token),
      body: JSON.stringify({ ts: Date.now(), type: 'service_stop', risk: 1, data: {} }),
    });

    expect(logRes.status).toBe(201);
    expect(await listEmailDeliveries()).toHaveLength(baselineCount);
  });
});
