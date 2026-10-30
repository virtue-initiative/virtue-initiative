import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { env, fetchMock, SELF } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  batchMetadataForm,
  clearDB,
  createDeviceForUser,
  signupAndGetCookie,
  uploadBatch,
} from './helpers';
import { getHashState, installHashServerMock, seedHashState } from './hash-server-mock';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

beforeEach(clearDB);

const b64 = (text: string) => Buffer.from(text).toString('base64');
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

async function setup(email = 'uploader@example.com') {
  const { cookie, userId } = await signupAndGetCookie(email);
  const device = await createDeviceForUser(email, 'password123', 'Laptop', 'linux');
  seedHashState(device.id, { hash: '5a'.repeat(32), seq: 3, last_received: 1_700_000_000 });
  return { cookie, userId, device };
}

type Metadata = Record<string, unknown>;

function validMetadata(userId: string): Metadata {
  return {
    start_time: 1_000,
    end_time: 2_000,
    access_keys: { [userId]: b64('owner-envelope') },
    event_counts: { total: 4, high: 1, medium: 2, screenshot: 1 },
  };
}

/** A multipart upload with `metadata` sent exactly as given, bypassing batchMetadataForm. */
function rawForm(
  metadata: unknown,
  file: FormDataEntryValue | null = new File([new Uint8Array([1, 2, 3])], 'batch.enc'),
) {
  const form = new FormData();
  if (metadata !== undefined) form.set('metadata', JSON.stringify(metadata));
  if (file !== null) form.set('file', file);
  return form;
}

async function storedBatchCount() {
  const row = await env.DB.prepare('SELECT COUNT(*) AS n FROM batches').first<{ n: number }>();
  return row!.n;
}

async function storedObjectCount() {
  return (await env.BUCKET.list({ prefix: 'user/' })).objects.length;
}

async function expectNothingStored() {
  expect(await storedBatchCount()).toBe(0);
  expect(await storedObjectCount()).toBe(0);
}

describe('POST /d/batch metadata validation (API-038)', () => {
  const without = (key: string) => (m: Metadata) => {
    const { [key]: _removed, ...rest } = m;
    return rest;
  };
  const withField = (key: string, value: unknown) => (m: Metadata) => ({ ...m, [key]: value });
  const withCount = (field: string, value: unknown) => (m: Metadata) => ({
    ...m,
    event_counts: { ...(m.event_counts as Metadata), [field]: value },
  });
  const withoutCount = (field: string) => (m: Metadata) => {
    const { [field]: _removed, ...counts } = m.event_counts as Metadata;
    return { ...m, event_counts: counts };
  };

  const timeCases = ['start_time', 'end_time'].flatMap((field) => [
    [`${field} missing`, without(field)],
    [`${field} is a string`, withField(field, '1000')],
    [`${field} is null`, withField(field, null)],
    [`${field} is not an integer`, withField(field, 1000.5)],
    [`${field} is negative`, withField(field, -1)],
  ]);

  const countCases = ['total', 'high', 'medium', 'screenshot'].flatMap((field) => [
    [`event_counts.${field} missing`, withoutCount(field)],
    [`event_counts.${field} is negative`, withCount(field, -1)],
    [`event_counts.${field} is not an integer`, withCount(field, 1.5)],
    [`event_counts.${field} is a string`, withCount(field, '3')],
    [`event_counts.${field} is null`, withCount(field, null)],
  ]);

  const cases: Array<[string, (m: Metadata) => unknown]> = [
    ...(timeCases as Array<[string, (m: Metadata) => unknown]>),
    ['access_keys missing', without('access_keys')],
    ['access_keys is an array', withField('access_keys', [])],
    ['access_keys is a string', withField('access_keys', 'owner')],
    ['access_keys has a key that is not a UUID', withField('access_keys', { owner: b64('k') })],
    [
      'access_keys has a value that is not Base64',
      (m) => ({ ...m, access_keys: { '00000000-0000-4000-8000-000000000001': '***' } }),
    ],
    [
      'access_keys has a value that is not a string',
      (m) => ({ ...m, access_keys: { '00000000-0000-4000-8000-000000000001': 5 } }),
    ],
    ['event_counts missing', without('event_counts')],
    ['event_counts is a number', withField('event_counts', 3)],
    ...(countCases as Array<[string, (m: Metadata) => unknown]>),
    ['notifications is not an array', withField('notifications', 'tamper')],
    ['a notification has no risk', withField('notifications', [{ ts: 1, type: 'system_event' }])],
    [
      'a notification risk is a string',
      withField('notifications', [{ ts: 1, type: 'system_event', risk: 'high' }]),
    ],
    ['a notification has no type', withField('notifications', [{ ts: 1, risk: 0.5 }])],
    ['a notification has no ts', withField('notifications', [{ type: 'system_event', risk: 0.5 }])],
    ['metadata is an array', () => []],
    ['metadata is null', () => null],
  ];

  it.each(cases)('answers 400 and stores nothing when %s (API-038)', async (_name, mutate) => {
    const { userId, device } = await setup();

    const res = await uploadBatch(device.refresh_token, rawForm(mutate(validMetadata(userId))));

    expect(res.status).toBe(400);
    expect(await res.json()).toMatchObject({ error: 'Invalid request data' });
    await expectNothingStored();
    // The hash state is only reset once a batch is stored.
    expect(getHashState(device.id).seq).toBe(3);
  });

  it('accepts the smallest valid metadata: zero counts, no keys, no notifications (API-038)', async () => {
    const { device } = await setup();

    const res = await uploadBatch(
      device.refresh_token,
      rawForm({
        start_time: 0,
        end_time: 0,
        access_keys: {},
        event_counts: { total: 0, high: 0, medium: 0, screenshot: 0 },
      }),
    );

    expect(res.status).toBe(200);
  });
});

describe('POST /d/batch form fields (API-038)', () => {
  it('answers 400 and stores nothing when the metadata field is missing (API-038)', async () => {
    const { device } = await setup();

    const res = await uploadBatch(device.refresh_token, rawForm(undefined));

    expect(res.status).toBe(400);
    await expectNothingStored();
  });

  it('answers 400 and stores nothing when the file is missing (API-038)', async () => {
    const { userId, device } = await setup();

    const res = await uploadBatch(device.refresh_token, rawForm(validMetadata(userId), null));

    expect(res.status).toBe(400);
    await expectNothingStored();
    expect(getHashState(device.id).seq).toBe(3);
  });

  it('answers 400 and stores nothing when the file is a text field, not an upload (API-038)', async () => {
    const { userId, device } = await setup();

    const res = await uploadBatch(device.refresh_token, rawForm(validMetadata(userId), 'bytes'));

    expect(res.status).toBe(400);
    await expectNothingStored();
  });

  // The spec only says the server stores the file. It does not mention an empty one, so
  // this records the current behaviour: an empty file is refused.
  it('answers 400 and stores nothing when the file is empty (API-038)', async () => {
    const { userId, device } = await setup();

    const res = await uploadBatch(
      device.refresh_token,
      rawForm(validMetadata(userId), new File([], 'empty.enc')),
    );

    expect(res.status).toBe(400);
    await expectNothingStored();
  });

  it('answers 400 and stores nothing when the body is JSON, not multipart (API-038)', async () => {
    const { userId, device } = await setup();

    const res = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${device.refresh_token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(validMetadata(userId)),
    });

    expect(res.status).toBe(400);
    await expectNothingStored();
  });
});

describe('POST /d/batch authentication (API-038)', () => {
  it('answers 401 and stores nothing without an Authorization header (API-038)', async () => {
    const { userId } = await setup();

    const res = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      body: rawForm(validMetadata(userId)),
    });

    expect(res.status).toBe(401);
    await expectNothingStored();
  });

  it.each([
    ['an unknown token', (_s: Awaited<ReturnType<typeof setup>>) => 'Bearer not-a-real-token'],
    [
      'the hash-server device JWT, which is a different token type',
      (s: Awaited<ReturnType<typeof setup>>) => `Bearer ${s.device.token}`,
    ],
    ['a web session token', (s: Awaited<ReturnType<typeof setup>>) => `Bearer ${s.cookie}`],
    [
      'a device token under the wrong scheme',
      (s: Awaited<ReturnType<typeof setup>>) => `Token ${s.device.refresh_token}`,
    ],
  ])('answers 401 and stores nothing for %s (API-038)', async (_name, authorization) => {
    const world = await setup();

    const res = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: { Authorization: authorization(world) },
      body: rawForm(validMetadata(world.userId)),
    });

    expect(res.status).toBe(401);
    await expectNothingStored();
  });

  it('answers 401 and stores nothing for a device that has logged out (API-038)', async () => {
    const { userId, device } = await setup();
    const logout = await SELF.fetch(`${BASE}/d/logout`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.refresh_token}` },
    });
    expect(logout.status).toBe(204);

    const res = await uploadBatch(device.refresh_token, rawForm(validMetadata(userId)));

    expect(res.status).toBe(401);
    await expectNothingStored();
  });

  it('keeps one device from uploading as another: the batch belongs to the token owner (API-038)', async () => {
    const first = await setup('first@example.com');
    const second = await setup('second@example.com');

    const res = await uploadBatch(
      first.device.refresh_token,
      rawForm({ ...validMetadata(second.userId), access_keys: { [second.userId]: b64('k') } }),
    );

    expect(res.status).toBe(200);
    const mine = (await (
      await SELF.fetch(`${BASE}/data?since=0`, { headers: authHeaders(first.cookie) })
    ).json()) as { batches: unknown[] };
    const theirs = (await (
      await SELF.fetch(`${BASE}/data?since=0`, { headers: authHeaders(second.cookie) })
    ).json()) as { batches: unknown[] };
    // It was stored for the first user, and the second user's key does not grant access to it.
    expect(mine.batches).toHaveLength(0);
    expect(theirs.batches).toHaveLength(0);
    expect(await storedBatchCount()).toBe(1);
  });
});

describe('POST /d/batch time clamping (API-038)', () => {
  const HOUR = 60 * 60 * 1000;

  // [name, start offset from now, end offset from now, expected start, expected end].
  // "created" means the time the server received the batch.
  const clampCases: Array<[string, number, number, number | 'created', number | 'created']> = [
    ['end in the future, start in the past', -2 * HOUR, 2 * HOUR, -2 * HOUR, 'created'],
    ['start and end both in the future', 3 * HOUR, 2 * HOUR, 'created', 'created'],
    ['start after end, both in the past', -HOUR, -3 * HOUR, -3 * HOUR, -3 * HOUR],
    ['start equal to end, in the past', -HOUR, -HOUR, -HOUR, -HOUR],
    ['start before end, both in the past', -3 * HOUR, -HOUR, -3 * HOUR, -HOUR],
  ];

  it.each(clampCases)(
    'stores times for %s as the spec clamps them (API-038)',
    async (_name, startOffset, endOffset, expectedStart, expectedEnd) => {
      const { cookie, userId, device } = await setup();
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
      const now = Date.now();

      const res = await uploadBatch(
        device.refresh_token,
        batchMetadataForm({
          start_time: now + startOffset,
          end_time: now + endOffset,
          access_keys: { [userId]: b64('k') },
        }),
      );
      warn.mockRestore();

      expect(res.status).toBe(200);
      const body = (await res.json()) as { start_time: number; end_time: number };
      const data = (await (
        await SELF.fetch(`${BASE}/data?since=0`, { headers: authHeaders(cookie) })
      ).json()) as { batches: Array<{ start_time: number; end_time: number; created_at: number }> };
      const stored = data.batches[0]!;
      const resolve = (value: number | 'created') =>
        value === 'created' ? stored.created_at : now + value;
      expect(stored.start_time).toBe(resolve(expectedStart));
      expect(stored.end_time).toBe(resolve(expectedEnd));
      // The response reports what was stored, not what the device sent.
      expect(body).toMatchObject({ start_time: stored.start_time, end_time: stored.end_time });
      expect(stored.end_time).toBeLessThanOrEqual(stored.created_at);
      expect(stored.start_time).toBeLessThanOrEqual(stored.end_time);
    },
  );

  it('logs a warning when it clamps an end_time in the future (API-038)', async () => {
    const { userId, device } = await setup();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await uploadBatch(
      device.refresh_token,
      batchMetadataForm({
        start_time: Date.now() - HOUR,
        end_time: Date.now() + HOUR,
        access_keys: { [userId]: b64('k') },
      }),
    );

    expect(warn).toHaveBeenCalledWith(expect.stringContaining('clamped'));
    warn.mockRestore();
  });

  // KNOWN-BUG: the warning is only logged when end_time is clamped. A start_time that is clamped to an end_time in the past is silent.
  // The spec wants a warning here. This pins today's silence, so it fails once the bug is fixed.
  it('logs no warning yet when it clamps a start_time later than end_time (API-038)', async () => {
    const { userId, device } = await setup();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const now = Date.now();

    const res = await uploadBatch(
      device.refresh_token,
      batchMetadataForm({
        start_time: now - HOUR,
        end_time: now - 3 * HOUR,
        access_keys: { [userId]: b64('k') },
      }),
    );

    expect(res.status).toBe(200);
    expect(warn).not.toHaveBeenCalled();
    warn.mockRestore();
  });

  it('does not log a warning when the times need no clamping (API-038)', async () => {
    const { userId, device } = await setup();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await uploadBatch(
      device.refresh_token,
      batchMetadataForm({
        start_time: 1_000,
        end_time: 2_000,
        access_keys: { [userId]: b64('k') },
      }),
    );

    expect(warn).not.toHaveBeenCalled();
    warn.mockRestore();
  });
});

describe('POST /d/batch access_keys recipients (API-038)', () => {
  // The spec describes access_keys as a map from user id to HPKE key and does not say
  // the ids must belong to the owner or an accepted partner. These rows record the
  // current behaviour: the upload is accepted, and a key never grants access by itself.
  it('accepts keys for a user who is not the owner or a partner, and that user still cannot see the batch (API-038, API-033)', async () => {
    const { cookie, userId, device } = await setup();
    const stranger = await signupAndGetCookie('stranger@example.com');

    const res = await uploadBatch(
      device.refresh_token,
      batchMetadataForm({
        start_time: 1_000,
        end_time: 2_000,
        access_keys: { [userId]: b64('owner-key'), [stranger.userId]: b64('stranger-key') },
      }),
    );

    expect(res.status).toBe(200);
    const strangerData = (await (
      await SELF.fetch(`${BASE}/data?since=0`, { headers: authHeaders(stranger.cookie) })
    ).json()) as { batches: unknown[] };
    expect(strangerData.batches).toEqual([]);
    const ownerData = (await (
      await SELF.fetch(`${BASE}/data?since=0`, { headers: authHeaders(cookie) })
    ).json()) as { batches: Array<{ encrypted_key: string }> };
    expect(ownerData.batches.map((batch) => batch.encrypted_key)).toEqual([b64('owner-key')]);
  });

  it('accepts a key for a user id that does not exist (API-038)', async () => {
    const { cookie, userId, device } = await setup();

    const res = await uploadBatch(
      device.refresh_token,
      batchMetadataForm({
        start_time: 1_000,
        end_time: 2_000,
        access_keys: {
          [userId]: b64('owner-key'),
          '00000000-0000-4000-8000-00000000dead': b64('ghost-key'),
        },
      }),
    );

    expect(res.status).toBe(200);
    const data = (await (
      await SELF.fetch(`${BASE}/data?since=0`, { headers: authHeaders(cookie) })
    ).json()) as { batches: unknown[] };
    expect(data.batches).toHaveLength(1);
  });

  it.each([
    ['no keys at all', {}],
    ['only a key for someone else', { '00000000-0000-4000-8000-00000000dead': b64('ghost-key') }],
  ])(
    'accepts %s, and the batch is then left out of the owner GET /data because they cannot decrypt it (API-038, API-033)',
    async (_name, accessKeys) => {
      const { cookie, device } = await setup();

      const res = await uploadBatch(
        device.refresh_token,
        batchMetadataForm({ start_time: 1_000, end_time: 2_000, access_keys: accessKeys }),
      );

      expect(res.status).toBe(200);
      expect(await storedBatchCount()).toBe(1);
      const data = (await (
        await SELF.fetch(`${BASE}/data?since=0`, { headers: authHeaders(cookie) })
      ).json()) as { batches: unknown[] };
      expect(data.batches).toEqual([]);
    },
  );
});

describe('POST /d/batch response (API-038)', () => {
  it('returns the stored batch with the refreshed device settings (API-038)', async () => {
    const { cookie, userId, device } = await setup();
    const file = new Uint8Array([4, 5, 6, 7, 8]);

    const res = await uploadBatch(
      device.refresh_token,
      batchMetadataForm({
        start_time: 1_000,
        end_time: 2_000,
        access_keys: { [userId]: b64('owner-key') },
        file: new File([file], 'batch.enc'),
      }),
    );

    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      id: string;
      start_time: number;
      end_time: number;
      end_hash: string;
      url: string;
      settings: {
        id: string;
        name: string;
        platform: string;
        wrapping_keys: Array<{ user_id: string; pub_key: string }>;
        hash_base_url: string;
        hash_token: string;
        feature_flags: string[];
      };
    };
    expect(Object.keys(body).sort()).toEqual(
      ['end_hash', 'end_time', 'id', 'settings', 'start_time', 'url'].sort(),
    );
    expect(body.id).toMatch(UUID);
    expect(body).toMatchObject({
      start_time: 1_000,
      end_time: 2_000,
      end_hash: '5a'.repeat(32),
      url: `${env.R2_URL}/user/${userId}/batches/${body.id}.enc`,
    });
    expect(body.settings).toMatchObject({
      id: device.id,
      name: 'Laptop',
      platform: 'linux',
      hash_base_url: 'https://example.com',
      feature_flags: expect.any(Array),
    });
    expect(body.settings.wrapping_keys.map((key) => key.user_id)).toEqual([userId]);
    expect(body.settings.wrapping_keys[0]!.pub_key).toEqual(expect.any(String));
    const claims = JSON.parse(
      atob(body.settings.hash_token.split('.')[1]!.replace(/-/g, '+').replace(/_/g, '/')),
    ) as {
      sub: string;
      type: string;
    };
    expect(claims).toMatchObject({ sub: device.id, type: 'device' });

    // The file is where the url says, and the owner's GET /data lists the same batch.
    const object = await SELF.fetch(body.url);
    expect(new Uint8Array(await object.arrayBuffer())).toEqual(file);
    const data = (await (
      await SELF.fetch(`${BASE}/data?since=0`, { headers: authHeaders(cookie) })
    ).json()) as { batches: Array<{ id: string; url: string; end_hash: string }> };
    expect(data.batches).toEqual([
      expect.objectContaining({ id: body.id, url: body.url, end_hash: body.end_hash }),
    ]);
  });

  it('resets the device hash state after storing, so the next batch gets a new end_hash (API-038)', async () => {
    const { userId, device } = await setup();
    const upload = () =>
      uploadBatch(
        device.refresh_token,
        batchMetadataForm({
          start_time: 1_000,
          end_time: 2_000,
          access_keys: { [userId]: b64('k') },
        }),
      );

    const first = (await (await upload()).json()) as { id: string; end_hash: string; url: string };
    expect(getHashState(device.id)).toMatchObject({ hash: '0'.repeat(64), seq: 0 });
    const second = (await (await upload()).json()) as { id: string; end_hash: string; url: string };

    expect(first.end_hash).toBe('5a'.repeat(32));
    expect(second.end_hash).toBe('0'.repeat(64));
    expect(second.id).not.toBe(first.id);
    expect(second.url).not.toBe(first.url);
  });
});
