import { fetchMock, SELF } from 'cloudflare:test';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { BASE, clearDB } from './helpers';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
});

beforeEach(clearDB);

afterEach(() => fetchMock.assertNoPendingInterceptors());

describe('POST /checkout', () => {
  it('creates a one-time Checkout session and returns its url', async () => {
    fetchMock
      .get('https://api.stripe.com')
      .intercept({ path: '/v1/checkout/sessions', method: 'POST' })
      .reply(200, {
        id: 'cs_test_123',
        url: 'https://checkout.stripe.com/c/pay/cs_test_123',
      });

    const res = await SELF.fetch(`${BASE}/checkout`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ amount: 25, recurring: false }),
    });

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ url: 'https://checkout.stripe.com/c/pay/cs_test_123' });
  });

  it('sends subscription mode and a monthly recurring price for recurring donations', async () => {
    let capturedBody = '';
    fetchMock
      .get('https://api.stripe.com')
      .intercept({ path: '/v1/checkout/sessions', method: 'POST' })
      .reply(200, (opts) => {
        capturedBody = String(opts.body);
        return { id: 'cs_test_456', url: 'https://checkout.stripe.com/c/pay/cs_test_456' };
      });

    const res = await SELF.fetch(`${BASE}/checkout`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ amount: 10, recurring: true }),
    });

    expect(res.status).toBe(200);
    const decoded = decodeURIComponent(capturedBody);
    expect(decoded).toContain('mode=subscription');
    expect(decoded).toContain('[recurring][interval]=month');
    // $10 -> 1000 cents
    expect(decoded).toContain('[unit_amount]=1000');
  });

  it('rejects a non-positive amount without calling Stripe', async () => {
    const res = await SELF.fetch(`${BASE}/checkout`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ amount: 0, recurring: false }),
    });

    expect(res.status).toBe(400);
  });
});
