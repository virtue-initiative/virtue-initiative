import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { env, fetchMock, SELF } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  batchMetadataForm,
  clearDB,
  createDeviceForUser,
  listEmailDeliveries,
  markUserEmailVerified,
  signupAndGetCookie,
} from './helpers';
import { installHashServerMock, seedHashState } from './hash-server-mock';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

beforeEach(clearDB);

describe('Main device routes', () => {
  it('lists devices owned by the authenticated user (API-029)', async () => {
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

  it("reports last_hash_at as milliseconds, converted from the hash server's unix-second last_received (API-029)", async () => {
    const { cookie } = await signupAndGetCookie('carol@example.com');
    const device = await createDeviceForUser(
      'carol@example.com',
      'password123',
      'Phone',
      'android',
    );
    seedHashState(device.id, { hash: '0a'.repeat(32), seq: 3, last_received: 1_700_000_000 });

    const res = await SELF.fetch(`${BASE}/device`, {
      headers: authHeaders(cookie),
    });

    expect(res.status).toBe(200);
    const body = (await res.json()) as Array<{ id: string; last_hash_at: number | null }>;
    const found = body.find((item) => item.id === device.id);
    expect(found?.last_hash_at).toBe(1_700_000_000_000);
  });

  it('derives status from last_hash_at, not from batches (API-029)', async () => {
    const { cookie } = await signupAndGetCookie('dave@example.com');
    const recent = await createDeviceForUser('dave@example.com', 'password123', 'Laptop', 'linux');
    const stale = await createDeviceForUser('dave@example.com', 'password123', 'Phone', 'android');
    const silent = await createDeviceForUser('dave@example.com', 'password123', 'Tablet', 'ios');
    const nowSeconds = Math.floor(Date.now() / 1000);
    seedHashState(recent.id, { hash: '0a'.repeat(32), seq: 1, last_received: nowSeconds - 600 });
    seedHashState(stale.id, {
      hash: '0b'.repeat(32),
      seq: 1,
      last_received: nowSeconds - 3 * 60 * 60,
    });

    const res = await SELF.fetch(`${BASE}/device`, {
      headers: authHeaders(cookie),
    });

    expect(res.status).toBe(200);
    const body = (await res.json()) as Array<{
      id: string;
      status: string;
      last_hash_at: number | null;
    }>;
    const byId = new Map(body.map((item) => [item.id, item]));
    expect(byId.get(recent.id)?.status).toBe('online');
    expect(byId.get(stale.id)?.status).toBe('offline');
    expect(byId.get(silent.id)).toMatchObject({ status: 'offline', last_hash_at: null });
    expect(body[0]).not.toHaveProperty('last_upload_at');
  });

  it('updates an owned device (API-030)', async () => {
    const { cookie } = await signupAndGetCookie('bob@example.com');
    const device = await createDeviceForUser('bob@example.com', 'password123', 'Old Name', 'macos');

    const res = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'PATCH',
      headers: authHeaders(cookie),
      body: JSON.stringify({ name: 'New Name' }),
    });

    expect(res.status).toBe(204);

    const listRes = await SELF.fetch(`${BASE}/device`, {
      headers: authHeaders(cookie),
    });
    const list = (await listRes.json()) as Array<{ id: string; name: string }>;
    expect(list.find((item) => item.id === device.id)).toMatchObject({
      name: 'New Name',
    });
  });

  it('forbids patching a device owned by another user (API-030)', async () => {
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

  it('returns dns_filter_domain as null for a device that has none (API-029)', async () => {
    const { cookie } = await signupAndGetCookie('nodns@example.com');
    await createDeviceForUser('nodns@example.com');

    const res = await SELF.fetch(`${BASE}/device`, { headers: authHeaders(cookie) });

    const body = (await res.json()) as Array<{ dns_filter_domain: string | null }>;
    expect(body[0]).toHaveProperty('dns_filter_domain', null);
  });

  it('sets, changes and clears the DNS filter domain, normalised, across device list and device settings (API-030, API-029, API-036, CORE-027)', async () => {
    const { cookie } = await signupAndGetCookie('dns@example.com');
    const device = await createDeviceForUser('dns@example.com', 'password123', 'Desk', 'linux');
    const patch = (body: unknown) =>
      SELF.fetch(`${BASE}/device/${device.id}`, {
        method: 'PATCH',
        headers: authHeaders(cookie),
        body: JSON.stringify(body),
      });
    const listed = async () => {
      const list = (await (
        await SELF.fetch(`${BASE}/device`, { headers: authHeaders(cookie) })
      ).json()) as Array<{ id: string; name: string; dns_filter_domain: string | null }>;
      return list.find((item) => item.id === device.id)!;
    };
    const onDevice = async () =>
      (
        (await (
          await SELF.fetch(`${BASE}/d/device`, {
            headers: { Authorization: `Bearer ${device.refresh_token}` },
          })
        ).json()) as { dns_filter_domain: string | null }
      ).dns_filter_domain;

    expect((await patch({ dns_filter_domain: '  Blocked.Example.NET ' })).status).toBe(204);
    expect(await listed()).toMatchObject({
      name: 'Desk',
      dns_filter_domain: 'blocked.example.net',
    });
    expect(await onDevice()).toBe('blocked.example.net');

    expect((await patch({ dns_filter_domain: 'other.example.org' })).status).toBe(204);
    expect((await listed()).dns_filter_domain).toBe('other.example.org');

    expect((await patch({ name: 'Renamed' })).status).toBe(204);
    expect(await listed()).toMatchObject({
      name: 'Renamed',
      dns_filter_domain: 'other.example.org',
    });

    expect((await patch({ dns_filter_domain: null })).status).toBe(204);
    expect((await listed()).dns_filter_domain).toBeNull();
    expect(await onDevice()).toBeNull();
  });

  it.each([
    ['a scheme', 'https://example.net'],
    ['a port', 'example.net:8080'],
    ['a space', 'example .net'],
    ['a path', 'example.net/path'],
    ['no dot', 'localhost'],
    ['an empty string', ''],
    [
      'more than 253 characters',
      `${'a'.repeat(63)}.${'b'.repeat(63)}.${'c'.repeat(63)}.${'d'.repeat(63)}.net`,
    ],
  ])('rejects a DNS filter domain with %s (API-030)', async (_name, domain) => {
    const { cookie } = await signupAndGetCookie('baddns@example.com');
    const device = await createDeviceForUser('baddns@example.com');

    const res = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'PATCH',
      headers: authHeaders(cookie),
      body: JSON.stringify({ dns_filter_domain: domain }),
    });

    expect(res.status).toBe(400);
    const list = (await (
      await SELF.fetch(`${BASE}/device`, { headers: authHeaders(cookie) })
    ).json()) as Array<{ dns_filter_domain: string | null }>;
    expect(list[0]!.dns_filter_domain).toBeNull();
  });

  it('forbids setting the DNS filter domain of a device owned by another user (API-030)', async () => {
    await signupAndGetCookie('dnsowner@example.com');
    const { cookie: attackerCookie } = await signupAndGetCookie('dnsattacker@example.com');
    const device = await createDeviceForUser('dnsowner@example.com');

    const res = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'PATCH',
      headers: authHeaders(attackerCookie),
      body: JSON.stringify({ dns_filter_domain: 'example.net' }),
    });

    expect(res.status).toBe(404);
    const row = await env.DB.prepare('SELECT dns_filter_domain FROM devices').first<{
      dns_filter_domain: string | null;
    }>();
    expect(row?.dns_filter_domain).toBeNull();
  });

  it('lists owner devices to an accepted partner (API-029)', async () => {
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
    expect(inviteRes.status).toBe(200);
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

  it("deletes a device's batch objects from R2 along with the device (API-031)", async () => {
    const { cookie, userId } = await signupAndGetCookie('wipe@example.com');
    const device = await createDeviceForUser('wipe@example.com', 'password123', 'Wipe Me', 'linux');

    const batchRes = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.refresh_token}` },
      body: batchMetadataForm({
        start_time: 1_000,
        end_time: 2_000,
        access_keys: { [userId]: Buffer.from('envelope').toString('base64') },
      }),
    });
    expect(batchRes.status).toBe(200);
    const batch = (await batchRes.json()) as { id: string };
    const key = `user/${userId}/batches/${batch.id}.enc`;
    expect(await env.BUCKET.head(key)).not.toBeNull();

    const res = await SELF.fetch(`${BASE}/device/${device.id}`, {
      method: 'DELETE',
      headers: authHeaders(cookie),
    });

    expect(res.status).toBe(204);
    expect(await env.BUCKET.head(key)).toBeNull();
  });

  it('deletes an owned device and sends a notification email (API-031)', async () => {
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
    expect(inviteRes.status).toBe(200);
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

  it('sends deletion notifications to accepted partners even if emails are unverified (API-031)', async () => {
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
    expect(inviteRes.status).toBe(200);
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
