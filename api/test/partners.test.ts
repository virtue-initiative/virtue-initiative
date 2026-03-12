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
  it('creates, accepts, lists, updates, and deletes a partnership', async () => {
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
      body: JSON.stringify({
        email: 'partner@example.com',
        permissions: { view_data: true },
        e2ee_key: Buffer.from('wrapped-key').toString('base64'),
      }),
    });
    expect(createRes.status).toBe(201);
    const created = (await createRes.json()) as { id: string };
    const afterInviteDeliveries = await listEmailDeliveries();
    const inviteDelivery = afterInviteDeliveries.find(
      (delivery) => delivery.kind === 'partner_invite',
    );
    expect(inviteDelivery).toBeTruthy();
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };
    expect(inviteDelivery?.text).toContain('partner_invite_token=');

    const acceptRes = await SELF.fetch(`${BASE}/partner/invite/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(200);
    const deliveries = await listEmailDeliveries();
    const acceptedDelivery = deliveries.find((delivery) => delivery.kind === 'partner_accepted');
    expect(acceptedDelivery).toBeTruthy();
    expect(acceptedDelivery?.text).toContain('http://localhost:5173');

    const listRes = await SELF.fetch(`${BASE}/partner`, {
      headers: { Authorization: `Bearer ${ownerToken}` },
    });
    const list = (await listRes.json()) as Array<{
      id: string;
      role: 'owner' | 'invitee';
      partner: { id?: string; email: string; name?: string };
      permissions: { view_data?: boolean };
      e2ee_key?: string;
    }>;
    const owned = list.find((partner) => partner.role === 'owner');
    expect(owned?.partner).toEqual({
      id: partnerUserId,
      email: 'partner@example.com',
      name: 'Partner',
    });
    expect(owned?.e2ee_key).toBeUndefined();

    const patchRes = await SELF.fetch(`${BASE}/partner/${created.id}`, {
      method: 'PATCH',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({ permissions: { view_data: false } }),
    });
    expect(patchRes.status).toBe(200);
    expect(await patchRes.json()).toEqual({
      id: created.id,
      permissions: { view_data: false },
    });

    const deleteRes = await SELF.fetch(`${BASE}/partner/${created.id}`, {
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
      body: JSON.stringify({ email: 'future@example.com', permissions: { view_data: true } }),
    });
    expect(inviteRes.status).toBe(201);
    const created = (await inviteRes.json()) as { id: string };
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' && delivery.recipient_email === 'future@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    const { token: futureToken } = await signupAndGetToken('accepted@example.com');

    const validateRes = await SELF.fetch(`${BASE}/partner/invite/validate`, {
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

    const acceptRes = await SELF.fetch(`${BASE}/partner/invite/accept`, {
      method: 'POST',
      headers: authHeaders(futureToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    expect(acceptRes.status).toBe(200);

    const ownerPartnersRes = await SELF.fetch(`${BASE}/partner`, {
      headers: { Authorization: `Bearer ${ownerToken}` },
    });
    const ownerPartners = (await ownerPartnersRes.json()) as Array<{
      id: string;
      role: 'owner' | 'invitee';
      partner: { email: string };
      status: 'pending' | 'accepted';
    }>;
    const owned = ownerPartners.find((partner) => partner.id === created.id);
    expect(owned?.partner.email).toBe('accepted@example.com');
    expect(owned?.status).toBe('accepted');
  });

  it('returns stored public keys and allows owners to confirm a partner later', async () => {
    const { token: ownerToken, userId: ownerUserId } =
      await signupAndGetToken('owner3@example.com');
    const { token: partnerToken, userId: partnerUserId } =
      await signupAndGetToken('partner3@example.com');
    await markUserEmailVerified(ownerUserId);

    await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({
        pub_key: Buffer.from('partner-public-key').toString('base64'),
      }),
    });

    const createRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({
        email: 'partner3@example.com',
        permissions: { view_data: true },
      }),
    });
    expect(createRes.status).toBe(201);
    const created = (await createRes.json()) as { id: string };
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' && delivery.recipient_email === 'partner3@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/invite/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const pubKeyRes = await SELF.fetch(`${BASE}/pubkey?email=partner3@example.com`);
    expect(pubKeyRes.status).toBe(200);
    expect(await pubKeyRes.json()).toEqual({
      pubkey: Buffer.from('partner-public-key').toString('base64'),
    });

    const patchRes = await SELF.fetch(`${BASE}/partner/${created.id}`, {
      method: 'PATCH',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({
        e2ee_key: Buffer.from('encrypted-owner-key').toString('base64'),
      }),
    });
    expect(patchRes.status).toBe(200);

    const listRes = await SELF.fetch(`${BASE}/partner`, {
      headers: { Authorization: `Bearer ${ownerToken}` },
    });
    const list = (await listRes.json()) as Array<{
      id: string;
      role: 'owner' | 'invitee';
      partner: { id?: string; email: string };
      e2ee_key?: string;
    }>;
    const owned = list.find((partner) => partner.id === created.id);
    expect(owned?.role).toBe('owner');
    expect(owned?.partner.id).toBe(partnerUserId);
    expect(owned?.partner.email).toBe('partner3@example.com');
    expect(Buffer.from(owned?.e2ee_key ?? '', 'base64').toString()).toBe('encrypted-owner-key');
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
        permissions: { view_data: true },
      }),
    });
    expect(createRes.status).toBe(201);
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'someone-else@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    const acceptRes = await SELF.fetch(`${BASE}/partner/invite/accept`, {
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
