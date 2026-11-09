import { beforeEach, describe, expect, it } from 'vitest';
import { env, SELF } from 'cloudflare:test';
import {
  acceptPartnerInvite,
  authHeaders,
  invitePartner,
  BASE,
  clearDB,
  listEmailDeliveries,
  markUserEmailVerified,
  signupAndGetCookie,
  uuidToBytes,
} from './helpers';
import { pruneExpiredLockedPasswords } from '../src/lib/retention';

beforeEach(clearDB);

async function acceptPartnership(ownerCookie: string, ownerEmail: string, partnerEmail: string) {
  const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(partnerEmail);
  await markUserEmailVerified(partnerUserId);

  const inviteRes = await SELF.fetch(`${BASE}/partner`, {
    method: 'POST',
    headers: authHeaders(ownerCookie),
    body: JSON.stringify({ email: partnerEmail }),
  });
  expect(inviteRes.status).toBe(200);
  await inviteRes.json();

  const inviteDelivery = (await listEmailDeliveries())
    .reverse()
    .find(
      (delivery) => delivery.kind === 'partner_invite' && delivery.recipient_email === partnerEmail,
    );
  const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

  const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
    method: 'POST',
    headers: authHeaders(partnerCookie),
    body: JSON.stringify({ token: inviteMetadata.inviteToken }),
  });
  expect(acceptRes.status).toBe(200);

  return { partnerCookie, partnerUserId };
}

describe('Locked passwords', () => {
  it('creates a locked password and returns its id (API-044)', async () => {
    const { cookie } = await signupAndGetCookie('alice@example.com');

    const res = await SELF.fetch(`${BASE}/locked-password`, {
      method: 'POST',
      headers: authHeaders(cookie),
      body: JSON.stringify({ label: 'Screen Time passcode', wrapped_value: 'aGVsbG8=' }),
    });

    expect(res.status).toBe(200);
    const body = (await res.json()) as { id: string };
    expect(body.id).toBeTruthy();
  });

  it('rejects creation with invalid data (API-044)', async () => {
    const { cookie } = await signupAndGetCookie('bad-create@example.com');

    const res = await SELF.fetch(`${BASE}/locked-password`, {
      method: 'POST',
      headers: authHeaders(cookie),
      body: JSON.stringify({ label: '', wrapped_value: 'not base64!' }),
    });

    expect(res.status).toBe(400);
  });

  it('rejects an oversized wrapped_value, so this cannot be used for file storage (API-043)', async () => {
    const { cookie } = await signupAndGetCookie('oversized@example.com');

    const res = await SELF.fetch(`${BASE}/locked-password`, {
      method: 'POST',
      headers: authHeaders(cookie),
      body: JSON.stringify({
        label: 'Too big',
        wrapped_value: btoa('x'.repeat(2000)),
      }),
    });

    expect(res.status).toBe(400);
  });

  it('lists only the caller-owned entries, without wrapped_value (API-045)', async () => {
    const { cookie } = await signupAndGetCookie('owner@example.com');
    await signupAndGetCookie('someone-else@example.com');

    await SELF.fetch(`${BASE}/locked-password`, {
      method: 'POST',
      headers: authHeaders(cookie),
      body: JSON.stringify({ label: 'Screen Time passcode', wrapped_value: 'aGVsbG8=' }),
    });

    const res = await SELF.fetch(`${BASE}/locked-password`, { headers: authHeaders(cookie) });
    expect(res.status).toBe(200);
    const body = (await res.json()) as Array<Record<string, unknown>>;
    expect(body).toHaveLength(1);
    expect(body[0]).toMatchObject({
      label: 'Screen Time passcode',
      accessed_at: null,
      deleted_at: null,
    });
    expect(body[0]).not.toHaveProperty('wrapped_value');
  });

  it('404s reveal/delete/restore/permanent-delete for an entry the caller does not own (API-046, API-047, API-048, API-049)', async () => {
    const { cookie: ownerCookie } = await signupAndGetCookie('owns-it@example.com');
    const { cookie: attackerCookie } = await signupAndGetCookie('not-owner@example.com');

    const createRes = await SELF.fetch(`${BASE}/locked-password`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({ label: 'Secret', wrapped_value: 'aGVsbG8=' }),
    });
    const { id } = (await createRes.json()) as { id: string };

    for (const req of [
      { path: `/locked-password/${id}/reveal`, method: 'POST' },
      { path: `/locked-password/${id}`, method: 'DELETE' },
      { path: `/locked-password/${id}/restore`, method: 'POST' },
      { path: `/locked-password/${id}/permanent`, method: 'DELETE' },
    ]) {
      const res = await SELF.fetch(`${BASE}${req.path}`, {
        method: req.method,
        headers: authHeaders(attackerCookie),
      });
      expect(res.status).toBe(404);
    }
  });

  it('reveals the value and permanently flags accessed_at the first time only, without re-alerting (API-046)', async () => {
    const { cookie } = await signupAndGetCookie('reveal-once@example.com');
    await acceptPartnership(cookie, 'reveal-once@example.com', 'reveal-once-partner@example.com');

    const createRes = await SELF.fetch(`${BASE}/locked-password`, {
      method: 'POST',
      headers: authHeaders(cookie),
      body: JSON.stringify({ label: 'Screen Time passcode', wrapped_value: 'aGVsbG8=' }),
    });
    const { id } = (await createRes.json()) as { id: string };

    const firstReveal = await SELF.fetch(`${BASE}/locked-password/${id}/reveal`, {
      method: 'POST',
      headers: authHeaders(cookie),
    });
    expect(firstReveal.status).toBe(200);
    const firstBody = (await firstReveal.json()) as { wrapped_value: string; accessed_at: number };
    expect(firstBody.wrapped_value).toBe('aGVsbG8=');
    expect(firstBody.accessed_at).toBeTypeOf('number');

    const alertEmails = (await listEmailDeliveries()).filter(
      (delivery) => delivery.kind === 'locked_password_accessed',
    );
    expect(alertEmails).toHaveLength(1);
    expect(alertEmails[0]?.recipient_email).toBe('reveal-once-partner@example.com');
    expect(alertEmails[0]?.text).toContain('Screen Time passcode');

    const secondReveal = await SELF.fetch(`${BASE}/locked-password/${id}/reveal`, {
      method: 'POST',
      headers: authHeaders(cookie),
    });
    expect(secondReveal.status).toBe(200);
    const secondBody = (await secondReveal.json()) as { accessed_at: number };
    expect(secondBody.accessed_at).toBe(firstBody.accessed_at);

    const alertEmailsAfterSecondReveal = (await listEmailDeliveries()).filter(
      (delivery) => delivery.kind === 'locked_password_accessed',
    );
    expect(alertEmailsAfterSecondReveal).toHaveLength(1);
  });

  it('skips alerting watchers whose email_frequency is none (API-046)', async () => {
    const { cookie } = await signupAndGetCookie('quiet-owner@example.com');
    const { partnerCookie } = await acceptPartnership(
      cookie,
      'quiet-owner@example.com',
      'quiet-partner@example.com',
    );

    const settingsRes = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ settings: { email_frequency: 'none' } }),
    });
    expect(settingsRes.status).toBe(200);

    const createRes = await SELF.fetch(`${BASE}/locked-password`, {
      method: 'POST',
      headers: authHeaders(cookie),
      body: JSON.stringify({ label: 'Secret', wrapped_value: 'aGVsbG8=' }),
    });
    const { id } = (await createRes.json()) as { id: string };

    await SELF.fetch(`${BASE}/locked-password/${id}/reveal`, {
      method: 'POST',
      headers: authHeaders(cookie),
    });

    const alertEmails = (await listEmailDeliveries()).filter(
      (delivery) => delivery.kind === 'locked_password_accessed',
    );
    expect(alertEmails).toHaveLength(0);
  });

  it('soft-deletes, still lists it, and restore un-deletes it (API-047, API-048, API-045)', async () => {
    const { cookie } = await signupAndGetCookie('soft-delete@example.com');

    const createRes = await SELF.fetch(`${BASE}/locked-password`, {
      method: 'POST',
      headers: authHeaders(cookie),
      body: JSON.stringify({ label: 'Secret', wrapped_value: 'aGVsbG8=' }),
    });
    const { id } = (await createRes.json()) as { id: string };

    const deleteRes = await SELF.fetch(`${BASE}/locked-password/${id}`, {
      method: 'DELETE',
      headers: authHeaders(cookie),
    });
    expect(deleteRes.status).toBe(204);

    const afterDeleteRes = await SELF.fetch(`${BASE}/locked-password`, {
      headers: authHeaders(cookie),
    });
    const afterDelete = (await afterDeleteRes.json()) as Array<{
      id: string;
      deleted_at: number | null;
    }>;
    const found = afterDelete.find((entry) => entry.id === id);
    expect(found?.deleted_at).toBeTypeOf('number');

    const restoreRes = await SELF.fetch(`${BASE}/locked-password/${id}/restore`, {
      method: 'POST',
      headers: authHeaders(cookie),
    });
    expect(restoreRes.status).toBe(204);

    const afterRestoreRes = await SELF.fetch(`${BASE}/locked-password`, {
      headers: authHeaders(cookie),
    });
    const afterRestore = (await afterRestoreRes.json()) as Array<{
      id: string;
      deleted_at: number | null;
    }>;
    expect(afterRestore.find((entry) => entry.id === id)?.deleted_at).toBeNull();
  });

  it('permanently deletes so the entry no longer appears at all (API-049, API-045)', async () => {
    const { cookie } = await signupAndGetCookie('permanent-delete@example.com');

    const createRes = await SELF.fetch(`${BASE}/locked-password`, {
      method: 'POST',
      headers: authHeaders(cookie),
      body: JSON.stringify({ label: 'Secret', wrapped_value: 'aGVsbG8=' }),
    });
    const { id } = (await createRes.json()) as { id: string };

    await SELF.fetch(`${BASE}/locked-password/${id}`, {
      method: 'DELETE',
      headers: authHeaders(cookie),
    });

    const permanentRes = await SELF.fetch(`${BASE}/locked-password/${id}/permanent`, {
      method: 'DELETE',
      headers: authHeaders(cookie),
    });
    expect(permanentRes.status).toBe(204);

    const listRes = await SELF.fetch(`${BASE}/locked-password`, { headers: authHeaders(cookie) });
    const list = (await listRes.json()) as Array<{ id: string }>;
    expect(list.find((entry) => entry.id === id)).toBeUndefined();
  });
});

type LockedEntry = {
  id: string;
  label: string;
  created_at: number;
  accessed_at: number | null;
  deleted_at: number | null;
};

const VALUE = 'dG9wLXNlY3JldC12YWx1ZQ=='; // distinctive, so a leak in an email is easy to spot

async function createEntry(cookie: string, label = 'Screen Time passcode', value = VALUE) {
  const res = await SELF.fetch(`${BASE}/locked-password`, {
    method: 'POST',
    headers: authHeaders(cookie),
    body: JSON.stringify({ label, wrapped_value: value }),
  });
  expect(res.status).toBe(200);
  return ((await res.json()) as { id: string }).id;
}

async function listEntries(cookie: string) {
  const res = await SELF.fetch(`${BASE}/locked-password`, { headers: authHeaders(cookie) });
  expect(res.status).toBe(200);
  return (await res.json()) as LockedEntry[];
}

function lockedCall(cookie: string, method: string, path: string) {
  return SELF.fetch(`${BASE}/locked-password${path}`, { method, headers: authHeaders(cookie) });
}

describe('Locked password requests (API-043 to API-049)', () => {
  it.each([
    ['API-044', 'POST', '', { label: 'x', wrapped_value: 'aGVsbG8=' }],
    ['API-045', 'GET', '', undefined],
    ['API-046', 'POST', '/00000000-0000-4000-8000-000000000001/reveal', undefined],
    ['API-047', 'DELETE', '/00000000-0000-4000-8000-000000000001', undefined],
    ['API-048', 'POST', '/00000000-0000-4000-8000-000000000001/restore', undefined],
    ['API-049', 'DELETE', '/00000000-0000-4000-8000-000000000001/permanent', undefined],
  ])(
    '%s: %s /locked-password%s answers 401 without a web session (API-044, API-045, API-046, API-047, API-048, API-049)',
    async (_id, method, path, body) => {
      const res = await SELF.fetch(`${BASE}/locked-password${path}`, {
        method,
        headers: { 'Content-Type': 'application/json' },
        ...(body ? { body: JSON.stringify(body) } : {}),
      });

      expect(res.status).toBe(401);
    },
  );

  // The label limit is 100 characters and the wrapped_value limit is 1024 Base64
  // characters, both inclusive.
  it.each([
    ['a label of 100 characters', { label: 'x'.repeat(100), wrapped_value: 'aGVsbG8=' }, 200],
    ['a label of 101 characters', { label: 'x'.repeat(101), wrapped_value: 'aGVsbG8=' }, 400],
    ['an empty label', { label: '', wrapped_value: 'aGVsbG8=' }, 400],
    ['no label', { wrapped_value: 'aGVsbG8=' }, 400],
    ['a label that is not a string', { label: 5, wrapped_value: 'aGVsbG8=' }, 400],
    ['a wrapped_value of 1024 characters', { label: 'a', wrapped_value: 'A'.repeat(1024) }, 200],
    ['a wrapped_value of 1028 characters', { label: 'a', wrapped_value: 'A'.repeat(1028) }, 400],
    ['no wrapped_value', { label: 'a' }, 400],
    ['a wrapped_value that is not Base64', { label: 'a', wrapped_value: 'not base64!' }, 400],
    ['a wrapped_value that is not a string', { label: 'a', wrapped_value: 12 }, 400],
  ])('POST /locked-password with %s answers %i (API-043, API-044)', async (_name, body, status) => {
    const { cookie } = await signupAndGetCookie('limits@example.com');

    const res = await SELF.fetch(`${BASE}/locked-password`, {
      method: 'POST',
      headers: authHeaders(cookie),
      body: JSON.stringify(body),
    });

    expect(res.status).toBe(status);
    expect(await listEntries(cookie)).toHaveLength(status === 200 ? 1 : 0);
  });

  it('answers 200 with just the id, and stores the entry against the caller (API-044)', async () => {
    const { cookie } = await signupAndGetCookie('creator@example.com');
    const { cookie: otherCookie } = await signupAndGetCookie('other@example.com');

    const res = await SELF.fetch(`${BASE}/locked-password`, {
      method: 'POST',
      headers: authHeaders(cookie),
      body: JSON.stringify({ label: 'Mine', wrapped_value: VALUE }),
    });

    const body = (await res.json()) as { id: string };
    expect(Object.keys(body)).toEqual(['id']);
    expect((await listEntries(cookie)).map((e) => e.id)).toEqual([body.id]);
    expect(await listEntries(otherCookie)).toEqual([]);
  });

  it('GET lists every entry the caller owns, soft-deleted and accessed ones included, with exactly the listed fields (API-045)', async () => {
    const { cookie } = await signupAndGetCookie('lister@example.com');
    const active = await createEntry(cookie, 'Active');
    const deleted = await createEntry(cookie, 'Deleted');
    const accessed = await createEntry(cookie, 'Accessed');
    await lockedCall(cookie, 'DELETE', `/${deleted}`);
    await lockedCall(cookie, 'POST', `/${accessed}/reveal`);

    const entries = await listEntries(cookie);

    expect(entries.map((e) => e.label).sort()).toEqual(['Accessed', 'Active', 'Deleted']);
    for (const entry of entries) {
      expect(Object.keys(entry).sort()).toEqual(
        ['accessed_at', 'created_at', 'deleted_at', 'id', 'label'].sort(),
      );
    }
    const byId = new Map(entries.map((e) => [e.id, e]));
    expect(byId.get(active)).toMatchObject({ accessed_at: null, deleted_at: null });
    expect(byId.get(deleted)).toMatchObject({ accessed_at: null, deleted_at: expect.any(Number) });
    expect(byId.get(accessed)).toMatchObject({ accessed_at: expect.any(Number), deleted_at: null });
  });
});

describe('Locked password reveal alerts (API-046)', () => {
  it('returns exactly the value and a null-then-set accessed_at, and records the access time (API-046)', async () => {
    const { cookie } = await signupAndGetCookie('revealer@example.com');
    const id = await createEntry(cookie);
    const before = Date.now();

    const res = await lockedCall(cookie, 'POST', `/${id}/reveal`);

    expect(res.status).toBe(200);
    const body = (await res.json()) as { wrapped_value: string; accessed_at: number };
    expect(Object.keys(body).sort()).toEqual(['accessed_at', 'wrapped_value']);
    expect(body.wrapped_value).toBe(VALUE);
    expect(body.accessed_at).toBeGreaterThanOrEqual(before);
    expect((await listEntries(cookie))[0]!.accessed_at).toBe(body.accessed_at);
  });

  it('emails every accepted watcher, naming the label and never the value (API-046)', async () => {
    const { cookie } = await signupAndGetCookie('alerter@example.com');
    await acceptPartnership(cookie, 'alerter@example.com', 'watch-one@example.com');
    await acceptPartnership(cookie, 'alerter@example.com', 'watch-two@example.com');
    const id = await createEntry(cookie, 'Router admin');

    await lockedCall(cookie, 'POST', `/${id}/reveal`);

    const alerts = (await listEmailDeliveries()).filter(
      (d) => d.kind === 'locked_password_accessed',
    );
    expect(alerts.map((d) => d.recipient_email).sort()).toEqual([
      'watch-one@example.com',
      'watch-two@example.com',
    ]);
    for (const alert of alerts) {
      expect(alert.text).toContain('Router admin');
      expect(JSON.stringify(alert)).not.toContain(VALUE);
      expect(JSON.stringify(alert)).not.toContain(atob(VALUE));
    }
  });

  it('does not email an invite that was never accepted (API-046)', async () => {
    const { cookie } = await signupAndGetCookie('pending-owner@example.com');
    await invitePartner(cookie, 'never-accepted@example.com');
    const id = await createEntry(cookie);

    const res = await lockedCall(cookie, 'POST', `/${id}/reveal`);

    expect(res.status).toBe(200);
    expect(
      (await listEmailDeliveries()).filter((d) => d.kind === 'locked_password_accessed'),
    ).toEqual([]);
  });

  it('does not email a watcher whose email is unverified, like a tamper alert (API-046)', async () => {
    const { cookie } = await signupAndGetCookie('verified-owner@example.com');
    const unverified = await signupAndGetCookie('unverified-watcher@example.com');
    const invite = await invitePartner(cookie, 'unverified-watcher@example.com');
    expect((await acceptPartnerInvite(unverified.cookie, invite.token)).status).toBe(200);
    await env.DB.prepare('UPDATE users SET email_verified = 0 WHERE id = ?')
      .bind(uuidToBytes(unverified.userId))
      .run();
    const id = await createEntry(cookie);

    await lockedCall(cookie, 'POST', `/${id}/reveal`);

    expect(
      (await listEmailDeliveries()).filter((d) => d.kind === 'locked_password_accessed'),
    ).toEqual([]);
  });

  it('does not email a watcher who was removed before the reveal (API-046)', async () => {
    const { cookie } = await signupAndGetCookie('removing-owner@example.com');
    const { partnerCookie } = await acceptPartnership(
      cookie,
      'removing-owner@example.com',
      'removed-watcher@example.com',
    );
    const partnership = (
      (await (
        await SELF.fetch(`${BASE}/partner`, { headers: authHeaders(partnerCookie) })
      ).json()) as { watching: Array<{ id: string }> }
    ).watching[0]!;
    await SELF.fetch(`${BASE}/partner/${partnership.id}`, {
      method: 'DELETE',
      headers: authHeaders(cookie),
    });
    const id = await createEntry(cookie);

    await lockedCall(cookie, 'POST', `/${id}/reveal`);

    expect(
      (await listEmailDeliveries()).filter((d) => d.kind === 'locked_password_accessed'),
    ).toEqual([]);
  });

  it('keeps accessed_at when the entry is deleted and restored, and a restored entry reveals without a new alert (API-046, API-047, API-048)', async () => {
    const { cookie } = await signupAndGetCookie('sticky@example.com');
    await acceptPartnership(cookie, 'sticky@example.com', 'sticky-watcher@example.com');
    const id = await createEntry(cookie);
    const first = (await (await lockedCall(cookie, 'POST', `/${id}/reveal`)).json()) as {
      accessed_at: number;
    };

    await lockedCall(cookie, 'DELETE', `/${id}`);
    await lockedCall(cookie, 'POST', `/${id}/restore`);
    const again = (await (await lockedCall(cookie, 'POST', `/${id}/reveal`)).json()) as {
      accessed_at: number;
    };

    expect((await listEntries(cookie))[0]!.accessed_at).toBe(first.accessed_at);
    expect(again.accessed_at).toBe(first.accessed_at);
    expect(
      (await listEmailDeliveries()).filter((d) => d.kind === 'locked_password_accessed'),
    ).toHaveLength(1);
  });

  // The spec says the caller must own the entry and says nothing about deleted entries,
  // so this records the current behavior: a soft-deleted entry can still be revealed.
  it('still reveals a soft-deleted entry to its owner (API-046)', async () => {
    const { cookie } = await signupAndGetCookie('deleted-reveal@example.com');
    const id = await createEntry(cookie);
    await lockedCall(cookie, 'DELETE', `/${id}`);

    const res = await lockedCall(cookie, 'POST', `/${id}/reveal`);

    expect(res.status).toBe(200);
    expect(((await res.json()) as { wrapped_value: string }).wrapped_value).toBe(VALUE);
  });
});

describe('Locked password deletion (API-047, API-048, API-049)', () => {
  it('soft delete answers 204 with an empty body and stamps deleted_at with the current time (API-047)', async () => {
    const { cookie } = await signupAndGetCookie('stamper@example.com');
    const id = await createEntry(cookie);
    const before = Date.now();

    const res = await lockedCall(cookie, 'DELETE', `/${id}`);

    expect(res.status).toBe(204);
    expect(await res.text()).toBe('');
    const deletedAt = (await listEntries(cookie))[0]!.deleted_at;
    expect(deletedAt).toBeGreaterThanOrEqual(before);
    expect(deletedAt).toBeLessThanOrEqual(Date.now());
  });

  it('soft delete leaves the other entries alone (API-047)', async () => {
    const { cookie } = await signupAndGetCookie('selective@example.com');
    const gone = await createEntry(cookie, 'Gone');
    const kept = await createEntry(cookie, 'Kept');

    await lockedCall(cookie, 'DELETE', `/${gone}`);

    const byId = new Map((await listEntries(cookie)).map((e) => [e.id, e]));
    expect(byId.get(kept)!.deleted_at).toBeNull();
    expect(byId.get(gone)!.deleted_at).not.toBeNull();
  });

  it('removes soft-deleted entries once deleted_at is more than 7 days old, and not before (API-047)', async () => {
    const { cookie } = await signupAndGetCookie('retained@example.com');
    const id = await createEntry(cookie);
    await lockedCall(cookie, 'DELETE', `/${id}`);
    const deletedAt = (await listEntries(cookie))[0]!.deleted_at!;
    const week = 7 * 24 * 60 * 60 * 1000;

    await pruneExpiredLockedPasswords(env, deletedAt + week);
    expect(await listEntries(cookie)).toHaveLength(1);

    await pruneExpiredLockedPasswords(env, deletedAt + week + 1);
    expect(await listEntries(cookie)).toEqual([]);
    expect((await lockedCall(cookie, 'POST', `/${id}/reveal`)).status).toBe(404);
  });

  it('never removes an entry that is not soft-deleted, however old (API-047)', async () => {
    const { cookie } = await signupAndGetCookie('ancient@example.com');
    await createEntry(cookie);

    await pruneExpiredLockedPasswords(env, Date.now() + 5 * 365 * 24 * 60 * 60 * 1000);

    expect(await listEntries(cookie)).toHaveLength(1);
  });

  it('restore answers 204, clears deleted_at, and does not touch accessed_at (API-048)', async () => {
    const { cookie } = await signupAndGetCookie('restorer@example.com');
    const id = await createEntry(cookie);
    await lockedCall(cookie, 'POST', `/${id}/reveal`);
    const accessedAt = (await listEntries(cookie))[0]!.accessed_at;
    await lockedCall(cookie, 'DELETE', `/${id}`);

    const res = await lockedCall(cookie, 'POST', `/${id}/restore`);

    expect(res.status).toBe(204);
    expect(await res.text()).toBe('');
    expect((await listEntries(cookie))[0]).toMatchObject({
      deleted_at: null,
      accessed_at: accessedAt,
    });
  });

  it('restore of an entry that is not deleted answers 204 and changes nothing (API-048)', async () => {
    const { cookie } = await signupAndGetCookie('restore-noop@example.com');
    const id = await createEntry(cookie);
    const before = await listEntries(cookie);

    const res = await lockedCall(cookie, 'POST', `/${id}/restore`);

    expect(res.status).toBe(204);
    expect(await listEntries(cookie)).toEqual(before);
  });

  it.each([
    ['an active entry', false],
    ['a soft-deleted entry', true],
  ])(
    'permanent delete of %s answers 204, removes it for good and leaves the others (API-049)',
    async (_name, softDeleteFirst) => {
      const { cookie } = await signupAndGetCookie('permanent@example.com');
      const target = await createEntry(cookie, 'Target');
      const other = await createEntry(cookie, 'Other');
      if (softDeleteFirst) await lockedCall(cookie, 'DELETE', `/${target}`);

      const res = await lockedCall(cookie, 'DELETE', `/${target}/permanent`);

      expect(res.status).toBe(204);
      expect(await res.text()).toBe('');
      expect((await listEntries(cookie)).map((e) => e.id)).toEqual([other]);
      expect((await lockedCall(cookie, 'POST', `/${target}/reveal`)).status).toBe(404);
      expect((await lockedCall(cookie, 'POST', `/${target}/restore`)).status).toBe(404);
      expect((await lockedCall(cookie, 'DELETE', `/${target}/permanent`)).status).toBe(404);
    },
  );
});
