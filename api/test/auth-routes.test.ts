import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import { authHeaders, BASE, clearDB, signupAndGetToken } from './helpers';

beforeEach(clearDB);

describe('Auth routes', () => {
  it('creates a user and returns session credentials on signup', async () => {
    const res = await SELF.fetch(`${BASE}/signup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email: 'alice@example.com',
        password: 'client-side-hash',
        name: 'Alice',
      }),
    });

    expect(res.status).toBe(201);
    expect(res.headers.get('set-cookie')).toContain('refresh_token=');

    const body = (await res.json()) as {
      access_token: string;
      user: { id: string; email: string; name: string };
    };

    expect(body.access_token).toBeTruthy();
    expect(body.user.email).toBe('alice@example.com');
    expect(body.user.name).toBe('Alice');
  });

  it('refreshes an access token from the refresh cookie', async () => {
    const signupRes = await SELF.fetch(`${BASE}/signup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'bob@example.com', password: 'pw' }),
    });

    const cookie = signupRes.headers.get('set-cookie') ?? '';
    const res = await SELF.fetch(`${BASE}/token`, {
      method: 'POST',
      headers: { Cookie: cookie },
    });

    expect(res.status).toBe(201);
    const body = (await res.json()) as { access_token: string };
    expect(body.access_token).toBeTruthy();
  });

  it('returns the current user and allows updating profile fields', async () => {
    const { token } = await signupAndGetToken('carol@example.com', 'pw', 'Carol');

    const patchRes = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(token),
      body: JSON.stringify({
        name: 'Updated Carol',
        e2ee_key: Buffer.from('secret').toString('base64'),
        pub_key: Buffer.from('public-key').toString('base64'),
        priv_key: Buffer.from('private-key').toString('base64'),
      }),
    });
    expect(patchRes.status).toBe(200);

    const getRes = await SELF.fetch(`${BASE}/user`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(getRes.status).toBe(200);

    const body = (await getRes.json()) as {
      name: string;
      e2ee_key: string;
      pub_key: string;
      priv_key: string;
    };
    expect(body.name).toBe('Updated Carol');
    expect(Buffer.from(body.e2ee_key, 'base64').toString()).toBe('secret');
    expect(Buffer.from(body.pub_key, 'base64').toString()).toBe('public-key');
    expect(Buffer.from(body.priv_key, 'base64').toString()).toBe('private-key');
  });
});
