import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  listEmailDeliveries,
  signupAndGetCookie,
} from './helpers';

beforeEach(clearDB);

describe('GET /updates', () => {
  it('requires authentication', async () => {
    const res = await SELF.fetch(`${BASE}/updates`);
    expect(res.status).toBe(401);
  });

  it('returns the combined user, devices, and partners shape for a real invite/accept flow', async () => {
    const { cookie: ownerCookie, userId: ownerUserId } = await signupAndGetCookie(
      'owner@example.com',
      'pw',
      'Owner',
    );
    const { cookie: partnerCookie } = await signupAndGetCookie(
      'partner@example.com',
      'pw',
      'Partner',
    );
    const device = await createDeviceForUser(ownerCookie, 'Laptop', 'linux');

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({ email: 'partner@example.com' }),
    });
    expect(inviteRes.status).toBe(201);

    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) => delivery.kind === 'partner_invite',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(200);

    const ownerUpdatesRes = await SELF.fetch(`${BASE}/updates`, {
      headers: authHeaders(ownerCookie),
    });
    expect(ownerUpdatesRes.status).toBe(200);
    const ownerUpdates = (await ownerUpdatesRes.json()) as {
      user: { id: string; email: string };
      devices: Array<{ id: string; owner: string }>;
      partners: {
        watchers: Array<{ status: string; user: { email: string } }>;
        watching: Array<unknown>;
      };
    };

    expect(ownerUpdates.user).toMatchObject({ id: ownerUserId, email: 'owner@example.com' });
    expect(ownerUpdates.devices).toHaveLength(1);
    expect(ownerUpdates.devices[0]).toMatchObject({ id: device.id, owner: ownerUserId });
    expect(ownerUpdates.partners.watchers).toHaveLength(1);
    expect(ownerUpdates.partners.watchers[0]).toMatchObject({
      status: 'accepted',
      user: { email: 'partner@example.com' },
    });
    expect(ownerUpdates.partners.watching).toHaveLength(0);

    const partnerUpdatesRes = await SELF.fetch(`${BASE}/updates`, {
      headers: authHeaders(partnerCookie),
    });
    expect(partnerUpdatesRes.status).toBe(200);
    const partnerUpdates = (await partnerUpdatesRes.json()) as {
      partners: { watching: Array<{ status: string; user: { email: string } }> };
    };
    expect(partnerUpdates.partners.watching).toHaveLength(1);
    expect(partnerUpdates.partners.watching[0]).toMatchObject({
      status: 'accepted',
      user: { email: 'owner@example.com' },
    });
  });

  it('matches the independent /user, /device, and /partner responses for the same session', async () => {
    const { cookie: ownerCookie } = await signupAndGetCookie('owner2@example.com', 'pw', 'Owner');
    const { cookie: partnerCookie } = await signupAndGetCookie(
      'partner2@example.com',
      'pw',
      'Partner',
    );
    await createDeviceForUser(ownerCookie, 'Phone', 'ios');

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({ email: 'partner2@example.com' }),
    });
    expect(inviteRes.status).toBe(201);
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) => delivery.kind === 'partner_invite',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };
    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const [updatesRes, userRes, deviceRes, partnerRes] = await Promise.all([
      SELF.fetch(`${BASE}/updates`, { headers: authHeaders(ownerCookie) }),
      SELF.fetch(`${BASE}/user`, { headers: authHeaders(ownerCookie) }),
      SELF.fetch(`${BASE}/device`, { headers: authHeaders(ownerCookie) }),
      SELF.fetch(`${BASE}/partner`, { headers: authHeaders(ownerCookie) }),
    ]);

    expect(updatesRes.status).toBe(200);
    expect(userRes.status).toBe(200);
    expect(deviceRes.status).toBe(200);
    expect(partnerRes.status).toBe(200);

    const updates = (await updatesRes.json()) as {
      user: unknown;
      devices: unknown;
      partners: unknown;
    };
    const user = await userRes.json();
    const devices = await deviceRes.json();
    const partners = await partnerRes.json();

    expect(updates.user).toEqual(user);
    expect(updates.devices).toEqual(devices);
    expect(updates.partners).toEqual(partners);
  });
});
