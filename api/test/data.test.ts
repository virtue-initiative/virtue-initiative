import { beforeEach, describe, expect, it } from 'vitest';
import { SELF, env } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  createServerToken,
  listEmailDeliveries,
  signupAndGetCookie,
  uuidToBytes,
} from './helpers';

beforeEach(clearDB);

describe('Data and device API routes', () => {
  it('handles device registration, settings, log upload, batch upload, and filtered data listing', async () => {
    const { cookie: userCookie, userId } = await signupAndGetCookie('alice@example.com');
    const device = await createDeviceForUser(userCookie, 'Phone', 'ios');

    const deviceInfoRes = await SELF.fetch(`${BASE}/d/device`, {
      headers: { Authorization: `Bearer ${device.refresh_token}` },
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

    // Get a hash JWT from POST /d/token
    const hashTokenRes = await SELF.fetch(`${BASE}/d/token`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.refresh_token}` },
    });
    expect(hashTokenRes.status).toBe(200);
    const { hash_token } = (await hashTokenRes.json()) as { hash_token: string };

    const hashUploadRes = await SELF.fetch(`${BASE}/hash`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${hash_token}` },
      body: new Uint8Array(32).fill(7),
    });
    expect(hashUploadRes.status).toBe(200);

    const logRes = await SELF.fetch(`${BASE}/d/log`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${device.refresh_token}`,
      },
      body: JSON.stringify({ ts: 1710000000000, type: 'system_event', data: { event: 'startup' } }),
    });
    expect(logRes.status).toBe(201);
    const uploadedLog = (await logRes.json()) as {
      id: string;
      type: string;
      data: { event: string };
    };
    expect(uploadedLog.id).toBeTruthy();
    expect(uploadedLog.type).toBe('system_event');
    expect(uploadedLog.data.event).toBe('startup');

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
      headers: { Authorization: `Bearer ${device.refresh_token}` },
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

    const dataRes = await SELF.fetch(`${BASE}/data?since=0`, {
      headers: authHeaders(userCookie),
    });
    expect(dataRes.status).toBe(200);
    const data = (await dataRes.json()) as {
      batches: Array<{
        device_id: string;
        end_hash: string;
        encrypted_key: string;
        created_at: number;
      }>;
      logs: Array<{
        id: string;
        device_id: string;
        type: string;
        data: { event: string };
        created_at: number;
      }>;
    };
    expect(data.batches[0]).toMatchObject({
      device_id: device.id,
      encrypted_key: Buffer.from('owner-envelope').toString('base64'),
    });
    expect(data.batches[0]?.created_at).toEqual(expect.any(Number));
    expect(data.logs[0]).toMatchObject({
      device_id: device.id,
      type: 'system_event',
      data: { event: 'startup' },
    });
    expect(data.logs[0]?.id).toBeTruthy();
    expect(data.logs[0]?.created_at).toEqual(expect.any(Number));

    const serverToken = await createServerToken(device.id);
    const resetRes = await SELF.fetch(`${BASE}/hash`, {
      method: 'DELETE',
      headers: { Authorization: `Bearer ${serverToken}` },
    });
    expect(resetRes.status).toBe(200);
  });

  it("returns the accepted partner's batch envelope and owner logs", async () => {
    const { cookie: ownerCookie, userId: ownerUserId } =
      await signupAndGetCookie('owner@example.com');
    const { cookie: partnerCookie, userId: partnerUserId } =
      await signupAndGetCookie('partner@example.com');
    const device = await createDeviceForUser(ownerCookie);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
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
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    await SELF.fetch(`${BASE}/d/log`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${device.refresh_token}`,
      },
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
      headers: { Authorization: `Bearer ${device.refresh_token}` },
      body: form,
    });
    expect(batchUploadRes.status).toBe(201);

    const ownerDataRes = await SELF.fetch(`${BASE}/data?since=0`, {
      headers: authHeaders(ownerCookie),
    });
    expect(ownerDataRes.status).toBe(200);
    const ownerData = (await ownerDataRes.json()) as {
      batches: Array<{ encrypted_key: string }>;
    };
    expect(ownerData.batches[0]?.encrypted_key).toBe(
      Buffer.from('owner-envelope').toString('base64'),
    );

    const partnerDataRes = await SELF.fetch(
      `${BASE}/data?since=0&user=${encodeURIComponent(ownerUserId)}`,
      {
        headers: authHeaders(partnerCookie),
      },
    );
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
