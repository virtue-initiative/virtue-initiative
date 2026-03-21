import { beforeEach, describe, expect, it } from 'vitest';
import { SELF, env } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  createServerToken,
  listEmailDeliveries,
  signupAndGetToken,
  uuidToBytes,
} from './helpers';

beforeEach(clearDB);

describe('Data and device API routes', () => {
  it('handles device registration, settings, log upload, batch upload, and filtered data listing', async () => {
    const { token: userToken, userId } = await signupAndGetToken('alice@example.com');
    const device = await createDeviceForUser(userToken, 'Phone', 'ios');

    const deviceInfoRes = await SELF.fetch(`${BASE}/d/device`, {
      headers: { Authorization: `Bearer ${device.access_token}` },
    });
    expect(deviceInfoRes.status).toBe(200);
    const deviceInfo = (await deviceInfoRes.json()) as {
      owner?: { user_id: string; pub_key: string };
      partners: Array<{ user_id: string; pub_key: string }>;
      hash_base_url: string;
    };
    expect(deviceInfo.owner?.user_id).toBe(userId);
    expect(deviceInfo.partners).toEqual([]);
    expect(deviceInfo.hash_base_url).toBeTruthy();

    const hashUploadRes = await SELF.fetch(`${BASE}/hash`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.access_token}` },
      body: new Uint8Array(32).fill(7),
    });
    expect(hashUploadRes.status).toBe(200);

    const logRes = await SELF.fetch(`${BASE}/d/log`, {
      method: 'POST',
      headers: authHeaders(device.access_token),
      body: JSON.stringify({ ts: 1710000000000, type: 'system_event', data: { event: 'startup' } }),
    });
    expect(logRes.status).toBe(201);

    const form = new FormData();
    form.set('start_time', '1710000000000');
    form.set('end_time', '1710003600000');
    form.set(
      'access_keys',
      JSON.stringify({
        keys: [
          {
            user_id: userId,
            hpke_key: Buffer.from('owner-envelope').toString('base64'),
          },
        ],
      }),
    );
    form.set('file', new File([new Uint8Array([1, 2, 3])], 'batch.enc'));
    const batchRes = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.access_token}` },
      body: form,
    });
    expect(batchRes.status).toBe(201);
    const batch = (await batchRes.json()) as {
      id: string;
      end_hash: string;
      url: string;
      start_time: number;
      end_time: number;
    };
    expect(batch.id).toBeTruthy();
    expect(batch.end_hash).toHaveLength(64);
    expect(batch.url).toContain('/user/');

    const storedBatch = await env.DB.prepare('SELECT access_keys FROM batches WHERE id = ?')
      .bind(uuidToBytes(batch.id))
      .first<{ access_keys: string }>();
    expect(JSON.parse(storedBatch!.access_keys)).toEqual({
      keys: [
        {
          user_id: userId,
          hpke_key: Buffer.from('owner-envelope').toString('base64'),
        },
      ],
    });

    const refreshRes = await SELF.fetch(`${BASE}/d/token`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ refresh_token: device.refresh_token }),
    });
    expect(refreshRes.status).toBe(200);

    const dataRes = await SELF.fetch(`${BASE}/data`, {
      headers: { Authorization: `Bearer ${userToken}` },
    });
    expect(dataRes.status).toBe(200);
    const data = (await dataRes.json()) as {
      batches: Array<{ device_id: string; end_hash: string; encrypted_key: string }>;
      logs: Array<{ device_id: string; type: string; data: { event: string } }>;
    };
    expect(data.batches[0]).toMatchObject({
      device_id: device.id,
      encrypted_key: Buffer.from('owner-envelope').toString('base64'),
    });
    expect(data.logs[0]).toMatchObject({
      device_id: device.id,
      type: 'system_event',
      data: { event: 'startup' },
    });

    const serverToken = await createServerToken(device.id);
    const resetRes = await SELF.fetch(`${BASE}/hash`, {
      method: 'DELETE',
      headers: { Authorization: `Bearer ${serverToken}` },
    });
    expect(resetRes.status).toBe(200);
  });

  it("returns the accepted partner's batch envelope and owner logs", async () => {
    const { token: ownerToken, userId: ownerUserId } = await signupAndGetToken('owner@example.com');
    const { token: partnerToken, userId: partnerUserId } = await signupAndGetToken(
      'partner@example.com',
    );
    const device = await createDeviceForUser(ownerToken);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({ email: 'partner@example.com' }),
    });
    await inviteRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' && delivery.recipient_email === 'partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };
    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    await SELF.fetch(`${BASE}/d/log`, {
      method: 'POST',
      headers: authHeaders(device.access_token),
      body: JSON.stringify({ ts: 1710000000000, type: 'system_event', data: { event: 'startup' } }),
    });

    const form = new FormData();
    form.set('start_time', '1710000000000');
    form.set('end_time', '1710003600000');
    form.set(
      'access_keys',
      JSON.stringify({
        keys: [
          {
            user_id: ownerUserId,
            hpke_key: Buffer.from('owner-envelope').toString('base64'),
          },
          {
            user_id: partnerUserId,
            hpke_key: Buffer.from('partner-envelope').toString('base64'),
          },
        ],
      }),
    );
    form.set('file', new File([new Uint8Array([1, 2, 3])], 'batch.enc'));
    const batchUploadRes = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.access_token}` },
      body: form,
    });
    expect(batchUploadRes.status).toBe(201);

    const ownerDataRes = await SELF.fetch(`${BASE}/data`, {
      headers: { Authorization: `Bearer ${ownerToken}` },
    });
    expect(ownerDataRes.status).toBe(200);
    const ownerData = (await ownerDataRes.json()) as {
      batches: Array<{ encrypted_key: string }>;
    };
    expect(ownerData.batches[0]?.encrypted_key).toBe(
      Buffer.from('owner-envelope').toString('base64'),
    );

    const partnerDataRes = await SELF.fetch(`${BASE}/data?user=${encodeURIComponent(ownerUserId)}`, {
      headers: { Authorization: `Bearer ${partnerToken}` },
    });
    expect(partnerDataRes.status).toBe(200);
    const partnerData = (await partnerDataRes.json()) as {
      batches: Array<{ encrypted_key: string }>;
      logs: Array<{ device_id: string }>;
    };
    expect(partnerData.batches[0]?.encrypted_key).toBe(
      Buffer.from('partner-envelope').toString('base64'),
    );
    expect(partnerData.logs[0]?.device_id).toBe(device.id);
  });
});
