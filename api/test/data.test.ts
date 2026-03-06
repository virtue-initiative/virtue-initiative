import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  createServerToken,
  signupAndGetToken,
} from './helpers';

beforeEach(clearDB);

describe('Data and device API routes', () => {
  it('handles device registration, token refresh, log upload, batch upload, and data listing', async () => {
    const { token: userToken } = await signupAndGetToken('alice@example.com');
    const device = await createDeviceForUser(userToken, 'Phone', 'ios');

    const deviceInfoRes = await SELF.fetch(`${BASE}/d/device`, {
      headers: { Authorization: `Bearer ${device.access_token}` },
    });
    expect(deviceInfoRes.status).toBe(200);

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
    form.set('start', '1710000000000');
    form.set('end', '1710003600000');
    form.set('file', new File([new Uint8Array([1, 2, 3])], 'batch.enc'));
    const batchRes = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.access_token}` },
      body: form,
    });
    expect(batchRes.status).toBe(201);
    const batch = (await batchRes.json()) as { id: string; end_hash: string; url: string };
    expect(batch.id).toBeTruthy();
    expect(batch.end_hash).toHaveLength(64);
    expect(batch.url).toContain('/user/');

    const hashReadRes = await SELF.fetch(`${BASE}/hash`, {
      headers: { Authorization: `Bearer ${device.access_token}` },
    });
    expect(hashReadRes.status).toBe(200);
    expect((await hashReadRes.arrayBuffer()).byteLength).toBe(32);

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
      batches: Array<{ device_id: string; end_hash: string }>;
      logs: Array<{ device_id: string; type: string; data: { event: string } }>;
    };
    expect(data.batches[0].device_id).toBe(device.id);
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

  it("allows an accepted partner with view_data to read another user's data", async () => {
    const { token: ownerToken, userId: ownerUserId } = await signupAndGetToken('owner@example.com');
    const { token: partnerToken } = await signupAndGetToken('partner@example.com');
    const device = await createDeviceForUser(ownerToken);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({ email: 'partner@example.com', permissions: { view_data: true } }),
    });
    const invite = (await inviteRes.json()) as { id: string };
    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ id: invite.id }),
    });

    await SELF.fetch(`${BASE}/d/log`, {
      method: 'POST',
      headers: authHeaders(device.access_token),
      body: JSON.stringify({ ts: 1710000000000, type: 'system_event', data: { event: 'startup' } }),
    });

    const res = await SELF.fetch(`${BASE}/data?user=${encodeURIComponent(ownerUserId)}`, {
      headers: { Authorization: `Bearer ${partnerToken}` },
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { logs: Array<{ device_id: string }> };
    expect(body.logs[0].device_id).toBe(device.id);
  });
});
