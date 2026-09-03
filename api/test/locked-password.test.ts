import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  listEmailDeliveries,
  markUserEmailVerified,
  signupAndGetCookie,
} from './helpers';

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
  it('creates a locked password and returns its id', async () => {
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

  it('rejects creation with invalid data', async () => {
    const { cookie } = await signupAndGetCookie('bad-create@example.com');

    const res = await SELF.fetch(`${BASE}/locked-password`, {
      method: 'POST',
      headers: authHeaders(cookie),
      body: JSON.stringify({ label: '', wrapped_value: 'not base64!' }),
    });

    expect(res.status).toBe(400);
  });

  it('lists only the caller-owned entries, without wrapped_value', async () => {
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

  it('404s reveal/delete/restore/permanent-delete for an entry the caller does not own', async () => {
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

  it('reveals the value and permanently flags accessed_at the first time only, without re-alerting', async () => {
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

  it('skips alerting watchers whose email_frequency is none', async () => {
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

  it('soft-deletes, still lists it, and restore un-deletes it', async () => {
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

  it('permanently deletes so the entry no longer appears at all', async () => {
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
