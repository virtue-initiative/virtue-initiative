import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { fetchMock, SELF, env } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  batchMetadataForm,
  clearDB,
  createDeviceForUser,
  listEmailDeliveries,
  signupAndGetCookie,
  uuidToBytes,
} from './helpers';
import { installHashServerMock, seedHashState } from './hash-server-mock';
import { decode } from '@msgpack/msgpack';
import { CURRENT_API_VERSION } from '../src/lib/api-version';
import { createBatch } from '../src/lib/db';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

beforeEach(clearDB);

describe('Data and device API routes', () => {
  it('refuses a batch when the hash server returns a malformed hash', async () => {
    await signupAndGetCookie('badhash@example.com');
    const device = await createDeviceForUser('badhash@example.com', 'password123', 'PC', 'linux');
    seedHashState(device.id, { hash: 'abc', seq: 1, last_received: 1000 });
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});

    const res = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.refresh_token}` },
      body: batchMetadataForm({ start_time: 1_000, end_time: 2_000, access_keys: {} }),
    });

    expect(res.status).toBe(502);
    const count = await env.DB.prepare('SELECT COUNT(*) AS n FROM batches').first<{ n: number }>();
    expect(count?.n).toBe(0);
    error.mockRestore();
  });

  it('does not store a batch for a device that no longer exists', async () => {
    const { userId } = await signupAndGetCookie('gone@example.com');
    const result = await createBatch(env.DB, {
      id: '00000000-0000-4000-8000-0000000000aa',
      user_id: userId,
      device_id: '00000000-0000-4000-8000-0000000000bb',
      start_time: 1_000,
      end_time: 2_000,
      end_hash: new Uint8Array(32).buffer,
      access_keys: new Uint8Array(),
      version: CURRENT_API_VERSION,
      created_at: 2_000,
    });

    expect(result.meta.changes).toBe(0);
  });

  it('leaves out and logs a batch whose access keys cannot be decoded', async () => {
    const { cookie, userId } = await signupAndGetCookie('corrupt@example.com');
    const device = await createDeviceForUser('corrupt@example.com', 'password123', 'PC', 'linux');
    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, start_time, end_time, end_hash, access_keys, version, created_at)
       VALUES (?, ?, ?, 1000, 2000, ?, ?, ?, 2000)`,
    )
      .bind(
        uuidToBytes('00000000-0000-4000-8000-0000000000cc'),
        uuidToBytes(userId),
        uuidToBytes(device.id),
        new Uint8Array(32),
        new Uint8Array([0xc1]),
        CURRENT_API_VERSION,
      )
      .run();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

    const res = await SELF.fetch(`${BASE}/data?since=0`, { headers: authHeaders(cookie) });

    expect(res.status).toBe(200);
    expect(((await res.json()) as { batches: unknown[] }).batches).toHaveLength(0);
    expect(warn).toHaveBeenCalledWith(
      expect.stringContaining('00000000-0000-4000-8000-0000000000cc'),
      expect.anything(),
    );
    warn.mockRestore();
  });

  it('clamps batch times from a device clock running ahead, and logs a warning', async () => {
    await signupAndGetCookie('skew@example.com');
    const device = await createDeviceForUser('skew@example.com', 'password123', 'Laptop', 'linux');
    seedHashState(device.id, { hash: '07'.repeat(32), seq: 1, last_received: 1000 });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const before = Date.now();
    const twoDays = 2 * 24 * 60 * 60 * 1000;

    const res = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.refresh_token}` },
      body: batchMetadataForm({
        start_time: before + twoDays - 3_600_000,
        end_time: before + twoDays,
        access_keys: {},
      }),
    });

    expect(res.status).toBe(200);
    const batch = (await res.json()) as { id: string; start_time: number; end_time: number };
    const stored = await env.DB.prepare(
      'SELECT start_time, end_time, created_at FROM batches WHERE id = ?',
    )
      .bind(uuidToBytes(batch.id))
      .first<{ start_time: number; end_time: number; created_at: number }>();
    expect(stored!.end_time).toBe(stored!.created_at);
    expect(stored!.start_time).toBe(stored!.created_at);
    expect(batch.end_time).toBe(stored!.end_time);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('clamped'));
    warn.mockRestore();
  });

  it('handles device registration, settings, batch upload, and data listing', async () => {
    const { cookie: userCookie, userId } = await signupAndGetCookie('alice@example.com');
    const device = await createDeviceForUser('alice@example.com', 'password123', 'Phone', 'ios');

    const deviceInfoRes = await SELF.fetch(`${BASE}/d/device`, {
      headers: { Authorization: `Bearer ${device.refresh_token}` },
    });
    expect(deviceInfoRes.status).toBe(200);
    const deviceInfo = (await deviceInfoRes.json()) as {
      wrapping_keys: Array<{ user_id: string; pub_key: string }>;
      hash_base_url: string;
      hash_token: string;
    };
    expect(deviceInfo.wrapping_keys).toHaveLength(1);
    expect(deviceInfo.wrapping_keys[0]?.user_id).toBe(userId);
    expect(deviceInfo.hash_base_url).toBeTruthy();
    expect(deviceInfo.hash_token).toBeTruthy();

    // Simulates the client having already uploaded a hash directly to the (mocked)
    // hash server — the API itself never POSTs there, so there's no request to make.
    seedHashState(device.id, { hash: '07'.repeat(32), seq: 1, last_received: 1000 });

    const form = batchMetadataForm({
      start_time: 1710000000000,
      end_time: 1710003600000,
      access_keys: { [userId]: Buffer.from('owner-envelope').toString('base64') },
    });
    const batchRes = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.refresh_token}` },
      body: form,
    });
    expect(batchRes.status).toBe(200);
    const batch = (await batchRes.json()) as {
      id: string;
      end_hash: string;
      url: string;
      start_time: number;
      end_time: number;
      settings: { hash_token: string };
    };
    expect(batch.id).toBeTruthy();
    expect(batch.end_hash).toBe('07'.repeat(32));
    expect(batch.url).toContain('/user/');
    expect(batch.settings.hash_token).toBeTruthy();

    expect(batch.url).toBe(`${env.R2_URL}/user/${userId}/batches/${batch.id}.enc`);

    // The row holds the hash as raw bytes and the access keys as msgpack pairs of
    // [user id bytes, wrapped key bytes], and no url.
    const storedBatch = await env.DB.prepare('SELECT * FROM batches WHERE id = ?')
      .bind(uuidToBytes(batch.id))
      .first<{ access_keys: number[]; end_hash: number[]; version: string; url?: string }>();
    expect(decode(Uint8Array.from(storedBatch!.access_keys))).toEqual([
      [new Uint8Array(uuidToBytes(userId)), new Uint8Array(Buffer.from('owner-envelope'))],
    ]);
    expect(Buffer.from(storedBatch!.end_hash).toString('hex')).toBe('07'.repeat(32));
    expect(storedBatch!.version).toBe(CURRENT_API_VERSION);
    expect(storedBatch).not.toHaveProperty('url');

    const dataRes = await SELF.fetch(`${BASE}/data?since=0`, {
      headers: authHeaders(userCookie),
    });
    expect(dataRes.status).toBe(200);
    const data = (await dataRes.json()) as {
      batches: Array<{
        user_id: string;
        device_id: string;
        url: string;
        end_hash: string;
        version: string;
        encrypted_key: string;
        created_at: number;
      }>;
      user: { id: string; email: string };
      watching: unknown[];
      watchers: unknown[];
    };
    expect(data.batches[0]).toMatchObject({
      user_id: userId,
      device_id: device.id,
      url: batch.url,
      end_hash: '07'.repeat(32),
      version: CURRENT_API_VERSION,
      encrypted_key: Buffer.from('owner-envelope').toString('base64'),
    });
    expect(data.batches[0]?.created_at).toEqual(expect.any(Number));
    expect(data.user).toMatchObject({ id: userId, email: 'alice@example.com' });
    expect(data.watching).toEqual([]);
    expect(data.watchers).toEqual([]);
  });

  it("bundles an accepted partner's batches into the watcher's own GET /data response", async () => {
    const { cookie: ownerCookie, userId: ownerUserId } =
      await signupAndGetCookie('owner@example.com');
    const { cookie: partnerCookie, userId: partnerUserId } =
      await signupAndGetCookie('partner@example.com');
    const device = await createDeviceForUser('owner@example.com');

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

    const form = batchMetadataForm({
      start_time: 1710000000000,
      end_time: 1710003600000,
      access_keys: {
        [ownerUserId]: Buffer.from('owner-envelope').toString('base64'),
        [partnerUserId]: Buffer.from('partner-envelope').toString('base64'),
      },
    });
    const batchUploadRes = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.refresh_token}` },
      body: form,
    });
    expect(batchUploadRes.status).toBe(200);

    const ownerDataRes = await SELF.fetch(`${BASE}/data?since=0`, {
      headers: authHeaders(ownerCookie),
    });
    expect(ownerDataRes.status).toBe(200);
    const ownerData = (await ownerDataRes.json()) as {
      batches: Array<{ encrypted_key: string }>;
      watchers: Array<{ user: { email: string } }>;
    };
    expect(ownerData.batches[0]?.encrypted_key).toBe(
      Buffer.from('owner-envelope').toString('base64'),
    );
    expect(ownerData.watchers[0]?.user.email).toBe('partner@example.com');

    const partnerDataRes = await SELF.fetch(`${BASE}/data?since=0`, {
      headers: authHeaders(partnerCookie),
    });
    expect(partnerDataRes.status).toBe(200);
    const partnerData = (await partnerDataRes.json()) as {
      batches: Array<{ encrypted_key: string }>;
      watching: Array<{ user: { email: string } }>;
    };
    expect(partnerData.batches[0]?.encrypted_key).toBe(
      Buffer.from('partner-envelope').toString('base64'),
    );
    expect(partnerData.watching[0]?.user.email).toBe('owner@example.com');
  });

  it('still serves a batch whose access keys were stored as JSON before they became msgpack', async () => {
    const { cookie, userId } = await signupAndGetCookie('legacy@example.com');
    const device = await createDeviceForUser('legacy@example.com', 'password123', 'Old', 'linux');
    const batchId = '00000000-0000-4000-8000-0000000000aa';

    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, start_time, end_time, end_hash, access_keys, version, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes(batchId),
        uuidToBytes(userId),
        uuidToBytes(device.id),
        1_000,
        2_000,
        new Uint8Array(32).fill(9),
        JSON.stringify({ [userId]: Buffer.from('legacy-envelope').toString('base64') }),
        'v0.1',
        3_000,
      )
      .run();

    const res = await SELF.fetch(`${BASE}/data?since=0`, { headers: authHeaders(cookie) });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { batches: unknown[] };
    expect(body.batches).toEqual([
      expect.objectContaining({
        id: batchId,
        url: `${env.R2_URL}/user/${userId}/batches/${batchId}.enc`,
        end_hash: '09'.repeat(32),
        encrypted_key: Buffer.from('legacy-envelope').toString('base64'),
      }),
    ]);
  });

  it('rejects a batch upload whose metadata is missing the required event_counts object', async () => {
    const { userId } = await signupAndGetCookie('bad-metadata@example.com');
    const device = await createDeviceForUser('bad-metadata@example.com', 'password123');

    const form = new FormData();
    form.set(
      'metadata',
      JSON.stringify({
        start_time: 1710000000000,
        end_time: 1710003600000,
        access_keys: { [userId]: Buffer.from('owner-envelope').toString('base64') },
      }),
    );
    form.set('file', new File([new Uint8Array([1, 2, 3])], 'batch.enc'));

    const res = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.refresh_token}` },
      body: form,
    });
    expect(res.status).toBe(400);
  });

  it('rejects a batch upload whose metadata field is not valid JSON', async () => {
    await signupAndGetCookie('bad-json@example.com');
    const device = await createDeviceForUser('bad-json@example.com', 'password123');

    const form = new FormData();
    form.set('metadata', 'not json');
    form.set('file', new File([new Uint8Array([1, 2, 3])], 'batch.enc'));

    const res = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.refresh_token}` },
      body: form,
    });
    expect(res.status).toBe(400);
  });
});
