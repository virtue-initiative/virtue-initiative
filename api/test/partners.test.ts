import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import { authHeaders, BASE, clearDB, signupAndGetToken } from './helpers';

beforeEach(clearDB);

describe('Partner routes', () => {
  it('creates, accepts, lists, updates, and deletes a partnership', async () => {
    const { token: ownerToken } = await signupAndGetToken('owner@example.com', 'pw', 'Owner');
    const { token: partnerToken, userId: partnerUserId } = await signupAndGetToken(
      'partner@example.com',
      'pw',
      'Partner',
    );

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

    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ id: created.id }),
    });
    expect(acceptRes.status).toBe(200);

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
    expect(Buffer.from(owned?.e2ee_key ?? '', 'base64').toString()).toBe('wrapped-key');

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
    const { token: ownerToken } = await signupAndGetToken('owner2@example.com');

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({ email: 'future@example.com', permissions: { view_data: true } }),
    });
    expect(inviteRes.status).toBe(201);
    const created = (await inviteRes.json()) as { id: string };

    const { token: futureToken } = await signupAndGetToken('future@example.com');

    const incomingRes = await SELF.fetch(`${BASE}/partner`, {
      headers: { Authorization: `Bearer ${futureToken}` },
    });
    expect(incomingRes.status).toBe(200);
    const incoming = (await incomingRes.json()) as Array<{
      id: string;
      role: 'owner' | 'invitee';
      partner: { email: string };
      status: 'pending' | 'accepted';
    }>;
    expect(incoming).toHaveLength(1);
    expect(incoming[0]?.id).toBe(created.id);
    expect(incoming[0]?.role).toBe('invitee');
    expect(incoming[0]?.status).toBe('pending');
    expect(incoming[0]?.partner.email).toBe('owner2@example.com');

    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(futureToken),
      body: JSON.stringify({ id: created.id }),
    });
    expect(acceptRes.status).toBe(200);
  });

  it('returns stored public keys and allows owners to confirm a partner later', async () => {
    const { token: ownerToken } = await signupAndGetToken('owner3@example.com');
    const { token: partnerToken, userId: partnerUserId } =
      await signupAndGetToken('partner3@example.com');

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
});
