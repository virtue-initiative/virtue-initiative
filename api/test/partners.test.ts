// Partner route integration tests.
import { describe, it, expect, beforeEach } from 'vitest';
import { SELF } from 'cloudflare:test';
import { BASE, clearDB, signupAndGetToken, authHeaders } from './helpers';

beforeEach(clearDB);

describe('POST /partner', () => {
  it('creates a pending partner invite', async () => {
    const { token } = await signupAndGetToken('alice@example.com');
    await signupAndGetToken('bob@example.com'); // create target user

    const res = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({
        email: 'bob@example.com',
        permissions: { view_data: true },
      }),
    });
    expect(res.status).toBe(201);
    const body = (await res.json()) as { id: string; status: string };
    expect(body.id).toBeTruthy();
    expect(body.status).toBe('pending');
  });

  it('returns 404 when target user does not exist', async () => {
    const { token } = await signupAndGetToken('carol@example.com');
    const res = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ email: 'nobody@example.com', permissions: {} }),
    });
    expect(res.status).toBe(404);
  });

  it('returns 409 for a duplicate partnership', async () => {
    const { token } = await signupAndGetToken('dave@example.com');
    await signupAndGetToken('eve@example.com');
    const payload = JSON.stringify({ email: 'eve@example.com', permissions: {} });
    await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(token),
      body: payload,
    });
    const res = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(token),
      body: payload,
    });
    expect(res.status).toBe(409);
  });
});

describe('POST /partner/accept', () => {
  it('accepts a pending invite', async () => {
    const { token: aliceToken } = await signupAndGetToken('alice2@example.com');
    const { token: bobToken } = await signupAndGetToken('bob2@example.com');

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(aliceToken),
      body: JSON.stringify({ email: 'bob2@example.com', permissions: { view_data: false } }),
    });
    const { id: inviteId } = (await inviteRes.json()) as { id: string };

    const acceptRes = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(bobToken),
      body: JSON.stringify({ id: inviteId }),
    });
    expect(acceptRes.status).toBe(200);
  });

  it('returns 404 when invite does not exist or is not for this user', async () => {
    const { token } = await signupAndGetToken('frank@example.com');
    const res = await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ id: 'bad-id' }),
    });
    expect(res.status).toBe(404);
  });
});

describe('GET /partner', () => {
  it('returns partnerships as owner and as partner', async () => {
    const { token: aliceToken } = await signupAndGetToken('alice3@example.com');
    const { token: bobToken } = await signupAndGetToken('bob3@example.com');

    await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(aliceToken),
      body: JSON.stringify({ email: 'bob3@example.com', permissions: {} }),
    });

    const aliceList = (await (
      await SELF.fetch(`${BASE}/partner`, {
        headers: { Authorization: `Bearer ${aliceToken}` },
      })
    ).json()) as Array<{ role: string }>;
    expect(aliceList.some((p) => p.role === 'owner')).toBe(true);

    const bobList = (await (
      await SELF.fetch(`${BASE}/partner`, {
        headers: { Authorization: `Bearer ${bobToken}` },
      })
    ).json()) as Array<{ role: string }>;
    expect(bobList.some((p) => p.role === 'partner')).toBe(true);
  });
});

describe('PATCH /partner/:id', () => {
  it('updates permissions', async () => {
    const { token: aliceToken } = await signupAndGetToken('alice4@example.com');
    await signupAndGetToken('bob4@example.com');

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(aliceToken),
      body: JSON.stringify({ email: 'bob4@example.com', permissions: { view_data: false } }),
    });
    const { id } = (await inviteRes.json()) as { id: string };

    const patchRes = await SELF.fetch(`${BASE}/partner/${id}`, {
      method: 'PATCH',
      headers: authHeaders(aliceToken),
      body: JSON.stringify({ permissions: { view_data: true } }),
    });
    expect(patchRes.status).toBe(200);
    const body = (await patchRes.json()) as { permissions: { view_data: boolean } };
    expect(body.permissions.view_data).toBe(true);
  });
});

describe('DELETE /partner/:id', () => {
  it('removes the partnership', async () => {
    const { token: aliceToken } = await signupAndGetToken('alice5@example.com');
    await signupAndGetToken('bob5@example.com');

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(aliceToken),
      body: JSON.stringify({ email: 'bob5@example.com', permissions: {} }),
    });
    const { id } = (await inviteRes.json()) as { id: string };

    const delRes = await SELF.fetch(`${BASE}/partner/${id}`, {
      method: 'DELETE',
      headers: { Authorization: `Bearer ${aliceToken}` },
    });
    expect(delRes.status).toBe(204);
  });

  it('returns 404 for a non-existent partnership', async () => {
    const { token } = await signupAndGetToken('ghost@example.com');
    const res = await SELF.fetch(`${BASE}/partner/bad-id`, {
      method: 'DELETE',
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.status).toBe(404);
  });
});
