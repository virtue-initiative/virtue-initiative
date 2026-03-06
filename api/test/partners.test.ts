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
      partner: { id?: string; email: string; name?: string };
      permissions: { view_data?: boolean };
      e2ee_key?: string;
    }>;
    expect(list[0].partner).toEqual({
      id: partnerUserId,
      email: 'partner@example.com',
      name: 'Partner',
    });
    expect(Buffer.from(list[0].e2ee_key ?? '', 'base64').toString()).toBe('wrapped-key');

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
    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(futureToken),
      body: JSON.stringify({ id: created.id }),
    });
    expect(acceptRes.status).toBe(200);
  });
});
