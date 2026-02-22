// Log route integration tests. R2 is mocked (presigned URL generation).
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { SELF } from 'cloudflare:test';
import { BASE, clearDB, signupAndGetToken, authHeaders } from './helpers';

vi.mock('../src/lib/r2', () => ({
  putObject: vi.fn().mockResolvedValue(undefined),
  objectExists: vi.fn().mockResolvedValue(false),
}));

beforeEach(clearDB);

async function setupUserWithDevice(email: string) {
  const { token, userId } = await signupAndGetToken(email);
  const deviceRes = await SELF.fetch(`${BASE}/device`, {
    method: 'POST',
    headers: authHeaders(token),
    body: JSON.stringify({ name: 'Device', platform: 'linux', avg_interval_seconds: 300 }),
  });
  const { id: deviceId } = (await deviceRes.json()) as { id: string };
  return { token, userId, deviceId };
}

describe('POST /log', () => {
  it('creates a log entry and returns its id', async () => {
    const { token, deviceId } = await setupUserWithDevice('alice@example.com');
    const res = await SELF.fetch(`${BASE}/log`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ type: 'screenshot', device_id: deviceId }),
    });
    expect(res.status).toBe(201);
    const body = (await res.json()) as { id: string; created_at: string };
    expect(body.id).toBeTruthy();
    expect(body.created_at).toBeTruthy();
  });

  it('stores optional metadata', async () => {
    const { token, deviceId } = await setupUserWithDevice('bob@example.com');
    const res = await SELF.fetch(`${BASE}/log`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({
        type: 'app_open',
        device_id: deviceId,
        metadata: { app: 'Firefox', url: 'https://example.com' },
      }),
    });
    expect(res.status).toBe(201);
  });

  it('returns 400 for missing required fields', async () => {
    const { token } = await setupUserWithDevice('carol@example.com');
    const res = await SELF.fetch(`${BASE}/log`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ type: 'screenshot' }), // missing device_id
    });
    expect(res.status).toBe(400);
  });

  it('returns 401 without auth', async () => {
    const res = await SELF.fetch(`${BASE}/log`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    });
    expect(res.status).toBe(401);
  });
});

describe('GET /log', () => {
  it('returns an empty list for a new user', async () => {
    const { token } = await setupUserWithDevice('dave@example.com');
    const res = await SELF.fetch(`${BASE}/log`, { headers: { Authorization: `Bearer ${token}` } });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { items: unknown[] };
    expect(body.items).toEqual([]);
  });

  it('returns created logs', async () => {
    const { token, deviceId } = await setupUserWithDevice('eve@example.com');
    await SELF.fetch(`${BASE}/log`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ type: 'screenshot', device_id: deviceId }),
    });
    const res = await SELF.fetch(`${BASE}/log`, { headers: { Authorization: `Bearer ${token}` } });
    const body = (await res.json()) as { items: Array<{ type: string }> };
    expect(body.items).toHaveLength(1);
    expect(body.items[0].type).toBe('screenshot');
  });

  it('filters by type', async () => {
    const { token, deviceId } = await setupUserWithDevice('frank@example.com');
    await SELF.fetch(`${BASE}/log`, {
      method: 'POST', headers: authHeaders(token),
      body: JSON.stringify({ type: 'screenshot', device_id: deviceId }),
    });
    await SELF.fetch(`${BASE}/log`, {
      method: 'POST', headers: authHeaders(token),
      body: JSON.stringify({ type: 'app_open', device_id: deviceId }),
    });

    const res = await SELF.fetch(`${BASE}/log?type=screenshot`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    const body = (await res.json()) as { items: Array<{ type: string }> };
    expect(body.items.every((i) => i.type === 'screenshot')).toBe(true);
  });

  it('paginates with next_cursor', async () => {
    const { token, deviceId } = await setupUserWithDevice('grace@example.com');
    // Create 3 logs
    for (let i = 0; i < 3; i++) {
      await SELF.fetch(`${BASE}/log`, {
        method: 'POST', headers: authHeaders(token),
        body: JSON.stringify({ type: 'screenshot', device_id: deviceId }),
      });
    }
    // Fetch first page of 2
    const res = await SELF.fetch(`${BASE}/log?limit=2`, { headers: { Authorization: `Bearer ${token}` } });
    const body = (await res.json()) as { items: unknown[]; next_cursor?: string };
    expect(body.items).toHaveLength(2);
    expect(body.next_cursor).toBeTruthy();

    // Fetch second page
    const res2 = await SELF.fetch(`${BASE}/log?limit=2&cursor=${body.next_cursor}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    const body2 = (await res2.json()) as { items: unknown[]; next_cursor?: string };
    expect(body2.items).toHaveLength(1);
    expect(body2.next_cursor).toBeUndefined();
  });
});

