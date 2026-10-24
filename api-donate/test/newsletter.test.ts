import { env, fetchMock, SELF } from 'cloudflare:test';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { BASE, clearDB } from './helpers';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
});

beforeEach(clearDB);

function post(body: Record<string, string>, headers: Record<string, string> = {}) {
  return SELF.fetch(`${BASE}/newsletter`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded', ...headers },
    body: new URLSearchParams(body),
    redirect: 'manual',
  });
}

type Captured = { body: Record<string, unknown> };

/** Intercepts the next Buttondown call. The result is filled in once the worker makes it. */
function interceptButtondown(statusCode = 201, data = '{}') {
  const captured: { value?: Captured } = {};
  fetchMock
    .get('https://api.buttondown.com')
    .intercept({ path: '/v1/subscribers', method: 'POST' })
    .reply((opts) => {
      captured.value = { body: JSON.parse(String(opts.body)) };
      return { statusCode, data };
    });
  return captured;
}

describe('POST /newsletter', () => {
  it('records the region, subscribes as unconfirmed and redirects', async () => {
    const captured = interceptButtondown();

    const res = await post({ email: ' Alice@Example.com ' }, { 'CF-Connecting-IP': '203.0.113.7' });

    expect(res.status).toBe(303);
    expect(res.headers.get('location')).toMatch(/\/newsletter\/check-email$/);
    const row = await env.DB.prepare('SELECT * FROM newsletter_subscribers').first();
    expect(row).toMatchObject({ email: 'alice@example.com', source: 'landing' });
    await vi.waitFor(() => expect(captured.value).toBeDefined());
    expect(captured.value?.body).toMatchObject({
      email_address: 'alice@example.com',
      type: 'unactivated',
      ip_address: '203.0.113.7',
    });
  });

  it('sends an invalid email back to the form without recording anything', async () => {
    const res = await post({ email: 'not-an-email' });
    expect(res.status).toBe(303);
    expect(res.headers.get('location')).toMatch(/\/#newsletter$/);
    expect(await env.DB.prepare('SELECT * FROM newsletter_subscribers').first()).toBeNull();
  });

  it('keeps the first location seen for an address', async () => {
    const captured = interceptButtondown();
    await env.DB.prepare(
      "INSERT INTO newsletter_subscribers (email, country, region_code, source) VALUES ('alice@example.com', 'US', 'CA', 'signup')",
    ).run();

    await post({ email: 'alice@example.com' });
    await vi.waitFor(() => expect(captured.value).toBeDefined());

    const row = await env.DB.prepare('SELECT * FROM newsletter_subscribers').first();
    expect(row).toMatchObject({ country: 'US', region_code: 'CA', source: 'signup' });
  });

  it('still redirects when Buttondown rejects the address', async () => {
    const captured = interceptButtondown(400, '{"code":"email_already_exists"}');
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});

    const res = await post({ email: 'alice@example.com' });

    expect(res.status).toBe(303);
    await vi.waitFor(() => expect(captured.value).toBeDefined());
    await vi.waitFor(() => expect(error).toHaveBeenCalled());
    error.mockRestore();
  });

  it('still subscribes and redirects when the database write fails', async () => {
    const captured = interceptButtondown();
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    await env.DB.prepare('DROP TABLE newsletter_subscribers').run();

    try {
      const res = await post({ email: 'alice@example.com' });
      expect(res.status).toBe(303);
      await vi.waitFor(() => expect(captured.value).toBeDefined());
    } finally {
      await env.DB.prepare(
        'CREATE TABLE newsletter_subscribers (email TEXT PRIMARY KEY, country TEXT, region_code TEXT, region TEXT, source TEXT NOT NULL, created_at INTEGER NOT NULL DEFAULT (unixepoch()))',
      ).run();
      error.mockRestore();
    }
  });
});
