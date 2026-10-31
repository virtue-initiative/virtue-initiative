import { beforeAll, beforeEach, describe, expect, it, onTestFinished, vi } from 'vitest';
import { createExecutionContext, env, fetchMock, SELF } from 'cloudflare:test';
import worker from '../src/index';
import {
  acceptPartnerInvite,
  authHeaders,
  BASE,
  batchMetadataForm,
  clearDB,
  createDeviceForUser,
  invitePartner,
  listEmailDeliveries,
  markUserEmailVerified,
  signupAndGetCookie,
  uploadBatch,
} from './helpers';
import { installHashServerMock } from './hash-server-mock';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
  installHashServerMock();
});

beforeEach(clearDB);

type Actor = 'owner' | 'accepted' | 'pending' | 'removed' | 'unrelated' | 'anonymous';

const ACTORS: Actor[] = ['owner', 'accepted', 'pending', 'removed', 'unrelated', 'anonymous'];
const NON_OWNERS: Actor[] = ['accepted', 'pending', 'removed', 'unrelated'];

const b64 = (text: string) => Buffer.from(text).toString('base64');
const objectKey = (batch: { url: string }) => batch.url.replace(`${env.R2_URL}/`, '');
const BATCH_BYTES = new Uint8Array([9, 8, 7, 6]);

/**
 * One owner with a device and two batches, and four other users in each relationship to
 * that owner the API distinguishes:
 *
 * - accepted: invited and accepted
 * - pending: invited, never accepted (has an account under the invited email)
 * - removed: accepted, then the owner deleted the partnership
 * - unrelated: never invited
 *
 * Every one of them has a key in the second batch's `access_keys`, so a test passing
 * proves access is decided by the partnership, not by the key list.
 */
async function buildWorld() {
  const owner = await signupAndGetCookie('owner@ac.test', 'password123', 'Owner');
  const accepted = await signupAndGetCookie('accepted@ac.test', 'password123', 'Accepted');
  const pending = await signupAndGetCookie('pending@ac.test', 'password123', 'Pending');
  const removed = await signupAndGetCookie('removed@ac.test', 'password123', 'Removed');
  const unrelated = await signupAndGetCookie('unrelated@ac.test', 'password123', 'Unrelated');
  for (const user of [owner, accepted, pending, removed, unrelated]) {
    await markUserEmailVerified(user.userId);
  }

  const device = await createDeviceForUser('owner@ac.test', 'password123', 'Owner Laptop', 'linux');

  const acceptedInvite = await invitePartner(owner.cookie, 'accepted@ac.test');
  expect((await acceptPartnerInvite(accepted.cookie, acceptedInvite.token)).status).toBe(200);
  const pendingInvite = await invitePartner(owner.cookie, 'pending@ac.test');
  const removedInvite = await invitePartner(owner.cookie, 'removed@ac.test');
  expect((await acceptPartnerInvite(removed.cookie, removedInvite.token)).status).toBe(200);

  // The first batch is uploaded while `removed` still watches the owner.
  const first = await uploadBatch(
    device.refresh_token,
    batchMetadataForm({
      start_time: 1_000,
      end_time: 2_000,
      access_keys: {
        [owner.userId]: b64('first-owner'),
        [accepted.userId]: b64('first-accepted'),
        [removed.userId]: b64('first-removed'),
      },
      file: new File([BATCH_BYTES], 'first.enc'),
    }),
  );
  expect(first.status).toBe(200);
  const firstBatch = (await first.json()) as { id: string; url: string };

  expect(
    (
      await SELF.fetch(`${BASE}/partner/${removedInvite.id}`, {
        method: 'DELETE',
        headers: authHeaders(owner.cookie),
      })
    ).status,
  ).toBe(204);

  const second = await uploadBatch(
    device.refresh_token,
    batchMetadataForm({
      start_time: 2_000,
      end_time: 3_000,
      access_keys: {
        [owner.userId]: b64('second-owner'),
        [accepted.userId]: b64('second-accepted'),
        [pending.userId]: b64('second-pending'),
        [removed.userId]: b64('second-removed'),
        [unrelated.userId]: b64('second-unrelated'),
      },
      file: new File([BATCH_BYTES], 'second.enc'),
    }),
  );
  expect(second.status).toBe(200);
  const secondBatch = (await second.json()) as { id: string; url: string };

  const lockedRes = await SELF.fetch(`${BASE}/locked-password`, {
    method: 'POST',
    headers: authHeaders(owner.cookie),
    body: JSON.stringify({ label: 'Screen Time passcode', wrapped_value: 'c2VjcmV0' }),
  });
  expect(lockedRes.status).toBe(200);
  const locked = (await lockedRes.json()) as { id: string };

  const users: Record<Exclude<Actor, 'anonymous'>, { cookie: string; userId: string }> = {
    owner,
    accepted,
    pending,
    removed,
    unrelated,
  };

  return {
    users,
    device,
    acceptedPartnershipId: acceptedInvite.id,
    pendingPartnershipId: pendingInvite.id,
    pendingInviteToken: pendingInvite.token,
    removedPartnershipId: removedInvite.id,
    batches: [firstBatch, secondBatch],
    lockedId: locked.id,
  };
}

type World = Awaited<ReturnType<typeof buildWorld>>;

/** Cookie headers for an actor, or none for the anonymous actor. */
function headersFor(world: World, actor: Actor): Record<string, string> {
  return actor === 'anonymous'
    ? { 'Content-Type': 'application/json' }
    : authHeaders(world.users[actor].cookie);
}

function call(world: World, actor: Actor, method: string, path: string, body?: unknown) {
  return SELF.fetch(`${BASE}${path}`, {
    method,
    headers: headersFor(world, actor),
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
}

type Observation = Record<Actor, unknown>;

/**
 * Runs `observe` as every actor against one world and compares the whole table at once,
 * so a failure shows every actor's result side by side. `last` actors run after the
 * others, for steps that change the world.
 */
async function observeAll(
  world: World,
  observe: (actor: Actor) => Promise<unknown>,
  last: Actor[] = [],
): Promise<Observation> {
  const order = [...ACTORS.filter((a) => !last.includes(a)), ...last];
  const observed: Partial<Observation> = {};
  for (const actor of order) {
    observed[actor] = await observe(actor);
  }
  return observed as Observation;
}

type Json = Record<string, any>;

async function json(res: Response): Promise<Json> {
  return (await res.json()) as Json;
}

const decodeKey = (value: string) => Buffer.from(value, 'base64').toString();
const partnerLabel = (p: { user: { email: string }; status: string }) =>
  `${p.user.email}:${p.status}`;

describe('access matrix: actor x endpoint', () => {
  it('GET /data returns the owner batches only to the owner and the accepted partner, each with their own key (API-033)', async () => {
    const world = await buildWorld();

    const observed = await observeAll(world, async (actor) => {
      const res = await call(world, actor, 'GET', '/data?since=0');
      if (res.status !== 200) return { status: res.status };
      const body = await json(res);
      return {
        status: 200,
        keys: body.batches.map((batch: Json) => decodeKey(batch.encrypted_key)),
        ownedBy: [...new Set(body.batches.map((batch: Json) => batch.user_id))],
      };
    });

    const ownerOnly = [world.users.owner.userId];
    expect(observed).toEqual({
      owner: { status: 200, keys: ['first-owner', 'second-owner'], ownedBy: ownerOnly },
      accepted: { status: 200, keys: ['first-accepted', 'second-accepted'], ownedBy: ownerOnly },
      pending: { status: 200, keys: [], ownedBy: [] },
      removed: { status: 200, keys: [], ownedBy: [] },
      unrelated: { status: 200, keys: [], ownedBy: [] },
      anonymous: { status: 401 },
    });
  });

  it('GET /data names only the caller in user, and each actor sees only the partnerships they are a party to (API-033, API-026)', async () => {
    const world = await buildWorld();

    const observed = await observeAll(world, async (actor) => {
      const res = await call(world, actor, 'GET', '/data?since=0');
      if (res.status !== 200) return { status: res.status };
      const body = await json(res);
      return {
        user: body.user.email,
        watching: body.watching.map(partnerLabel).sort(),
        watchers: body.watchers.map(partnerLabel).sort(),
        leaksOwnerPrivateKey: JSON.stringify(body).includes(b64('priv:owner@ac.test')),
      };
    });

    const none = { watching: [], watchers: [] };
    expect(observed).toEqual({
      owner: {
        user: 'owner@ac.test',
        watching: [],
        watchers: ['accepted@ac.test:accepted', 'pending@ac.test:pending'],
        leaksOwnerPrivateKey: true, // the owner's own private key envelope is theirs to read
      },
      accepted: {
        user: 'accepted@ac.test',
        ...none,
        watching: ['owner@ac.test:accepted'],
        leaksOwnerPrivateKey: false,
      },
      pending: { user: 'pending@ac.test', ...none, leaksOwnerPrivateKey: false },
      removed: { user: 'removed@ac.test', ...none, leaksOwnerPrivateKey: false },
      unrelated: { user: 'unrelated@ac.test', ...none, leaksOwnerPrivateKey: false },
      anonymous: { status: 401 },
    });
  });

  it('GET /device lists the owner device to the owner and the accepted partner only (API-029)', async () => {
    const world = await buildWorld();

    const observed = await observeAll(world, async (actor) => {
      const res = await call(world, actor, 'GET', '/device');
      if (res.status !== 200) return { status: res.status };
      const body = (await res.json()) as Json[];
      return { status: 200, devices: body.map((d) => [d.id, d.owner]) };
    });

    const device = [[world.device.id, world.users.owner.userId]];
    expect(observed).toEqual({
      owner: { status: 200, devices: device },
      accepted: { status: 200, devices: device },
      pending: { status: 200, devices: [] },
      removed: { status: 200, devices: [] },
      unrelated: { status: 200, devices: [] },
      anonymous: { status: 401 },
    });
  });

  it('PATCH /device/:id renames the device for the owner only (API-030)', async () => {
    const world = await buildWorld();
    const names = async () =>
      ((await (await call(world, 'owner', 'GET', '/device')).json()) as Json[]).map((d) => d.name);

    const observed = await observeAll(
      world,
      async (actor) => {
        const res = await call(world, actor, 'PATCH', `/device/${world.device.id}`, {
          name: `Renamed by ${actor}`,
        });
        return { status: res.status, nameAfter: (await names())[0] };
      },
      ['owner'],
    );

    expect(observed).toEqual({
      accepted: { status: 404, nameAfter: 'Owner Laptop' },
      pending: { status: 404, nameAfter: 'Owner Laptop' },
      removed: { status: 404, nameAfter: 'Owner Laptop' },
      unrelated: { status: 404, nameAfter: 'Owner Laptop' },
      anonymous: { status: 401, nameAfter: 'Owner Laptop' },
      owner: { status: 204, nameAfter: 'Renamed by owner' },
    });
  });

  it('DELETE /device/:id is refused to everyone but the owner, and removes nothing (API-031)', async () => {
    const world = await buildWorld();

    const observed = await observeAll(world, async (actor) => {
      if (actor === 'owner') return 'skipped';
      return (await call(world, actor, 'DELETE', `/device/${world.device.id}`)).status;
    });

    expect(observed).toEqual({
      owner: 'skipped',
      accepted: 404,
      pending: 404,
      removed: 404,
      unrelated: 404,
      anonymous: 401,
    });
    const devices = (await (await call(world, 'owner', 'GET', '/device')).json()) as Json[];
    expect(devices.map((d) => d.id)).toEqual([world.device.id]);
    const data = await json(await call(world, 'owner', 'GET', '/data?since=0'));
    expect(data.batches).toHaveLength(2);
    expect(await env.BUCKET.head(objectKey(world.batches[0]!))).not.toBeNull();
    expect(
      (await listEmailDeliveries()).filter((delivery) => delivery.kind === 'device_deleted'),
    ).toEqual([]);
  });

  it('DELETE /device/:id as the owner removes the device and batches for every viewer and emails the owner and the accepted partner only (API-031)', async () => {
    const world = await buildWorld();

    const res = await call(world, 'owner', 'DELETE', `/device/${world.device.id}`);

    expect(res.status).toBe(204);
    for (const actor of ['owner', 'accepted'] as const) {
      expect(await (await call(world, actor, 'GET', '/device')).json()).toEqual([]);
      const data = await json(await call(world, actor, 'GET', '/data?since=0'));
      expect(data.batches).toEqual([]);
    }
    expect(await env.BUCKET.head(objectKey(world.batches[0]!))).toBeNull();
    const recipients = (await listEmailDeliveries())
      .filter((delivery) => delivery.kind === 'device_deleted')
      .map((delivery) => delivery.recipient_email)
      .sort();
    expect(recipients).toEqual(['accepted@ac.test', 'owner@ac.test']);
  });

  it('GET /partner shows each actor only the partnerships they are a party to (API-026)', async () => {
    const world = await buildWorld();

    const observed = await observeAll(world, async (actor) => {
      const res = await call(world, actor, 'GET', '/partner');
      if (res.status !== 200) return { status: res.status };
      const body = await json(res);
      return {
        status: 200,
        watching: body.watching.map(partnerLabel).sort(),
        watchers: body.watchers.map(partnerLabel).sort(),
      };
    });

    expect(observed).toEqual({
      owner: {
        status: 200,
        watching: [],
        watchers: ['accepted@ac.test:accepted', 'pending@ac.test:pending'],
      },
      accepted: { status: 200, watching: ['owner@ac.test:accepted'], watchers: [] },
      pending: { status: 200, watching: [], watchers: [] },
      removed: { status: 200, watching: [], watchers: [] },
      unrelated: { status: 200, watching: [], watchers: [] },
      anonymous: { status: 401 },
    });
  });

  it('DELETE /partner/:id is 404 for anyone who is not a party, and 401 without a session, and changes nothing (API-027)', async () => {
    const world = await buildWorld();
    const parties: Record<string, Actor[]> = {
      [world.acceptedPartnershipId]: ['owner', 'accepted'],
      [world.pendingPartnershipId]: ['owner', 'pending'],
      [world.removedPartnershipId]: ['owner', 'removed'], // already deleted, so nobody is a party
    };

    const observed: Record<string, Record<string, number>> = {};
    for (const [name, id] of Object.entries({
      accepted: world.acceptedPartnershipId,
      pending: world.pendingPartnershipId,
      removed: world.removedPartnershipId,
    })) {
      observed[name] = {};
      for (const actor of ACTORS) {
        const isParty = name !== 'removed' && parties[id]!.includes(actor);
        if (isParty) continue; // success paths are covered below, they change the world
        observed[name]![actor] = (await call(world, actor, 'DELETE', `/partner/${id}`)).status;
      }
    }

    expect(observed).toEqual({
      accepted: { pending: 404, removed: 404, unrelated: 404, anonymous: 401 },
      pending: { accepted: 404, removed: 404, unrelated: 404, anonymous: 401 },
      removed: {
        owner: 404,
        accepted: 404,
        pending: 404,
        removed: 404,
        unrelated: 404,
        anonymous: 401,
      },
    });
    const owner = await json(await call(world, 'owner', 'GET', '/partner'));
    expect(owner.watchers.map(partnerLabel).sort()).toEqual([
      'accepted@ac.test:accepted',
      'pending@ac.test:pending',
    ]);
  });

  it.each([
    ['the owner', 'owner', 'accepted'],
    ['the accepted partner', 'accepted', 'accepted'],
    ['the owner', 'owner', 'pending'],
    ['the invited user', 'pending', 'pending'],
  ] as const)(
    'DELETE /partner/:id by %s of the %s partnership answers 204 and ends the watcher access (API-027, API-033)',
    async (_name, actor, which) => {
      const world = await buildWorld();
      const id = which === 'accepted' ? world.acceptedPartnershipId : world.pendingPartnershipId;

      const res = await call(world, actor, 'DELETE', `/partner/${id}`);

      expect(res.status).toBe(204);
      const owner = await json(await call(world, 'owner', 'GET', '/partner'));
      expect(owner.watchers.map((p: Json) => p.id)).not.toContain(id);
      const watcherData = await json(await call(world, 'accepted', 'GET', '/data?since=0'));
      expect(watcherData.batches).toHaveLength(which === 'accepted' ? 0 : 2);
    },
  );

  it('POST /partner/accept answers 409 to the sender and to someone already watching, 401 to no session (API-025)', async () => {
    const world = await buildWorld();

    const observed = await observeAll(world, async (actor) => {
      if (actor === 'pending' || actor === 'removed' || actor === 'unrelated') return 'skipped';
      return (
        await call(world, actor, 'POST', '/partner/accept', { token: world.pendingInviteToken })
      ).status;
    });

    expect(observed).toEqual({
      owner: 409,
      accepted: 409,
      pending: 'skipped',
      removed: 'skipped',
      unrelated: 'skipped',
      anonymous: 401,
    });
  });
});

describe('GET /r2/* access (API-040)', () => {
  // The spec only says the route forwards to R2 and is meant for dev. It names no
  // authentication, so this records that every caller, signed in or not, can read any
  // object whose key they know.
  it('serves the owner batch object to every actor, signed in or not (API-040)', async () => {
    const world = await buildWorld();
    const path = `/r2/${objectKey(world.batches[1]!)}`;

    const observed = await observeAll(world, async (actor) => {
      const res = await SELF.fetch(`${BASE}${path}`, { headers: headersFor(world, actor) });
      return { status: res.status, body: [...new Uint8Array(await res.arrayBuffer())] };
    });

    const served = { status: 200, body: [...BATCH_BYTES] };
    expect(observed).toEqual({
      owner: served,
      accepted: served,
      pending: served,
      removed: served,
      unrelated: served,
      anonymous: served,
    });
  });

  it.each([
    ['an unknown key', '/r2/user/nobody/batches/none.enc'],
    ['no key', '/r2/'],
  ])('answers 404 for %s (API-040)', async (_name, path) => {
    const res = await SELF.fetch(`${BASE}${path}`);

    expect(res.status).toBe(404);
  });

  it('answers 404 for a real object without DEV_R2_ROUTE (API-040)', async () => {
    const key = 'user/someone/batches/real.enc';
    await env.BUCKET.put(key, BATCH_BYTES);
    const request = new Request(`${BASE}/r2/${key}`);

    const res = await worker.fetch(
      request,
      { ...env, DEV_R2_ROUTE: undefined },
      createExecutionContext(),
    );
    // An R2 stream left unread breaks the test pool's isolated storage.
    await res.arrayBuffer();

    expect(res.status).toBe(404);
  });
});

describe('locked password access (API-043 to API-049)', () => {
  it('GET /locked-password lists the entry to its owner only, never with the value (API-045)', async () => {
    const world = await buildWorld();

    const observed = await observeAll(world, async (actor) => {
      const res = await call(world, actor, 'GET', '/locked-password');
      if (res.status !== 200) return { status: res.status };
      const body = (await res.json()) as Json[];
      return {
        status: 200,
        labels: body.map((e) => e.label),
        leaksValue: JSON.stringify(body).includes('c2VjcmV0'),
      };
    });

    const none = { status: 200, labels: [], leaksValue: false };
    expect(observed).toEqual({
      owner: { status: 200, labels: ['Screen Time passcode'], leaksValue: false },
      accepted: none,
      pending: none,
      removed: none,
      unrelated: none,
      anonymous: { status: 401 },
    });
  });

  it('POST /locked-password/:id/reveal is 404 for everyone but the owner, and does not mark the entry accessed or alert anyone (API-046)', async () => {
    const world = await buildWorld();

    const observed = await observeAll(world, async (actor) => {
      if (actor === 'owner') return 'skipped';
      const res = await call(world, actor, 'POST', `/locked-password/${world.lockedId}/reveal`);
      return { status: res.status, body: JSON.stringify(await res.json()).includes('c2VjcmV0') };
    });

    const refused = (status: number) => ({ status, body: false });
    expect(observed).toEqual({
      owner: 'skipped',
      accepted: refused(404),
      pending: refused(404),
      removed: refused(404),
      unrelated: refused(404),
      anonymous: refused(401),
    });
    const list = (await (await call(world, 'owner', 'GET', '/locked-password')).json()) as Json[];
    expect(list[0]!.accessed_at).toBeNull();
    expect(
      (await listEmailDeliveries()).filter((d) => d.kind === 'locked_password_accessed'),
    ).toEqual([]);
  });

  it('POST /locked-password/:id/reveal by the owner alerts the accepted partner and nobody else (API-046)', async () => {
    const world = await buildWorld();

    const res = await call(world, 'owner', 'POST', `/locked-password/${world.lockedId}/reveal`);

    expect(res.status).toBe(200);
    expect(await json(res)).toMatchObject({ wrapped_value: 'c2VjcmV0' });
    const alerts = (await listEmailDeliveries()).filter(
      (d) => d.kind === 'locked_password_accessed',
    );
    expect(alerts.map((d) => d.recipient_email)).toEqual(['accepted@ac.test']);
    expect(alerts[0]!.text).toContain('Screen Time passcode');
    expect(JSON.stringify(alerts[0])).not.toContain('c2VjcmV0');
  });

  it('DELETE, restore and permanent delete of a locked password are 404 for everyone but the owner, and change nothing (API-047, API-048, API-049)', async () => {
    const world = await buildWorld();
    const operations = [
      ['DELETE', ''],
      ['POST', '/restore'],
      ['DELETE', '/permanent'],
    ] as const;

    const observed: Record<string, Record<string, number>> = {};
    for (const actor of ACTORS.filter((a) => a !== 'owner')) {
      observed[actor] = {};
      for (const [method, suffix] of operations) {
        const res = await call(world, actor, method, `/locked-password/${world.lockedId}${suffix}`);
        observed[actor]![`${method}${suffix}`] = res.status;
      }
    }

    const refused = (status: number) => ({
      DELETE: status,
      'POST/restore': status,
      'DELETE/permanent': status,
    });
    expect(observed).toEqual({
      accepted: refused(404),
      pending: refused(404),
      removed: refused(404),
      unrelated: refused(404),
      anonymous: refused(401),
    });
    const list = (await (await call(world, 'owner', 'GET', '/locked-password')).json()) as Json[];
    expect(list).toHaveLength(1);
    expect(list[0]!.deleted_at).toBeNull();
  });
});

describe('who a device upload reaches (API-038)', () => {
  it('lists the owner and accepted partners, and nobody else, as the device wrapping keys (API-038)', async () => {
    const world = await buildWorld();

    const res = await uploadBatch(
      world.device.refresh_token,
      batchMetadataForm({ start_time: 1, end_time: 2, access_keys: {} }),
    );

    const { settings } = (await res.json()) as {
      settings: { wrapping_keys: Array<{ user_id: string }> };
    };
    expect(settings.wrapping_keys.map((key) => key.user_id).sort()).toEqual(
      [world.users.owner.userId, world.users.accepted.userId].sort(),
    );
  });

  it('emails an uploaded tamper alert to accepted watchers only (API-038)', async () => {
    const world = await buildWorld();

    const res = await uploadBatch(
      world.device.refresh_token,
      batchMetadataForm({
        start_time: 1,
        end_time: 2,
        access_keys: {},
        notifications: [{ ts: 1, type: 'system_event', risk: 0.7 }],
      }),
    );

    expect(res.status).toBe(200);
    const recipients = (await listEmailDeliveries())
      .filter((delivery) => delivery.kind === 'tamper_alert')
      .map((delivery) => delivery.recipient_email);
    expect(recipients).toEqual(['accepted@ac.test']);
  });
});

describe('malformed ids in the path (API-004)', () => {
  const rows: Array<[string, string, string, string]> = [
    ['API-030', 'PATCH', '/device/abc', 'name'],
    ['API-031', 'DELETE', '/device/abc', ''],
    ['API-046', 'POST', '/locked-password/abc/reveal', ''],
    ['API-047', 'DELETE', '/locked-password/abc', ''],
    ['API-048', 'POST', '/locked-password/abc/restore', ''],
    ['API-049', 'DELETE', '/locked-password/abc/permanent', ''],
  ];

  it.each(rows)(
    '%s: %s %s answers 404, since no such resource exists (API-030, API-031, API-046, API-047, API-048, API-049)',
    async (_id, method, path, field) => {
      const owner = await signupAndGetCookie('malformed@ac.test');

      const res = await SELF.fetch(`${BASE}${path}`, {
        method,
        headers: authHeaders(owner.cookie),
        ...(field ? { body: JSON.stringify({ [field]: 'New name' }) } : {}),
      });

      expect(res.status).toBe(404);
    },
  );

  it.each(['1', 'zzzzzzzz-zzzz-4zzz-8zzz-zzzzzzzzzzzz', `${'0'.repeat(32)}0`])(
    'answers 404 for the id %s',
    async (id) => {
      const owner = await signupAndGetCookie('malformed@ac.test');
      const res = await SELF.fetch(`${BASE}/device/${id}`, {
        method: 'DELETE',
        headers: authHeaders(owner.cookie),
      });
      expect(res.status).toBe(404);
    },
  );

  it('checks the session before the id (API-003)', async () => {
    const res = await SELF.fetch(`${BASE}/device/abc`, { method: 'DELETE' });
    expect(res.status).toBe(401);
  });

  it('answers 500 and logs a bad UUID that did not come from the path (API-004)', async () => {
    const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
    onTestFinished(() => logged.mockRestore());
    const corruptDb = {
      prepare: () => ({ bind: () => ({ first: async () => ({ id: 'not-a-uuid' }) }) }),
    };

    const res = await worker.fetch(
      new Request(`${BASE}/user/login-material?email=corrupt%40ac.test`),
      { ...env, DB: corruptDb as unknown as D1Database },
      createExecutionContext(),
    );

    expect(res.status).toBe(500);
    expect(logged).toHaveBeenCalled();
  });

  it('answers an invalid body on a malformed id as it does on an unknown id (API-004, API-030)', async () => {
    const owner = await signupAndGetCookie('malformed@ac.test');
    for (const id of ['abc', '00000000-0000-4000-8000-000000000000']) {
      const res = await SELF.fetch(`${BASE}/device/${id}`, {
        method: 'PATCH',
        headers: authHeaders(owner.cookie),
        body: JSON.stringify({}),
      });
      expect(res.status).toBe(400);
    }
  });
});
