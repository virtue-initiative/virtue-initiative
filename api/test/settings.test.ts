// Settings route integration tests.
import { describe, it, expect, beforeEach } from 'vitest';
import { SELF } from 'cloudflare:test';
import { BASE, clearDB, signupAndGetToken, authHeaders } from './helpers';

beforeEach(clearDB);

describe('GET /settings', () => {
  it('returns defaults when no settings have been saved', async () => {
    const { token } = await signupAndGetToken('alice@example.com');
    const res = await SELF.fetch(`${BASE}/settings`, { headers: { Authorization: `Bearer ${token}` } });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { timezone: string; retention_days: number };
    expect(body.timezone).toBe('UTC');
    expect(body.retention_days).toBe(30);
  });

  it('returns 401 without auth', async () => {
    const res = await SELF.fetch(`${BASE}/settings`);
    expect(res.status).toBe(401);
  });
});

describe('POST /settings', () => {
  it('saves settings and returns the merged object', async () => {
    const { token } = await signupAndGetToken('bob@example.com');
    const res = await SELF.fetch(`${BASE}/settings`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ timezone: 'America/New_York', retention_days: 60 }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { timezone: string; retention_days: number };
    expect(body.timezone).toBe('America/New_York');
    expect(body.retention_days).toBe(60);
  });

  it('merges partial updates with existing settings', async () => {
    const { token } = await signupAndGetToken('carol@example.com');
    // First write
    await SELF.fetch(`${BASE}/settings`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ timezone: 'Europe/London', retention_days: 90 }),
    });
    // Partial update — only change retention_days
    const res = await SELF.fetch(`${BASE}/settings`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ retention_days: 14 }),
    });
    const body = (await res.json()) as { timezone: string; retention_days: number };
    expect(body.timezone).toBe('Europe/London'); // preserved
    expect(body.retention_days).toBe(14);         // updated
  });

  it('persists settings so GET returns the saved values', async () => {
    const { token } = await signupAndGetToken('dave@example.com');
    await SELF.fetch(`${BASE}/settings`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ timezone: 'Asia/Tokyo' }),
    });
    const res = await SELF.fetch(`${BASE}/settings`, { headers: { Authorization: `Bearer ${token}` } });
    const body = (await res.json()) as { timezone: string };
    expect(body.timezone).toBe('Asia/Tokyo');
  });
});

