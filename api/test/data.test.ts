import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
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
import { CURRENT_API_VERSION } from '../src/lib/api-version';
import { createBatch } from '../src/lib/db';
import { DATA_BATCH_PAGE_LIMIT } from '../src/routes/data';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

beforeEach(clearDB);

describe('Data and device API routes', () => {
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

    const storedBatch = await env.DB.prepare(
      'SELECT access_keys, version FROM batches WHERE id = ?',
    )
      .bind(uuidToBytes(batch.id))
      .first<{ access_keys: string; version: string }>();
    expect(JSON.parse(storedBatch!.access_keys)).toEqual({
      [userId]: Buffer.from('owner-envelope').toString('base64'),
    });
    expect(storedBatch!.version).toBe(CURRENT_API_VERSION);

    const dataRes = await SELF.fetch(`${BASE}/data?since=0`, {
      headers: authHeaders(userCookie),
    });
    expect(dataRes.status).toBe(200);
    const data = (await dataRes.json()) as {
      batches: Array<{
        device_id: string;
        end_hash: string;
        version: string;
        encrypted_key: string;
        created_at: number;
      }>;
      batches_complete: boolean;
      user: { id: string; email: string };
      watching: unknown[];
      watchers: unknown[];
    };
    expect(data.batches[0]).toMatchObject({
      device_id: device.id,
      version: CURRENT_API_VERSION,
      encrypted_key: Buffer.from('owner-envelope').toString('base64'),
    });
    expect(data.batches[0]?.created_at).toEqual(expect.any(Number));
    expect(data.batches_complete).toBe(true);
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

  it('?since=0 returns every batch as complete when the backlog exactly fills the page limit', async () => {
    const { cookie: userCookie, userId } = await signupAndGetCookie('since-zero@example.com');
    const device = await createDeviceForUser('since-zero@example.com', 'password123');

    const baseTime = 1710000000000;
    for (let i = 0; i < DATA_BATCH_PAGE_LIMIT; i += 1) {
      const id = `00000000-0000-4000-9100-${i.toString(16).padStart(12, '0')}`;
      await createBatch(env.DB, {
        id,
        user_id: userId,
        device_id: device.id,
        url: `https://example.com/batch-${i}.enc`,
        start_time: baseTime + i,
        end_time: baseTime + i,
        end_hash: `hash-${i}`,
        access_keys: JSON.stringify({ [userId]: `key-${i}` }),
        version: CURRENT_API_VERSION,
        created_at: baseTime + i,
      });
    }

    const res = await SELF.fetch(`${BASE}/data?since=0`, { headers: authHeaders(userCookie) });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      batches: Array<{ encrypted_key: string }>;
      batches_complete: boolean;
    };
    expect(body.batches).toHaveLength(DATA_BATCH_PAGE_LIMIT);
    expect(body.batches_complete).toBe(true);

    // Omitting `since` entirely defaults to 0 and MUST behave identically.
    const defaultedRes = await SELF.fetch(`${BASE}/data`, { headers: authHeaders(userCookie) });
    expect(defaultedRes.status).toBe(200);
    const defaultedBody = (await defaultedRes.json()) as {
      batches: Array<{ encrypted_key: string }>;
      batches_complete: boolean;
    };
    expect(defaultedBody.batches).toEqual(body.batches);
    expect(defaultedBody.batches_complete).toBe(true);
  });

  it('pages a backlog larger than the per-request batch limit, oldest first', async () => {
    const { cookie: userCookie, userId } = await signupAndGetCookie('pagination@example.com');
    const device = await createDeviceForUser('pagination@example.com', 'password123');

    const backlogSize = DATA_BATCH_PAGE_LIMIT + 20;
    const baseTime = 1710000000000;
    for (let i = 0; i < backlogSize; i += 1) {
      const id = `00000000-0000-4000-9000-${i.toString(16).padStart(12, '0')}`;
      await createBatch(env.DB, {
        id,
        user_id: userId,
        device_id: device.id,
        url: `https://example.com/batch-${i}.enc`,
        start_time: baseTime + i,
        end_time: baseTime + i,
        end_hash: `hash-${i}`,
        access_keys: JSON.stringify({ [userId]: `key-${i}` }),
        version: CURRENT_API_VERSION,
        created_at: baseTime + i,
      });
    }

    const firstPageRes = await SELF.fetch(`${BASE}/data?since=0`, {
      headers: authHeaders(userCookie),
    });
    expect(firstPageRes.status).toBe(200);
    const firstPage = (await firstPageRes.json()) as {
      batches: Array<{ created_at: number; encrypted_key: string }>;
      batches_complete: boolean;
    };
    expect(firstPage.batches).toHaveLength(DATA_BATCH_PAGE_LIMIT);
    expect(firstPage.batches_complete).toBe(false);
    expect(firstPage.batches.map((b) => b.encrypted_key)).toEqual(
      Array.from({ length: DATA_BATCH_PAGE_LIMIT }, (_, i) => `key-${i}`),
    );
    const createdAts = firstPage.batches.map((b) => b.created_at);
    expect(createdAts).toEqual([...createdAts].sort((a, b) => a - b));

    const cursor = firstPage.batches[firstPage.batches.length - 1]!.created_at;
    const secondPageRes = await SELF.fetch(`${BASE}/data?since=${cursor}`, {
      headers: authHeaders(userCookie),
    });
    expect(secondPageRes.status).toBe(200);
    const secondPage = (await secondPageRes.json()) as {
      batches: Array<{ created_at: number; encrypted_key: string }>;
      batches_complete: boolean;
    };
    expect(secondPage.batches).toHaveLength(backlogSize - DATA_BATCH_PAGE_LIMIT);
    expect(secondPage.batches_complete).toBe(true);
    expect(secondPage.batches.map((b) => b.encrypted_key)).toEqual(
      Array.from(
        { length: backlogSize - DATA_BATCH_PAGE_LIMIT },
        (_, i) => `key-${DATA_BATCH_PAGE_LIMIT + i}`,
      ),
    );
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
