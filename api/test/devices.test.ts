import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { fetchMock, SELF } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  listEmailDeliveries,
  markUserEmailVerified,
  signupAndGetCookie,
} from './helpers';
import { installHashServerMock } from './hash-server-mock';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

beforeEach(clearDB);

describe('Main device routes', () => {
  it('lists devices owned by the authenticated user', async () => {
    const { cookie } = await signupAndGetCookie('alice@example.com');
    await createDeviceForUser('alice@example.com', 'password123', 'Work Laptop', 'linux');

    const res = await SELF.fetch(`${BASE}/device`, {
      headers: authHeaders(cookie),
    });

    expect(res.status).toBe(200);
    const body = (await res.json()) as Array<{ name: string; platform: string }>;
    expect(body).toHaveLength(1);
    expect(body[0]).toMatchObject({ name: 'Work Laptop', platform: 'linux' });
  });

  it('updates an owned device', async () => {
    const { cookie } = await signupAndGetCookie('bob@example.com');
    const device = await createDeviceForUser('bob@example.com', 'password123', 'Old Name', 'macos');

    const res = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'PATCH',
      headers: authHeaders(cookie),
      body: JSON.stringify({ name: 'New Name' }),
    });

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ id: device.id, updated: true });

    const listRes = await SELF.fetch(`${BASE}/device`, {
      headers: authHeaders(cookie),
    });
    const list = (await listRes.json()) as Array<{ id: string; name: string }>;
    expect(list.find((item) => item.id === device.id)).toMatchObject({
      name: 'New Name',
    });
  });

  it('forbids patching a device owned by another user', async () => {
    await signupAndGetCookie('owner@example.com');
    const { cookie: attackerCookie } = await signupAndGetCookie('attacker@example.com');
    const device = await createDeviceForUser('owner@example.com');

    const res = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'PATCH',
      headers: authHeaders(attackerCookie),
      body: JSON.stringify({ name: 'nope' }),
    });

    expect(res.status).toBe(404);
  });

  it('lists owner devices to an accepted partner', async () => {
    const { cookie: ownerCookie } = await signupAndGetCookie('owner2@example.com');
    const { cookie: partnerCookie, userId: partnerUserId } =
      await signupAndGetCookie('partner2@example.com');
    await markUserEmailVerified(partnerUserId);
    const device = await createDeviceForUser(
      'owner2@example.com',
      'password123',
      'Owner Phone',
      'android',
    );

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
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
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(200);

    const beforeConfirmRes = await SELF.fetch(`${BASE}/device`, {
      headers: authHeaders(partnerCookie),
    });
    expect(beforeConfirmRes.status).toBe(200);
    const beforeConfirm = (await beforeConfirmRes.json()) as Array<{ id: string }>;
    expect(beforeConfirm.find((item) => item.id === device.id)).toBeTruthy();
  });

  it('deletes an owned device and sends a notification email', async () => {
    const { cookie, userId } = await signupAndGetCookie('delete-device@example.com');
    await markUserEmailVerified(userId);
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'delete-device-partner@example.com',
    );
    await markUserEmailVerified(partnerUserId);
    const device = await createDeviceForUser(
      'delete-device@example.com',
      'password123',
      'Delete Me',
      'linux',
    );

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(cookie),
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
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(200);

    const deleteRes = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'DELETE',
      headers: authHeaders(cookie),
    });
    expect(deleteRes.status).toBe(204);

    const listRes = await SELF.fetch(`${BASE}/device`, {
      headers: authHeaders(cookie),
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

  it('sends deletion notifications to accepted partners even if emails are unverified', async () => {
    const { cookie } = await signupAndGetCookie('delete-device-unverified@example.com');
    const { cookie: partnerCookie } = await signupAndGetCookie(
      'delete-device-unverified-partner@example.com',
    );
    const device = await createDeviceForUser(
      'delete-device-unverified@example.com',
      'password123',
      'Unverified Delete',
      'linux',
    );

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(cookie),
      body: JSON.stringify({
        email: 'delete-device-unverified-partner@example.com',
      }),
    });
    expect(inviteRes.status).toBe(201);
    await inviteRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'delete-device-unverified-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(200);

    const deleteRes = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'DELETE',
      headers: authHeaders(cookie),
    });
    expect(deleteRes.status).toBe(204);

    const deletionEmails = (await listEmailDeliveries()).filter(
      (delivery) => delivery.kind === 'device_deleted',
    );
    expect(
      deletionEmails.some(
        (delivery) => delivery.recipient_email === 'delete-device-unverified-partner@example.com',
      ),
    ).toBe(true);
  });
});
