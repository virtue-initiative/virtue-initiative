import { beforeEach, describe, expect, it } from 'vitest';
import { env, SELF } from 'cloudflare:test';
import {
  acceptPartnerInvite,
  authHeaders,
  BASE,
  clearDB,
  invitePartner,
  listEmailDeliveries,
  markUserEmailVerified,
  signupAndGetCookie,
} from './helpers';
import { generateOpaqueToken } from '../src/lib/tokens';

beforeEach(clearDB);

describe('Partner routes', () => {
  it('creates, accepts, lists, and deletes a partnership without shared-key fields (API-023, API-025, API-026, API-027)', async () => {
    const { cookie: ownerCookie, userId: ownerUserId } = await signupAndGetCookie(
      'owner@example.com',
      'pw',
      'Owner',
    );
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'partner@example.com',
      'pw',
      'Partner',
    );
    await markUserEmailVerified(ownerUserId);

    const createRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({ email: 'partner@example.com' }),
    });
    expect(createRes.status).toBe(200);
    const created = (await createRes.json()) as { id: string };

    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) => delivery.kind === 'partner_invite',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(200);

    const ownerListRes = await SELF.fetch(`${BASE}/partner`, {
      headers: authHeaders(ownerCookie),
    });
    const ownerList = (await ownerListRes.json()) as {
      watchers: Array<{
        id: string;
        user: { id?: string; email: string; name?: string };
        status: 'pending' | 'accepted';
      }>;
    };
    expect(ownerList.watchers.find((partner) => partner.id === created.id)).toMatchObject({
      id: created.id,
      user: {
        id: partnerUserId,
        email: 'partner@example.com',
        name: 'Partner',
      },
      status: 'accepted',
    });

    const partnerListRes = await SELF.fetch(`${BASE}/partner`, {
      headers: authHeaders(partnerCookie),
    });
    const partnerList = (await partnerListRes.json()) as {
      watching: Array<{
        id: string;
        user: { id: string; email: string; name?: string };
        status: 'pending' | 'accepted';
      }>;
    };
    expect(partnerList.watching.find((partner) => partner.id === created.id)?.user.id).toBe(
      ownerUserId,
    );

    const deleteRes = await SELF.fetch(`${BASE}/partner/${created.id}`, {
      method: 'DELETE',
      headers: authHeaders(partnerCookie),
    });
    expect(deleteRes.status).toBe(204);
  });

  it('supports inviting an email before the partner account exists (API-023, API-024, API-025, API-026)', async () => {
    const { cookie: ownerCookie, userId: ownerUserId } =
      await signupAndGetCookie('owner2@example.com');
    await markUserEmailVerified(ownerUserId);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({ email: 'future@example.com' }),
    });
    expect(inviteRes.status).toBe(200);
    const created = (await inviteRes.json()) as { id: string };
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' && delivery.recipient_email === 'future@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    const { cookie: futureCookie } = await signupAndGetCookie('accepted@example.com');

    const validateRes = await SELF.fetch(`${BASE}/partner/validate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(validateRes.status).toBe(200);
    expect(await validateRes.json()).toMatchObject({
      ok: true,
      partnership_id: created.id,
      owner: { email: 'owner2@example.com' },
    });

    const listWatchers = async () => {
      const res = await SELF.fetch(`${BASE}/partner`, { headers: authHeaders(ownerCookie) });
      const body = (await res.json()) as {
        watchers: Array<{ id: string; created_at: number; accepted_at?: number }>;
      };
      return body.watchers.find((partner) => partner.id === created.id);
    };

    const pending = await listWatchers();
    expect(pending?.created_at).toBeTypeOf('number');
    expect(pending).not.toHaveProperty('accepted_at');

    const beforeAccept = Date.now();
    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(futureCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(200);

    // The accept date is separate from the invite date.
    const accepted = await listWatchers();
    expect(accepted?.created_at).toBe(pending?.created_at);
    expect(accepted?.accepted_at).toBeGreaterThanOrEqual(beforeAccept);

    const ownerPartnersRes = await SELF.fetch(`${BASE}/partner`, {
      headers: authHeaders(ownerCookie),
    });
    const ownerPartners = (await ownerPartnersRes.json()) as {
      watchers: Array<{
        id: string;
        user: { email: string };
        status: 'pending' | 'accepted';
      }>;
    };
    const owned = ownerPartners.watchers.find((partner) => partner.id === created.id);
    expect(owned?.user.email).toBe('accepted@example.com');
    expect(owned?.status).toBe('accepted');
  });

  it('lets either side of an accepted partnership delete it via DELETE /partner/:id (API-027)', async () => {
    const { cookie: ownerCookie, userId: ownerUserId } = await signupAndGetCookie(
      'owner3@example.com',
      'pw',
      'Owner',
    );
    const { cookie: partnerCookie } = await signupAndGetCookie(
      'partner3@example.com',
      'pw',
      'Partner',
    );
    const { cookie: outsiderCookie } = await signupAndGetCookie('outsider3@example.com', 'pw');
    await markUserEmailVerified(ownerUserId);

    const createRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({ email: 'partner3@example.com' }),
    });
    const created = (await createRes.json()) as { id: string };
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' && delivery.recipient_email === 'partner3@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };
    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const outsiderDeleteRes = await SELF.fetch(`${BASE}/partner/${created.id}`, {
      method: 'DELETE',
      headers: authHeaders(outsiderCookie),
    });
    expect(outsiderDeleteRes.status).toBe(404);

    // The owning side (the person being watched) can also delete the partnership,
    // not just the watcher — DELETE /partner/:id works from either direction.
    const ownerDeleteRes = await SELF.fetch(`${BASE}/partner/${created.id}`, {
      method: 'DELETE',
      headers: authHeaders(ownerCookie),
    });
    expect(ownerDeleteRes.status).toBe(204);

    const ownerListRes = await SELF.fetch(`${BASE}/partner`, {
      headers: authHeaders(ownerCookie),
    });
    const ownerList = (await ownerListRes.json()) as { watchers: Array<{ id: string }> };
    expect(ownerList.watchers.find((partner) => partner.id === created.id)).toBeUndefined();
  });

  it('prevents accepting your own invite link (API-025)', async () => {
    const { cookie: ownerCookie, userId: ownerUserId } =
      await signupAndGetCookie('owner4@example.com');
    await markUserEmailVerified(ownerUserId);

    const createRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({
        email: 'someone-else@example.com',
      }),
    });
    expect(createRes.status).toBe(200);
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'someone-else@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(409);
    expect(await acceptRes.json()).toEqual({
      error: 'You cannot accept your own partner invite',
    });
  });
});

type PartnerList = {
  watching: Array<{ id: string; user: { id?: string; email: string }; status: string }>;
  watchers: Array<{
    id: string;
    user: { id?: string; email: string; name?: string };
    status: string;
    created_at: number;
    accepted_at?: number;
  }>;
};

async function listPartners(cookie: string) {
  const res = await SELF.fetch(`${BASE}/partner`, { headers: authHeaders(cookie) });
  expect(res.status).toBe(200);
  return (await res.json()) as PartnerList;
}

function validate(token: string) {
  return SELF.fetch(`${BASE}/partner/validate`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ token }),
  });
}

function removePartnership(cookie: string, id: string) {
  return SELF.fetch(`${BASE}/partner/${id}`, { method: 'DELETE', headers: authHeaders(cookie) });
}

async function ownerAndPartner() {
  const owner = await signupAndGetCookie('own@partners.test', 'password123', 'Olive Owner');
  const partner = await signupAndGetCookie('par@partners.test', 'password123', 'Pat Partner');
  await markUserEmailVerified(owner.userId);
  await markUserEmailVerified(partner.userId);
  return { owner, partner };
}

describe('every partner endpoint but validate needs a web session', () => {
  it.each([
    ['API-023', 'POST', '/partner', { email: 'x@partners.test' }],
    ['API-025', 'POST', '/partner/accept', { token: 'x' }],
    ['API-026', 'GET', '/partner', undefined],
    ['API-027', 'DELETE', '/partner/00000000-0000-4000-8000-000000000001', undefined],
  ])(
    '%s: %s %s answers 401 without a session (API-023, API-025, API-026, API-027)',
    async (_id, method, path, body) => {
      const res = await SELF.fetch(`${BASE}${path}`, {
        method,
        headers: { 'Content-Type': 'application/json' },
        ...(body ? { body: JSON.stringify(body) } : {}),
      });

      expect(res.status).toBe(401);
    },
  );

  it('rejects a device token used as a web token (API-023)', async () => {
    const res = await SELF.fetch(`${BASE}/partner`, {
      headers: { Authorization: 'Bearer vi_dev_not-a-web-token' },
    });

    expect(res.status).toBe(401);
  });
});

describe('POST /partner (API-023)', () => {
  it('emails the invite, with the accept link, to the invited address only (API-023)', async () => {
    const { owner } = await ownerAndPartner();

    const { token } = await invitePartner(owner.cookie, 'new-person@partners.test');

    const invites = (await listEmailDeliveries()).filter((d) => d.kind === 'partner_invite');
    expect(invites).toHaveLength(1);
    expect(invites[0]!.recipient_email).toBe('new-person@partners.test');
    expect(invites[0]!.text).toContain(`partner_token=${encodeURIComponent(token)}`);
  });

  it('answers 200 with just the new id and a pending status (API-023)', async () => {
    const { owner } = await ownerAndPartner();

    const res = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(owner.cookie),
      body: JSON.stringify({ email: 'new-person@partners.test' }),
    });

    expect(res.status).toBe(200);
    const body = (await res.json()) as { id: string; status: string };
    expect(Object.keys(body).sort()).toEqual(['id', 'status']);
    expect(body.status).toBe('pending');
    const list = await listPartners(owner.cookie);
    expect(list.watchers.map((p) => p.id)).toEqual([body.id]);
  });

  it('replaces a pending invite named by replace_id: the old one and its token stop working (API-023)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const first = await invitePartner(owner.cookie, 'typo@partners.test');

    const second = await invitePartner(owner.cookie, 'par@partners.test').catch(() => null);
    expect(second).not.toBeNull();
    const replaceRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(owner.cookie),
      body: JSON.stringify({ email: 'right@partners.test', replace_id: first.id }),
    });

    expect(replaceRes.status).toBe(200);
    const replaced = (await replaceRes.json()) as { id: string };
    expect(replaced.id).not.toBe(first.id);
    const list = await listPartners(owner.cookie);
    expect(list.watchers.map((p) => p.id).sort()).toEqual([replaced.id, second!.id].sort());
    expect((await acceptPartnerInvite(partner.cookie, first.token)).status).not.toBe(200);
    expect((await listPartners(owner.cookie)).watchers.map((p) => p.id)).not.toContain(first.id);
  });

  it.each([['an id that does not exist', () => '00000000-0000-4000-8000-0000000000ee']])(
    'answers 404 for replace_id naming %s, and creates no invite (API-023)',
    async (_name, pick) => {
      const { owner } = await ownerAndPartner();

      const res = await SELF.fetch(`${BASE}/partner`, {
        method: 'POST',
        headers: authHeaders(owner.cookie),
        body: JSON.stringify({ email: 'new@partners.test', replace_id: pick() }),
      });

      expect(res.status).toBe(404);
      expect((await listPartners(owner.cookie)).watchers).toEqual([]);
    },
  );

  it('answers 404 for replace_id naming an accepted partnership, and leaves it accepted (API-023)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');
    expect((await acceptPartnerInvite(partner.cookie, invite.token)).status).toBe(200);

    const res = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(owner.cookie),
      body: JSON.stringify({ email: 'other@partners.test', replace_id: invite.id }),
    });

    expect(res.status).toBe(404);
    const watchers = (await listPartners(owner.cookie)).watchers;
    expect(watchers).toHaveLength(1);
    expect(watchers[0]).toMatchObject({ id: invite.id, status: 'accepted' });
  });

  it('answers 404 for replace_id naming a partnership the caller is only the watcher of (API-023)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');

    const res = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(partner.cookie),
      body: JSON.stringify({ email: 'other@partners.test', replace_id: invite.id }),
    });

    expect(res.status).toBe(404);
    expect((await listPartners(owner.cookie)).watchers.map((p) => p.id)).toEqual([invite.id]);
  });

  // The spec does not say what happens for these, so the rows record current behavior.
  it.each([
    ['an invalid email', { email: 'not-an-email' }, 400],
    ['no email', {}, 400],
    ['the caller own email', { email: 'own@partners.test' }, 400],
    ['a replace_id that is not a UUID', { email: 'x@partners.test', replace_id: 'abc' }, 400],
  ])('answers %s with %i and creates no invite (API-023)', async (_name, body, status) => {
    const { owner } = await ownerAndPartner();

    const res = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(owner.cookie),
      body: JSON.stringify(body),
    });

    expect(res.status).toBe(status);
    expect((await listPartners(owner.cookie)).watchers).toEqual([]);
  });

  it('answers 409 for a second invite to an address that already has one, and sends no second email (API-023)', async () => {
    const { owner } = await ownerAndPartner();
    await invitePartner(owner.cookie, 'par@partners.test');

    const res = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(owner.cookie),
      body: JSON.stringify({ email: 'par@partners.test' }),
    });

    expect(res.status).toBe(409);
    expect((await listEmailDeliveries()).filter((d) => d.kind === 'partner_invite')).toHaveLength(
      1,
    );
  });
});

describe('POST /partner/validate (API-024)', () => {
  const unknownTokens: Array<[string, () => string]> = [
    ['a made-up string', () => 'nope'],
    ['a well-formed token nobody was issued', () => generateOpaqueToken('partner_invite')],
  ];

  it.each(unknownTokens)('answers 400 for %s (API-024)', async (_name, token) => {
    const res = await validate(token());

    expect(res.status).toBe(400);
  });

  it.each(unknownTokens)('does not reveal an owner for %s (API-024)', async (_name, token) => {
    const res = await validate(token());

    expect(res.ok).toBe(false);
    expect(await res.json()).not.toHaveProperty('owner');
  });

  it('answers 400 for the token of an invite the owner deleted (API-024)', async () => {
    const { owner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');
    expect((await removePartnership(owner.cookie, invite.id)).status).toBe(204);

    const res = await validate(invite.token);

    expect(res.status).toBe(400);
  });

  // Whether a used or lapsed invite counts as invalid is not stated, so these rows record
  // the current behavior: refused, with no owner revealed.
  it('refuses the token of an accepted invite without revealing the owner (API-024)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');
    expect((await acceptPartnerInvite(partner.cookie, invite.token)).status).toBe(200);

    const res = await validate(invite.token);

    expect(res.status).toBe(400);
    expect(await res.json()).not.toHaveProperty('owner');
  });

  it('refuses the token of an expired invite without revealing the owner (API-024)', async () => {
    const { owner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');
    await env.DB.prepare(
      "UPDATE email_tokens SET expires_at = 1 WHERE purpose = 'partner_invite'",
    ).run();

    const res = await validate(invite.token);

    expect(res.status).toBe(400);
    expect(await res.json()).not.toHaveProperty('owner');
  });

  it.each([
    ['no token', {}],
    ['an empty token', { token: '' }],
    ['a token that is not a string', { token: 5 }],
  ])('answers 400 for %s (API-024)', async (_name, body) => {
    const res = await SELF.fetch(`${BASE}/partner/validate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });

    expect(res.status).toBe(400);
  });

  it('needs no session, names the owner and partnership, and leaves the invite usable (API-024)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');

    for (let i = 0; i < 2; i += 1) {
      const res = await validate(invite.token);
      expect(res.status).toBe(200);
      expect(await res.json()).toEqual({
        ok: true,
        partnership_id: invite.id,
        owner: { id: owner.userId, email: 'own@partners.test', name: 'Olive Owner' },
      });
    }
    expect((await acceptPartnerInvite(partner.cookie, invite.token)).status).toBe(200);
  });

  it('omits the owner name when the owner has none (API-024)', async () => {
    const owner = await signupAndGetCookie('nameless@partners.test');
    await markUserEmailVerified(owner.userId);
    const invite = await invitePartner(owner.cookie, 'par@partners.test');

    const res = await validate(invite.token);

    const body = (await res.json()) as { owner: Record<string, unknown> };
    expect(body.owner).toEqual({ id: owner.userId, email: 'nameless@partners.test' });
  });
});

describe('POST /partner/accept (API-025)', () => {
  it('finalizes the partnership for both sides and emails the owner (API-025)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');
    const before = Date.now();

    const res = await acceptPartnerInvite(partner.cookie, invite.token);

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ id: invite.id });
    const ownerView = await listPartners(owner.cookie);
    expect(ownerView.watchers[0]).toMatchObject({
      id: invite.id,
      status: 'accepted',
      user: { id: partner.userId, email: 'par@partners.test', name: 'Pat Partner' },
    });
    expect(ownerView.watchers[0]!.accepted_at).toBeGreaterThanOrEqual(before);
    const partnerView = await listPartners(partner.cookie);
    expect(partnerView.watching).toMatchObject([
      { id: invite.id, status: 'accepted', user: { id: owner.userId } },
    ]);
    const mail = (await listEmailDeliveries()).filter((d) => d.kind === 'partner_accepted');
    expect(mail.map((d) => d.recipient_email)).toEqual(['own@partners.test']);
  });

  // The spec does not say which status an unusable token gets here, so these rows record
  // the current behavior: 400, and no partnership is created.
  it.each([
    ['a made-up string', () => 'nope'],
    ['a well-formed token nobody was issued', () => generateOpaqueToken('partner_invite')],
  ])('answers 400 for %s and creates no partnership (API-025)', async (_name, token) => {
    const { partner } = await ownerAndPartner();

    const res = await acceptPartnerInvite(partner.cookie, token());

    expect(res.status).toBe(400);
    expect((await listPartners(partner.cookie)).watching).toEqual([]);
  });

  it('answers 400 for the token of an expired invite and creates no partnership (API-025)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');
    await env.DB.prepare(
      "UPDATE email_tokens SET expires_at = 1 WHERE purpose = 'partner_invite'",
    ).run();

    const res = await acceptPartnerInvite(partner.cookie, invite.token);

    expect(res.status).toBe(400);
    expect((await listPartners(owner.cookie)).watchers[0]).toMatchObject({ status: 'pending' });
  });

  it('answers 400 for the token of an invite the owner withdrew (API-025)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');
    expect((await removePartnership(owner.cookie, invite.id)).status).toBe(204);

    const res = await acceptPartnerInvite(partner.cookie, invite.token);

    expect(res.status).toBe(400);
    expect((await listPartners(partner.cookie)).watching).toEqual([]);
  });

  it('lets a second user not take over an invite that was already accepted (API-025)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const intruder = await signupAndGetCookie('intruder@partners.test');
    const invite = await invitePartner(owner.cookie, 'par@partners.test');
    expect((await acceptPartnerInvite(partner.cookie, invite.token)).status).toBe(200);

    const res = await acceptPartnerInvite(intruder.cookie, invite.token);

    expect(res.status).toBe(400);
    expect((await listPartners(intruder.cookie)).watching).toEqual([]);
    expect((await listPartners(owner.cookie)).watchers[0]!.user.id).toBe(partner.userId);
  });

  it('answers 200 with the same id when the user who accepted accepts again, and emails the owner once (API-025)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');
    await acceptPartnerInvite(partner.cookie, invite.token);

    const res = await acceptPartnerInvite(partner.cookie, invite.token);

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ id: invite.id });
    expect((await listEmailDeliveries()).filter((d) => d.kind === 'partner_accepted')).toHaveLength(
      1,
    );
  });

  it('lets an account under a different email accept, and lists that account as the watcher (API-025)', async () => {
    const { owner } = await ownerAndPartner();
    const other = await signupAndGetCookie('different@partners.test', 'password123', 'Dee');
    const invite = await invitePartner(owner.cookie, 'invited-address@partners.test');

    const res = await acceptPartnerInvite(other.cookie, invite.token);

    expect(res.status).toBe(200);
    expect((await listPartners(owner.cookie)).watchers[0]).toMatchObject({
      status: 'accepted',
      user: { id: other.userId, email: 'different@partners.test' },
    });
  });

  it('answers 409 when the accepting user already watches the owner, and keeps the second invite pending (API-025)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const first = await invitePartner(owner.cookie, 'par@partners.test');
    await acceptPartnerInvite(partner.cookie, first.token);
    const second = await invitePartner(owner.cookie, 'par-alias@partners.test');

    const res = await acceptPartnerInvite(partner.cookie, second.token);

    expect(res.status).toBe(409);
    const statuses = (await listPartners(owner.cookie)).watchers.map((p) => p.status).sort();
    expect(statuses).toEqual(['accepted', 'pending']);
  });

  it('answers 409 to the sender and leaves the invite pending (API-025)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');

    const res = await acceptPartnerInvite(owner.cookie, invite.token);

    expect(res.status).toBe(409);
    expect((await listPartners(owner.cookie)).watchers[0]).toMatchObject({ status: 'pending' });
    expect((await acceptPartnerInvite(partner.cookie, invite.token)).status).toBe(200);
  });
});

describe('GET /partner (API-026)', () => {
  it('answers two empty lists for a user with no partnerships (API-026)', async () => {
    const { owner } = await ownerAndPartner();

    const res = await SELF.fetch(`${BASE}/partner`, { headers: authHeaders(owner.cookie) });

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ watching: [], watchers: [] });
  });

  it('reports a pending invite to an address with no account as a watcher without a user id (API-026)', async () => {
    const { owner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'nobody-yet@partners.test');

    const list = await listPartners(owner.cookie);

    expect(list.watching).toEqual([]);
    expect(list.watchers).toEqual([
      {
        id: invite.id,
        user: { email: 'nobody-yet@partners.test' },
        status: 'pending',
        created_at: expect.any(Number),
      },
    ]);
  });

  it('lists a user as watching and as watched in the same response, each from their own side (API-026)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const third = await signupAndGetCookie('third@partners.test');
    await markUserEmailVerified(third.userId);
    const watchingOwner = await invitePartner(owner.cookie, 'par@partners.test');
    await acceptPartnerInvite(partner.cookie, watchingOwner.token);
    const watchedByThird = await invitePartner(partner.cookie, 'third@partners.test');
    await acceptPartnerInvite(third.cookie, watchedByThird.token);

    const middle = await listPartners(partner.cookie);

    expect(middle.watching.map((p) => p.id)).toEqual([watchingOwner.id]);
    expect(middle.watchers.map((p) => p.id)).toEqual([watchedByThird.id]);
    expect((await listPartners(owner.cookie)).watching).toEqual([]);
    expect((await listPartners(third.cookie)).watchers).toEqual([]);
  });
});

describe('DELETE /partner/:id (API-027)', () => {
  it('answers 404 for a UUID that names nothing (API-027)', async () => {
    const { owner } = await ownerAndPartner();

    const res = await removePartnership(owner.cookie, '00000000-0000-4000-8000-0000000000ee');

    expect(res.status).toBe(404);
  });

  it('answers 404 for an id that is not a UUID, since no such partnership exists (API-004, API-027)', async () => {
    const { owner } = await ownerAndPartner();

    const res = await removePartnership(owner.cookie, 'abc');

    expect(res.status).toBe(404);
  });

  it('answers 204 with an empty body, then 404 when asked again (API-027)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');
    await acceptPartnerInvite(partner.cookie, invite.token);

    const first = await removePartnership(owner.cookie, invite.id);
    const second = await removePartnership(owner.cookie, invite.id);

    expect(first.status).toBe(204);
    expect(await first.text()).toBe('');
    expect(second.status).toBe(404);
  });

  it('removes the partnership from both lists (API-027)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');
    await acceptPartnerInvite(partner.cookie, invite.token);

    await removePartnership(partner.cookie, invite.id);

    expect((await listPartners(owner.cookie)).watchers).toEqual([]);
    expect((await listPartners(partner.cookie)).watching).toEqual([]);
  });

  it.each([
    ['the owner', 'owner', 'par@partners.test'],
    ['the watcher', 'partner', 'own@partners.test'],
  ] as const)(
    'notifies only the other side when %s deletes an accepted partnership (API-027)',
    async (_name, who, notified) => {
      const parties = await ownerAndPartner();
      const invite = await invitePartner(parties.owner.cookie, 'par@partners.test');
      await acceptPartnerInvite(parties.partner.cookie, invite.token);

      await removePartnership(parties[who].cookie, invite.id);

      const removals = (await listEmailDeliveries()).filter((d) => d.kind === 'partner_removed');
      expect(removals.map((d) => d.recipient_email)).toEqual([notified]);
    },
  );

  it('sends no email, and voids the token, when a pending invite is withdrawn (API-027)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');

    expect((await removePartnership(owner.cookie, invite.id)).status).toBe(204);

    expect((await listEmailDeliveries()).filter((d) => d.kind === 'partner_removed')).toEqual([]);
    expect((await acceptPartnerInvite(partner.cookie, invite.token)).status).toBe(400);
  });

  it('lets the invited user decline a pending invite by deleting it, and the owner sees it gone (API-027)', async () => {
    const { owner, partner } = await ownerAndPartner();
    const invite = await invitePartner(owner.cookie, 'par@partners.test');

    expect((await removePartnership(partner.cookie, invite.id)).status).toBe(204);

    expect((await listPartners(owner.cookie)).watchers).toEqual([]);
  });
});
