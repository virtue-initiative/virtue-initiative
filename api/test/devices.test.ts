// Device route integration tests.
import { describe, it, expect, beforeEach } from 'vitest';
import { SELF } from 'cloudflare:test';
import { BASE, clearDB, signupAndGetToken, authHeaders } from './helpers';

beforeEach(clearDB);

describe('POST /device', () => {
  it('registers a device and returns its id', async () => {
    const { token } = await signupAndGetToken('alice@example.com');
    const res = await SELF.fetch(`${BASE}/device`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ name: 'MacBook Pro', platform: 'macos', avg_interval_seconds: 300 }),
    });
    expect(res.status).toBe(201);
    const body = (await res.json()) as { id: string };
    expect(body.id).toBeTruthy();
  });

  it('returns 401 without a token', async () => {
    const res = await SELF.fetch(`${BASE}/device`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'PC', platform: 'windows', avg_interval_seconds: 300 }),
    });
    expect(res.status).toBe(401);
  });

  it('returns 400 for missing required fields', async () => {
    const { token } = await signupAndGetToken('bob@example.com');
    const res = await SELF.fetch(`${BASE}/device`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ name: 'PC' }), // missing platform
    });
    expect(res.status).toBe(400);
  });
});

describe('GET /device', () => {
  it('returns an empty list for a new user', async () => {
    const { token } = await signupAndGetToken('carol@example.com');
    const res = await SELF.fetch(`${BASE}/device`, { headers: { Authorization: `Bearer ${token}` } });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual([]);
  });

  it('returns registered devices with status', async () => {
    const { token } = await signupAndGetToken('dave@example.com');
    await SELF.fetch(`${BASE}/device`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ name: 'iPhone', platform: 'ios', avg_interval_seconds: 60 }),
    });
    const res = await SELF.fetch(`${BASE}/device`, { headers: { Authorization: `Bearer ${token}` } });
    const body = (await res.json()) as Array<{ name: string; platform: string; status: string }>;
    expect(body).toHaveLength(1);
    expect(body[0].name).toBe('iPhone');
    expect(body[0].platform).toBe('ios');
    expect(['online', 'offline']).toContain(body[0].status);
  });

  it('returns 401 without a token', async () => {
    const res = await SELF.fetch(`${BASE}/device`);
    expect(res.status).toBe(401);
  });
});

describe('PATCH /device/:id', () => {
  it('updates device name and enabled flag', async () => {
    const { token } = await signupAndGetToken('eve@example.com');
    const createRes = await SELF.fetch(`${BASE}/device`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ name: 'Old Name', platform: 'linux', avg_interval_seconds: 300 }),
    });
    const { id } = (await createRes.json()) as { id: string };

    const patchRes = await SELF.fetch(`${BASE}/device/${id}`, {
      method: 'PATCH',
      headers: authHeaders(token),
      body: JSON.stringify({ name: 'New Name', enabled: false }),
    });
    expect(patchRes.status).toBe(200);
    const body = (await patchRes.json()) as { updated: boolean };
    expect(body.updated).toBe(true);

    // Verify the change persisted
    const listRes = await SELF.fetch(`${BASE}/device`, { headers: { Authorization: `Bearer ${token}` } });
    const devices = (await listRes.json()) as Array<{ id: string; name: string; enabled: boolean }>;
    const updated = devices.find((d) => d.id === id);
    expect(updated?.name).toBe('New Name');
    expect(updated?.enabled).toBe(false);
  });

  it('returns 404 for an unknown device id', async () => {
    const { token } = await signupAndGetToken('frank@example.com');
    const res = await SELF.fetch(`${BASE}/device/does-not-exist`, {
      method: 'PATCH',
      headers: authHeaders(token),
      body: JSON.stringify({ name: 'Ghost' }),
    });
    expect(res.status).toBe(404);
  });
});

