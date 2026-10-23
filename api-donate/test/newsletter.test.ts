import { env, fetchMock, SELF } from 'cloudflare:test';
import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { BASE, clearDB } from './helpers';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
});

beforeEach(clearDB);

function post(body: Record<string, string>) {
  return SELF.fetch(`${BASE}/newsletter`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams(body),
    redirect: 'manual',
  });
}

describe('POST /newsletter', () => {
  it('records the region, subscribes as unconfirmed and redirects', async () => {
    let captured: { headers: Record<string, string>; body: unknown } | undefined;
    fetchMock
      .get('https://api.buttondown.com')
      .intercept({ path: '/v1/subscribers', method: 'POST' })
      .reply((opts) => {
        captured = {
          headers: opts.headers as Record<string, string>,
          body: JSON.parse(String(opts.body)),
        };
        return { statusCode: 201, data: '{}' };
      });

    const res = await SELF.fetch(`${BASE}/newsletter`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/x-www-form-urlencoded',
        'CF-Connecting-IP': '203.0.113.7',
      },
      body: new URLSearchParams({ email: ' Alice@Example.com ' }),
      redirect: 'manual',
    });

    expect(res.status).toBe(303);
    expect(res.headers.get('location')).toMatch(/\/newsletter\/check-email$/);
    const row = await env.DB.prepare('SELECT * FROM newsletter_subscribers').first();
    expect(row).toMatchObject({ email: 'alice@example.com', source: 'landing' });
    await new Promise((r) => setTimeout(r, 50));
    expect(captured?.body).toMatchObject({
      email_address: 'alice@example.com',
      type: 'unactivated',
      ip_address: '203.0.113.7',
    });
  });

  it('rejects an invalid email', async () => {
    const res = await post({ email: 'not-an-email' });
    expect(res.status).toBe(400);
    const row = await env.DB.prepare('SELECT * FROM newsletter_subscribers').first();
    expect(row).toBeNull();
  });
});
