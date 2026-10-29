import { fetchMock, SELF } from 'cloudflare:test';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { BASE } from './helpers';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
});

afterEach(() => fetchMock.assertNoPendingInterceptors());

const SESSION_URL = 'https://checkout.stripe.com/c/pay/cs_test_edge';

function postCheckout(body: unknown, raw = false) {
  return SELF.fetch(`${BASE}/checkout`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: raw ? (body as string) : JSON.stringify(body),
  });
}

/** Answer the next Stripe session create call and capture its form body as params. */
function interceptStripe(
  reply: Record<string, unknown> = { id: 'cs_test_edge', url: SESSION_URL },
) {
  const captured: { params: URLSearchParams | null; calls: number } = { params: null, calls: 0 };
  fetchMock
    .get('https://api.stripe.com')
    .intercept({ path: '/v1/checkout/sessions', method: 'POST' })
    .reply(200, (opts) => {
      captured.calls += 1;
      captured.params = new URLSearchParams(String(opts.body));
      return reply;
    });
  return captured;
}

describe('POST /checkout validation (DONATE-007)', () => {
  // None of these may reach Stripe: with no interceptor registered, a call would
  // fail the request with a 500 instead of the 400 asserted here.
  it.each([
    ['zero amount', { amount: 0, recurring: false }],
    ['negative amount', { amount: -5, recurring: false }],
    ['amount above 100000', { amount: 100000.01, recurring: false }],
    ['amount far above the cap', { amount: 1e9, recurring: true }],
    ['string amount', { amount: '25', recurring: false }],
    ['null amount', { amount: null, recurring: false }],
    ['missing amount', { recurring: false }],
    ['missing recurring', { amount: 25 }],
    ['string recurring', { amount: 25, recurring: 'true' }],
    ['numeric recurring', { amount: 25, recurring: 1 }],
    ['null recurring', { amount: 25, recurring: null }],
    ['empty object', {}],
    ['array body', []],
    ['null body', null],
  ])('rejects %s with 400 and does not call Stripe', async (_name, body) => {
    const res = await postCheckout(body);
    expect(res.status).toBe(400);
  });

  // KNOWN-BUG: malformed JSON makes the validator throw an HTTPException(400), which app.onError turns into a 500
  it.fails('rejects malformed JSON with 400', async () => {
    expect((await postCheckout('{"amount": 25,', true)).status).toBe(400);
    expect((await postCheckout('', true)).status).toBe(400);
  });

  it('rejects a request without a JSON content type with 400', async () => {
    const res = await SELF.fetch(`${BASE}/checkout`, {
      method: 'POST',
      body: JSON.stringify({ amount: 25, recurring: false }),
    });
    expect(res.status).toBe(400);
  });

  it.each([
    [0.01, 1],
    [1, 100],
    [10.5, 1050],
    [19.99, 1999],
    [100000, 10000000],
  ])('accepts amount %s dollars as %s cents', async (amount, cents) => {
    const captured = interceptStripe();
    const res = await postCheckout({ amount, recurring: false });
    expect(res.status).toBe(200);
    expect(captured.params?.get('line_items[0][price_data][unit_amount]')).toBe(String(cents));
  });
});

describe('POST /checkout session contents (DONATE-008)', () => {
  it('creates one one-time line item in USD with the required checkout options', async () => {
    const captured = interceptStripe();
    const res = await postCheckout({ amount: 25, recurring: false });
    expect(res.status).toBe(200);
    expect(captured.calls).toBe(1);

    const p = captured.params!;
    expect(p.get('mode')).toBe('payment');
    expect(p.get('line_items[0][quantity]')).toBe('1');
    expect(p.get('line_items[0][price_data][currency]')).toBe('usd');
    expect(p.get('line_items[0][price_data][unit_amount]')).toBe('2500');
    expect(p.get('line_items[0][price_data][product_data][name]')).toBe(
      'Donation to The Virtue Initiative',
    );
    // A one-time payment has no recurring price and only one line item.
    expect(p.get('line_items[0][price_data][recurring][interval]')).toBeNull();
    expect([...p.keys()].filter((k) => k.startsWith('line_items[1]'))).toEqual([]);

    expect(p.get('submit_type')).toBe('donate');
    expect(p.get('name_collection[individual][enabled]')).toBe('true');
    expect(p.get('name_collection[individual][optional]')).toBe('false');
    expect(p.get('phone_number_collection[enabled]')).toBe('true');
    expect(p.get('billing_address_collection')).toBe('auto');
    expect(p.get('success_url')).toBe(
      'http://localhost:4321/donate/success?session_id={CHECKOUT_SESSION_ID}',
    );
    expect(p.get('cancel_url')).toBe('http://localhost:4321/donate');
  });

  it('creates a monthly subscription line item for a recurring donation', async () => {
    const captured = interceptStripe();
    const res = await postCheckout({ amount: 7.25, recurring: true });
    expect(res.status).toBe(200);

    const p = captured.params!;
    expect(p.get('mode')).toBe('subscription');
    expect(p.get('line_items[0][quantity]')).toBe('1');
    expect(p.get('line_items[0][price_data][currency]')).toBe('usd');
    expect(p.get('line_items[0][price_data][unit_amount]')).toBe('725');
    expect(p.get('line_items[0][price_data][recurring][interval]')).toBe('month');
    expect(p.get('submit_type')).toBe('donate');
    expect(p.get('success_url')).toBe(
      'http://localhost:4321/donate/success?session_id={CHECKOUT_SESSION_ID}',
    );
    expect(p.get('cancel_url')).toBe('http://localhost:4321/donate');
  });

  it('ignores extra fields in the request instead of passing them to Stripe', async () => {
    const captured = interceptStripe();
    const res = await postCheckout({
      amount: 5,
      recurring: false,
      currency: 'eur',
      success_url: 'https://evil.example',
      mode: 'subscription',
    });
    expect(res.status).toBe(200);
    const p = captured.params!;
    expect(p.get('mode')).toBe('payment');
    expect(p.get('line_items[0][price_data][currency]')).toBe('usd');
    expect(p.get('success_url')).toContain('http://localhost:4321/');
  });
});

describe('POST /checkout failures (DONATE-009)', () => {
  it.each([
    ['no url field', { id: 'cs_test_edge' }],
    ['a null url', { id: 'cs_test_edge', url: null }],
  ])('answers 502 when Stripe returns a session with %s', async (_name, reply) => {
    interceptStripe(reply);
    const res = await postCheckout({ amount: 25, recurring: false });
    expect(res.status).toBe(502);
    expect(await res.json()).toEqual({ error: 'Failed to create checkout session' });
  });

  it('falls through to the 500 error when the Stripe call fails', async () => {
    fetchMock
      .get('https://api.stripe.com')
      .intercept({ path: '/v1/checkout/sessions', method: 'POST' })
      .reply(401, { error: { type: 'invalid_request_error', message: 'Invalid API Key' } });
    const res = await postCheckout({ amount: 25, recurring: false });
    expect(res.status).toBe(500);
    const body = (await res.json()) as { error: string; details: { message: string } };
    expect(body.error).toBe('Internal server error');
    expect(typeof body.details.message).toBe('string');
  });
});
