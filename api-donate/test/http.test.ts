import { env, fetchMock, SELF } from 'cloudflare:test';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import app from '../src/index';
import { BASE, checkoutCompletedEvent, signedWebhook } from './helpers';

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
});

afterEach(() => fetchMock.assertNoPendingInterceptors());

const post = (path: string, init: RequestInit = {}) =>
  SELF.fetch(`${BASE}${path}`, { method: 'POST', ...init });

describe('GET / (DONATE-005)', () => {
  it('returns the health check body', async () => {
    const res = await SELF.fetch(`${BASE}/`);
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      name: 'Virtue Initiative Donations API',
      version: '1.0.0',
      status: 'ok',
    });
  });

  it('needs no authentication and ignores an Authorization header', async () => {
    const res = await SELF.fetch(`${BASE}/`, { headers: { Authorization: 'Bearer nonsense' } });
    expect(res.status).toBe(200);
  });
});

describe('errors (DONATE-004)', () => {
  it.each([
    ['GET', '/nope'],
    ['POST', '/nope'],
    ['GET', '/goals/extra'],
    ['GET', '/checkout'],
    ['GET', '/webhook'],
    ['DELETE', '/goals'],
    ['PUT', '/checkout'],
  ])(
    'answers an unknown route or method %s %s with 404 and the error body',
    async (method, path) => {
      const res = await SELF.fetch(`${BASE}${path}`, { method });
      expect(res.status).toBe(404);
      expect(await res.json()).toEqual({ error: 'Not found' });
    },
  );

  it('answers an unhandled exception with 500 and the message in details', async () => {
    fetchMock
      .get('https://api.stripe.com')
      .intercept({ path: '/v1/checkout/sessions', method: 'POST' })
      .reply(400, {
        error: { type: 'invalid_request_error', message: 'Amount must be at least 50 cents' },
      });

    const res = await post('/checkout', {
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ amount: 0.001, recurring: false }),
    });

    expect(res.status).toBe(500);
    const body = (await res.json()) as { error: string; details: { message: unknown } };
    expect(body.error).toBe('Internal server error');
    expect(typeof body.details.message).toBe('string');
    expect(body.details.message).not.toBe('');
    expect(Object.keys(body).sort()).toEqual(['details', 'error']);
  });
});

describe('CORS (DONATE-003)', () => {
  const allowed = [
    ['the LANDING_URL origin', 'http://localhost:4321'],
    ['localhost on another port', 'http://localhost:5173'],
    ['localhost with no port', 'http://localhost'],
    ['a vanity .localhost host', 'https://myfeature.localhost'],
    ['a nested .localhost host with a port', 'https://app.myfeature.localhost:8443'],
  ];
  const disallowed = [
    ['an unrelated site', 'https://evil.example'],
    ['a host that only starts with localhost', 'http://localhost.evil.example'],
    ['a host that only ends in localhost', 'https://notlocalhost'],
    ['a lookalike of .localhost', 'https://foo.localhost.evil.example'],
    ['a malformed origin', 'not a url'],
    ['the null origin', 'null'],
  ];

  it.each(allowed)('echoes %s back on GET /goals', async (_name, origin) => {
    const res = await SELF.fetch(`${BASE}/goals`, { headers: { Origin: origin } });
    expect(res.status).toBe(200);
    expect(res.headers.get('access-control-allow-origin')).toBe(origin);
  });

  it.each(disallowed)('gives %s no CORS allow headers', async (_name, origin) => {
    const res = await SELF.fetch(`${BASE}/goals`, { headers: { Origin: origin } });
    expect(res.headers.get('access-control-allow-origin')).toBeNull();
    expect(res.headers.get('access-control-allow-methods')).toBeNull();
    expect(res.headers.get('access-control-allow-headers')).toBeNull();
  });

  it.each([
    ['GET', '/'],
    ['GET', '/goals'],
    ['GET', '/nope'],
    ['POST', '/checkout'],
  ])('applies CORS to %s %s', async (method, path) => {
    const origin = 'http://localhost:5173';
    const res = await SELF.fetch(`${BASE}${path}`, {
      method,
      headers: { Origin: origin, 'Content-Type': 'application/json' },
      // An invalid body, so POST /checkout fails validation and never calls Stripe.
      body: method === 'POST' ? '{}' : undefined,
    });
    expect(res.headers.get('access-control-allow-origin')).toBe(origin);
  });

  it('answers a preflight from an allowed origin with the allowed methods and headers', async () => {
    const origin = 'https://myfeature.localhost';
    const res = await SELF.fetch(`${BASE}/checkout`, {
      method: 'OPTIONS',
      headers: {
        Origin: origin,
        'Access-Control-Request-Method': 'POST',
        'Access-Control-Request-Headers': 'content-type',
      },
    });
    expect(res.status).toBeLessThan(300);
    expect(res.headers.get('access-control-allow-origin')).toBe(origin);
    const methods = (res.headers.get('access-control-allow-methods') ?? '')
      .split(',')
      .map((m) => m.trim());
    expect(methods.sort()).toEqual(['GET', 'OPTIONS', 'POST']);
    expect(res.headers.get('access-control-allow-headers')?.toLowerCase()).toBe('content-type');
  });

  const disallowedPreflight = () =>
    SELF.fetch(`${BASE}/checkout`, {
      method: 'OPTIONS',
      headers: {
        Origin: 'https://evil.example',
        'Access-Control-Request-Method': 'POST',
        'Access-Control-Request-Headers': 'content-type',
      },
    });

  it('does not allow the origin on a preflight from a disallowed origin', async () => {
    const res = await disallowedPreflight();
    expect(res.headers.get('access-control-allow-origin')).toBeNull();
  });

  it('does not CORS-gate POST /webhook, for any origin', async () => {
    for (const origin of ['https://evil.example', 'http://localhost:5173', undefined]) {
      const event = checkoutCompletedEvent({
        eventId: `evt_cors_${origin ?? 'none'}`,
        sessionId: `cs_cors_${origin ?? 'none'}`,
      });
      const { payload, signature } = await signedWebhook(event);
      const headers: Record<string, string> = { 'stripe-signature': signature };
      if (origin) headers.Origin = origin;
      const res = await post('/webhook', { headers, body: payload });
      expect(res.status, String(origin)).toBe(200);
      expect(res.headers.get('access-control-allow-origin')).toBeNull();
    }
  });

  it('adds no CORS headers to a preflight for /webhook', async () => {
    const res = await SELF.fetch(`${BASE}/webhook`, {
      method: 'OPTIONS',
      headers: { Origin: 'http://localhost:5173', 'Access-Control-Request-Method': 'POST' },
    });
    expect(res.headers.get('access-control-allow-origin')).toBeNull();
    expect(res.headers.get('access-control-allow-methods')).toBeNull();
  });
});

describe('CORS with a deployed LANDING_URL (DONATE-003)', () => {
  const deployed = { ...env, LANDING_URL: 'https://virtueinitiative.org' };
  const fetchApp = (origin: string, path = '/goals') =>
    app.fetch(new Request(`${BASE}${path}`, { headers: { Origin: origin } }), deployed);

  it.each([
    ['the LANDING_URL origin', 'https://virtueinitiative.org'],
    ['a localhost origin', 'http://localhost:5173'],
    ['a .localhost origin', 'https://dev.localhost'],
  ])('allows %s', async (_name, origin) => {
    const res = await fetchApp(origin);
    expect(res.headers.get('access-control-allow-origin')).toBe(origin);
  });

  it.each([
    ['the same host over http', 'http://virtueinitiative.org'],
    ['a subdomain of the landing site', 'https://www.virtueinitiative.org'],
    ['the same host on another port', 'https://virtueinitiative.org:8443'],
    ['a different site', 'https://example.com'],
    ['a lookalike of localhost', 'http://localhost.example.com'],
  ])('rejects %s', async (_name, origin) => {
    const res = await fetchApp(origin);
    expect(res.headers.get('access-control-allow-origin')).toBeNull();
  });

  it('compares against the origin of LANDING_URL even when it has a path', async () => {
    const res = await app.fetch(
      new Request(`${BASE}/goals`, { headers: { Origin: 'https://virtueinitiative.org' } }),
      { ...env, LANDING_URL: 'https://virtueinitiative.org/' },
    );
    expect(res.headers.get('access-control-allow-origin')).toBe('https://virtueinitiative.org');
  });
});
