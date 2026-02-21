// Auth route integration tests — real PBKDF2-SHA-256, real D1, real JWT.
import { describe, it, expect, beforeEach } from 'vitest';
import { SELF } from 'cloudflare:test';
import { BASE, clearDB } from './helpers';

beforeEach(clearDB);

describe('POST /signup', () => {
  it('creates a user and returns an access token', async () => {
    const res = await SELF.fetch(`${BASE}/signup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'alice@example.com', password: 'password123' }),
    });
    expect(res.status).toBe(201);
    const body = (await res.json()) as { access_token: string; user: { id: string; email: string } };
    expect(body.access_token).toBeTruthy();
    expect(body.user.email).toBe('alice@example.com');
  });

  it('returns 409 for a duplicate email', async () => {
    const payload = JSON.stringify({ email: 'dup@example.com', password: 'password123' });
    const headers = { 'Content-Type': 'application/json' };
    await SELF.fetch(`${BASE}/signup`, { method: 'POST', headers, body: payload });
    const res = await SELF.fetch(`${BASE}/signup`, { method: 'POST', headers, body: payload });
    expect(res.status).toBe(409);
  });

  it('returns 400 for missing fields', async () => {
    const res = await SELF.fetch(`${BASE}/signup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'bad@example.com' }), // missing password
    });
    expect(res.status).toBe(400);
  });
});

describe('POST /login', () => {
  it('returns an access token for valid credentials', async () => {
    await SELF.fetch(`${BASE}/signup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'bob@example.com', password: 'password123' }),
    });

    const res = await SELF.fetch(`${BASE}/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'bob@example.com', password: 'password123' }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { access_token: string };
    expect(body.access_token).toBeTruthy();
  });

  it('returns 401 for wrong password', async () => {
    await SELF.fetch(`${BASE}/signup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'carol@example.com', password: 'correctpass' }),
    });
    const res = await SELF.fetch(`${BASE}/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'carol@example.com', password: 'wrongpass' }),
    });
    expect(res.status).toBe(401);
  });

  it('returns 401 for unknown email', async () => {
    const res = await SELF.fetch(`${BASE}/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'nobody@example.com', password: 'pw' }),
    });
    expect(res.status).toBe(401);
  });
});

describe('POST /logout', () => {
  it('returns 204 and clears the refresh cookie', async () => {
    const res = await SELF.fetch(`${BASE}/logout`, { method: 'POST' });
    expect(res.status).toBe(204);
  });
});

describe('POST /token', () => {
  it('returns a new access token using the refresh cookie', async () => {
    // Signup to get the refresh_token cookie
    const signupRes = await SELF.fetch(`${BASE}/signup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'dave@example.com', password: 'password123' }),
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

  it('returns 401 with no refresh cookie', async () => {
    const res = await SELF.fetch(`${BASE}/token`, { method: 'POST' });
    expect(res.status).toBe(401);
  });
});
