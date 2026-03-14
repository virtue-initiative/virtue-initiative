import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  listEmailDeliveries,
  markUserEmailVerified,
  signupAndGetToken,
} from './helpers';

beforeEach(clearDB);

describe('Main device routes', () => {
  it('lists devices owned by the authenticated user', async () => {
    const { token } = await signupAndGetToken('alice@example.com');
    await createDeviceForUser(token, 'Work Laptop', 'linux');

    const res = await SELF.fetch(`${BASE}/device`, {
      headers: { Authorization: `Bearer ${token}` },
    });

    expect(res.status).toBe(200);
    const body = (await res.json()) as Array<{ name: string; platform: string; enabled: boolean }>;
    expect(body).toHaveLength(1);
    expect(body[0]).toMatchObject({ name: 'Work Laptop', platform: 'linux', enabled: true });
  });

  it('updates an owned device', async () => {
    const { token } = await signupAndGetToken('bob@example.com');
    const device = await createDeviceForUser(token, 'Old Name', 'macos');

    const res = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'PATCH',
      headers: authHeaders(token),
      body: JSON.stringify({ name: 'New Name', enabled: false }),
    });

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ id: device.id, updated: true });

    const listRes = await SELF.fetch(`${BASE}/device`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    const list = (await listRes.json()) as Array<{ id: string; name: string; enabled: boolean }>;
    expect(list.find((item) => item.id === device.id)).toMatchObject({
      name: 'New Name',
      enabled: false,
    });
  });

  it('forbids patching a device owned by another user', async () => {
    const { token: ownerToken } = await signupAndGetToken('owner@example.com');
    const { token: attackerToken } = await signupAndGetToken('attacker@example.com');
    const device = await createDeviceForUser(ownerToken);

    const res = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'PATCH',
      headers: authHeaders(attackerToken),
      body: JSON.stringify({ name: 'nope' }),
    });

    expect(res.status).toBe(404);
  });

  it('lists owner devices to an accepted partner', async () => {
    const { token: ownerToken } = await signupAndGetToken('owner2@example.com');
    const { token: partnerToken, userId: partnerUserId } =
      await signupAndGetToken('partner2@example.com');
    await markUserEmailVerified(partnerUserId);
    const device = await createDeviceForUser(ownerToken, 'Owner Phone', 'android');

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({ email: 'partner2@example.com' }),
    });
    expect(inviteRes.status).toBe(201);
    await inviteRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' && delivery.recipient_email === 'partner2@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(200);

    const beforeConfirmRes = await SELF.fetch(`${BASE}/device`, {
      headers: { Authorization: `Bearer ${partnerToken}` },
    });
    expect(beforeConfirmRes.status).toBe(200);
    const beforeConfirm = (await beforeConfirmRes.json()) as Array<{ id: string }>;
    expect(beforeConfirm.find((item) => item.id === device.id)).toBeTruthy();
  });

  it('deletes an owned device and sends a notification email', async () => {
    const { token, userId } = await signupAndGetToken('delete-device@example.com');
    await markUserEmailVerified(userId);
    const { token: partnerToken, userId: partnerUserId } = await signupAndGetToken(
      'delete-device-partner@example.com',
    );
    await markUserEmailVerified(partnerUserId);
    const device = await createDeviceForUser(token, 'Delete Me', 'linux');

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({
        email: 'delete-device-partner@example.com',
      }),
    });
    expect(inviteRes.status).toBe(201);
    await inviteRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'delete-device-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(200);

    const deleteRes = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'DELETE',
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(deleteRes.status).toBe(204);

    const listRes = await SELF.fetch(`${BASE}/device`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    const list = (await listRes.json()) as Array<{ id: string }>;
    expect(list.find((item) => item.id === device.id)).toBeUndefined();

    const deliveries = await listEmailDeliveries();
    const deletionEmails = deliveries.filter((delivery) => delivery.kind === 'device_deleted');
    expect(deletionEmails).toHaveLength(2);
    expect(
      deletionEmails.some((delivery) => delivery.recipient_email === 'delete-device@example.com'),
    ).toBe(true);
    expect(
      deletionEmails.some(
        (delivery) => delivery.recipient_email === 'delete-device-partner@example.com',
      ),
    ).toBe(true);
    expect(
      deletionEmails.some(
        (delivery) =>
          delivery.recipient_email === 'delete-device-partner@example.com' &&
          delivery.text.includes('deleted the device "Delete Me"'),
      ),
    ).toBe(true);
  });
});
