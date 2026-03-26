import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  listEmailDeliveries,
  markUserEmailVerified,
  signupAndGetToken,
} from './helpers';

beforeEach(clearDB);

describe('Partner routes', () => {
  it('creates, accepts, lists, and deletes a partnership without shared-key fields', async () => {
    const { token: ownerToken, userId: ownerUserId } = await signupAndGetToken(
      'owner@example.com',
      'pw',
      'Owner',
    );
    const { token: partnerToken, userId: partnerUserId } = await signupAndGetToken(
      'partner@example.com',
      'pw',
      'Partner',
    );
    await markUserEmailVerified(ownerUserId);

    const createRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({ email: 'partner@example.com' }),
    });
    expect(createRes.status).toBe(201);
    const created = (await createRes.json()) as { id: string };

    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) => delivery.kind === 'partner_invite',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(200);

    const ownerListRes = await SELF.fetch(`${BASE}/partner`, {
      headers: { Authorization: `Bearer ${ownerToken}` },
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
      headers: { Authorization: `Bearer ${partnerToken}` },
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

    const deleteRes = await SELF.fetch(`${BASE}/partner/watching/${created.id}`, {
      method: 'DELETE',
      headers: { Authorization: `Bearer ${partnerToken}` },
    });
    expect(deleteRes.status).toBe(204);
  });

  it('supports inviting an email before the partner account exists', async () => {
    const { token: ownerToken, userId: ownerUserId } =
      await signupAndGetToken('owner2@example.com');
    await markUserEmailVerified(ownerUserId);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({ email: 'future@example.com' }),
    });
    expect(inviteRes.status).toBe(201);
    const created = (await inviteRes.json()) as { id: string };
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' && delivery.recipient_email === 'future@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    const { token: futureToken } = await signupAndGetToken('accepted@example.com');

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

    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(futureToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(200);

    const ownerPartnersRes = await SELF.fetch(`${BASE}/partner`, {
      headers: { Authorization: `Bearer ${ownerToken}` },
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

  it('prevents accepting your own invite link', async () => {
    const { token: ownerToken, userId: ownerUserId } =
      await signupAndGetToken('owner4@example.com');
    await markUserEmailVerified(ownerUserId);

    const createRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({
        email: 'someone-else@example.com',
      }),
    });
    expect(createRes.status).toBe(201);
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'someone-else@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(409);
    expect(await acceptRes.json()).toEqual({
      error: 'You cannot accept your own partner invite',
    });
  });
});
