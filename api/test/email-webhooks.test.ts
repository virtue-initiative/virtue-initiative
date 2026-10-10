import { beforeEach, describe, expect, it } from 'vitest';
import { env, SELF } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  latestEmailToken,
  listEmailDeliveries,
  markUserEmailVerified,
  passwordAuthFor,
  signupAndGetCookie,
  uuidToBytes,
} from './helpers';

beforeEach(clearDB);

function postBounce(
  email: string,
  bounceType: 'Permanent' | 'Transient' | 'Undetermined',
  emailTokenId?: string,
) {
  return SELF.fetch(`${BASE}/email/sns`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      Type: 'Notification',
      Message: JSON.stringify({
        eventType: 'Bounce',
        mail: { tags: emailTokenId ? { email_token_id: [emailTokenId] } : {} },
        bounce: { bounceType, bouncedRecipients: [{ emailAddress: email }] },
      }),
    }),
  });
}

async function listBounces(email: string) {
  const { results } = await env.DB.prepare(
    'SELECT reason FROM email_bounces WHERE email = ? ORDER BY bounced_at',
  )
    .bind(email)
    .all<{ reason: string }>();
  return results.map((row) => row.reason);
}

async function tokenHasBounce(tokenId: string) {
  const row = await env.DB.prepare('SELECT bounce_id FROM email_tokens WHERE id = ?')
    .bind(uuidToBytes(tokenId))
    .first<{ bounce_id: ArrayBuffer | null }>();
  return row?.bounce_id != null;
}

async function invitePartner(cookie: string, email: string, replaceId?: string) {
  return SELF.fetch(`${BASE}/partner`, {
    method: 'POST',
    headers: authHeaders(cookie),
    body: JSON.stringify({ email, ...(replaceId ? { replace_id: replaceId } : {}) }),
  });
}

async function listWatchers(cookie: string) {
  const res = await SELF.fetch(`${BASE}/partner`, { headers: authHeaders(cookie) });
  const body = (await res.json()) as {
    watchers: Array<{ id: string; status: string; user: { email: string } }>;
  };
  return body.watchers;
}

describe('Email webhooks', () => {
  it('marks users unverified on SNS bounce notifications', async () => {
    const { userId } = await signupAndGetCookie('bounce@example.com', 'pw');
    await markUserEmailVerified(userId);

    const res = await SELF.fetch(`${BASE}/email/sns`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        Type: 'Notification',
        Message: JSON.stringify({
          eventType: 'Bounce',
          bounce: {
            bounceType: 'Permanent',
            bouncedRecipients: [{ emailAddress: 'bounce@example.com' }],
          },
        }),
      }),
    });

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, updated: 1 });

    const user = await env.DB.prepare(
      'SELECT email_verified, email_bounced_at FROM users WHERE id = ?',
    )
      .bind(uuidToBytes(userId))
      .first<{ email_verified: number; email_bounced_at: number | null }>();
    expect(user?.email_verified).toBe(0);
    expect(typeof user?.email_bounced_at).toBe('number');
  });

  it('marks users unverified on SNS complaint notifications', async () => {
    const { userId } = await signupAndGetCookie('complaint@example.com', 'pw');
    await markUserEmailVerified(userId);

    const res = await SELF.fetch(`${BASE}/email/sns`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        Type: 'Notification',
        Message: JSON.stringify({
          eventType: 'Complaint',
          complaint: {
            complainedRecipients: [{ emailAddress: 'complaint@example.com' }],
          },
        }),
      }),
    });

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, updated: 1 });

    const user = await env.DB.prepare(
      'SELECT email_verified, email_bounced_at FROM users WHERE id = ?',
    )
      .bind(uuidToBytes(userId))
      .first<{ email_verified: number; email_bounced_at: number | null }>();
    expect(user?.email_verified).toBe(0);
    expect(user?.email_bounced_at).toBeNull();
  });

  it('records a temporary bounce without unverifying the user or blocking the address', async () => {
    const { userId } = await signupAndGetCookie('soft@example.com', 'pw');
    await markUserEmailVerified(userId);

    expect((await postBounce('soft@example.com', 'Transient')).status).toBe(200);

    expect(await listBounces('soft@example.com')).toEqual(['temporary']);
    const user = await env.DB.prepare('SELECT email_verified FROM users WHERE id = ?')
      .bind(uuidToBytes(userId))
      .first<{ email_verified: number }>();
    expect(user?.email_verified).toBe(1);

    const before = (await listEmailDeliveries()).length;
    const resetRes = await SELF.fetch(`${BASE}/password-reset`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'soft@example.com' }),
    });
    expect(resetRes.status).toBe(204);
    expect((await listEmailDeliveries()).length).toBe(before + 1);
  });

  it('marks only the tagged token on a temporary bounce', async () => {
    const { cookie, userId } = await signupAndGetCookie('owner@example.com', 'pw');
    await markUserEmailVerified(userId);
    const { cookie: otherCookie, userId: otherId } = await signupAndGetCookie(
      'other@example.com',
      'pw',
    );
    await markUserEmailVerified(otherId);

    await invitePartner(cookie, 'friend@example.com');
    const firstToken = await latestEmailToken('partner_invite');
    await invitePartner(otherCookie, 'friend@example.com');

    await postBounce('friend@example.com', 'Transient', firstToken!.id);

    expect((await listWatchers(cookie))[0].status).toBe('invite_failed');
    expect((await listWatchers(otherCookie))[0].status).toBe('pending');
  });

  it('fails every outstanding invite to a permanently bounced address', async () => {
    const { cookie, userId } = await signupAndGetCookie('owner@example.com', 'pw');
    await markUserEmailVerified(userId);
    const { cookie: otherCookie, userId: otherId } = await signupAndGetCookie(
      'other@example.com',
      'pw',
    );
    await markUserEmailVerified(otherId);

    await invitePartner(cookie, 'gone@example.com');
    await invitePartner(otherCookie, 'gone@example.com');

    await postBounce('gone@example.com', 'Permanent');

    expect((await listWatchers(cookie))[0].status).toBe('invite_failed');
    expect((await listWatchers(otherCookie))[0].status).toBe('invite_failed');
  });

  it('does not send to a permanently bounced address and fails the new invite immediately', async () => {
    const { cookie, userId } = await signupAndGetCookie('owner@example.com', 'pw');
    await markUserEmailVerified(userId);
    await postBounce('gone@example.com', 'Permanent');

    const before = (await listEmailDeliveries()).length;
    const res = await invitePartner(cookie, 'gone@example.com');
    expect(res.status).toBe(200);
    expect(((await res.json()) as { status: string }).status).toBe('invite_failed');
    expect((await listEmailDeliveries()).length).toBe(before);

    const token = await latestEmailToken('partner_invite');
    expect(await tokenHasBounce(token!.id)).toBe(true);
    expect((await listWatchers(cookie))[0].status).toBe('invite_failed');
  });

  it('resends a failed invite to a new email by replacing the old one', async () => {
    const { cookie, userId } = await signupAndGetCookie('owner@example.com', 'pw');
    await markUserEmailVerified(userId);
    const created = (await (await invitePartner(cookie, 'typo@example.com')).json()) as {
      id: string;
    };
    await postBounce('typo@example.com', 'Permanent');

    const res = await invitePartner(cookie, 'right@example.com', created.id);
    expect(res.status).toBe(200);
    expect(((await res.json()) as { status: string }).status).toBe('pending');

    const watchers = await listWatchers(cookie);
    expect(watchers).toHaveLength(1);
    expect(watchers[0]).toMatchObject({ status: 'pending', user: { email: 'right@example.com' } });
  });

  it('resends to the same email after a temporary bounce', async () => {
    const { cookie, userId } = await signupAndGetCookie('owner@example.com', 'pw');
    await markUserEmailVerified(userId);
    const created = (await (await invitePartner(cookie, 'full@example.com')).json()) as {
      id: string;
    };
    const token = await latestEmailToken('partner_invite');
    await postBounce('full@example.com', 'Transient', token!.id);
    expect((await listWatchers(cookie))[0].status).toBe('invite_failed');

    const res = await invitePartner(cookie, 'full@example.com', created.id);
    expect(res.status).toBe(200);
    const watchers = await listWatchers(cookie);
    expect(watchers).toHaveLength(1);
    expect(watchers[0].status).toBe('pending');
  });

  it("rejects replace_id for someone else's or an accepted partnership", async () => {
    const { cookie, userId } = await signupAndGetCookie('owner@example.com', 'pw');
    await markUserEmailVerified(userId);
    const { cookie: otherCookie } = await signupAndGetCookie('other@example.com', 'pw');
    const created = (await (await invitePartner(cookie, 'friend@example.com')).json()) as {
      id: string;
    };

    const res = await invitePartner(otherCookie, 'new@example.com', created.id);
    expect(res.status).toBe(404);
    expect(await listWatchers(cookie)).toHaveLength(1);
  });

  it('tells signup-request that the address bounced instead of sending', async () => {
    await postBounce('gone@example.com', 'Permanent');

    const res = await SELF.fetch(`${BASE}/signup-request`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'gone@example.com' }),
    });
    expect(res.status).toBe(422);
    expect(((await res.json()) as { code: string }).code).toBe('email_bounced');
    expect(await listEmailDeliveries()).toHaveLength(0);
  });

  it('reports the bounce instead of resending verification at login', async () => {
    const { userId } = await signupAndGetCookie('gone@example.com', 'pw');
    await markUserEmailVerified(userId);
    await postBounce('gone@example.com', 'Permanent');

    const before = (await listEmailDeliveries()).length;
    const res = await SELF.fetch(`${BASE}/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email: 'gone@example.com',
        password_auth: await passwordAuthFor('pw'),
      }),
    });
    expect(res.status).toBe(403);
    expect(((await res.json()) as { code?: string }).code).toBe('email_bounced');
    expect((await listEmailDeliveries()).length).toBe(before);
    const token = await latestEmailToken('email_verification');
    expect(await tokenHasBounce(token!.id)).toBe(true);
  });

  it('reports the bounce when changing email to a permanently bounced address', async () => {
    const { cookie, userId } = await signupAndGetCookie('user@example.com', 'pw');
    await markUserEmailVerified(userId);
    await postBounce('gone@example.com', 'Permanent');

    const before = (await listEmailDeliveries()).length;
    const res = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(cookie),
      body: JSON.stringify({ email: 'gone@example.com' }),
    });
    expect(res.status).toBe(200);
    expect(await res.json()).toMatchObject({
      email_bounced: true,
      pending_email: 'gone@example.com',
    });
    expect((await listEmailDeliveries()).length).toBe(before);
  });

  it('clears bounces for an address once a verification link sent to it is used', async () => {
    const { userId } = await signupAndGetCookie('back@example.com', 'pw');
    await env.DB.prepare('UPDATE users SET email_verified = 0 WHERE id = ?')
      .bind(uuidToBytes(userId))
      .run();
    await SELF.fetch(`${BASE}/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email: 'back@example.com',
        password_auth: await passwordAuthFor('pw'),
      }),
    });
    const delivery = (await listEmailDeliveries()).reverse()[0];
    const verifyUrl = (JSON.parse(delivery.metadata) as { verifyUrl: string }).verifyUrl;
    const token = new URL(verifyUrl).searchParams.get('token');
    await postBounce('back@example.com', 'Permanent');

    const res = await SELF.fetch(`${BASE}/email-verification/validate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token }),
    });
    expect(res.status).toBe(200);
    expect(await listBounces('back@example.com')).toEqual([]);
  });
});
