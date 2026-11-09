import { beforeEach, describe, expect, it } from 'vitest';
import { createExecutionContext, env, SELF } from 'cloudflare:test';
import worker from '../src/index';
import { BASE, clearDB } from './helpers';

beforeEach(clearDB);

describe('API base path routing (API-007)', () => {
  it('serves the same health payload with and without the configured base path', async () => {
    const [rootRes, prefixedRes] = await Promise.all([
      SELF.fetch(`${BASE}/`),
      SELF.fetch(`${BASE}/api`),
    ]);

    expect(rootRes.status).toBe(200);
    expect(prefixedRes.status).toBe(200);
    const rootBody = await rootRes.json();
    expect(await prefixedRes.json()).toEqual(rootBody);
    expect(rootBody).toMatchObject({
      name: 'Virtue Initiative API',
      version: '1.0.0',
      status: 'ok',
    });
    expect(rootBody).toHaveProperty('commit');
  });
});

describe('API version prefix (API-005)', () => {
  it('routes requests prefixed with the current major version the same as unprefixed requests', async () => {
    const [unprefixed, apiPrefixed, versionOnly] = await Promise.all([
      SELF.fetch(`${BASE}/user/login-material`),
      SELF.fetch(`${BASE}/api/v0.1/user/login-material`),
      SELF.fetch(`${BASE}/v0.1/user/login-material`),
    ]);

    expect(unprefixed.status).toBe(200);
    expect(apiPrefixed.status).toBe(200);
    expect(versionOnly.status).toBe(200);

    const body = await unprefixed.json();
    expect(await apiPrefixed.json()).toEqual(body);
    expect(await versionOnly.json()).toEqual(body);
  });

  it('routes /api/v0.1 and /v0.1 to the health check, same as /api and /', async () => {
    const [apiPrefixed, versionOnly] = await Promise.all([
      SELF.fetch(`${BASE}/api/v0.1`),
      SELF.fetch(`${BASE}/v0.1`),
    ]);

    expect(apiPrefixed.status).toBe(200);
    expect(versionOnly.status).toBe(200);
    expect(await apiPrefixed.json()).toMatchObject({ name: 'Virtue Initiative API' });
    expect(await versionOnly.json()).toMatchObject({ name: 'Virtue Initiative API' });
  });

  it.each(['v0.2', 'v1', 'v2'])(
    'responds 410 Gone for a no-longer-supported version prefix (%s)',
    async (version) => {
      const [apiPrefixed, versionOnly] = await Promise.all([
        SELF.fetch(`${BASE}/api/${version}/user/login-material`),
        SELF.fetch(`${BASE}/${version}/user/login-material`),
      ]);

      expect(apiPrefixed.status).toBe(410);
      expect(versionOnly.status).toBe(410);
      expect(await apiPrefixed.json()).toMatchObject({ error: expect.any(String) });
    },
  );

  it('responds 410 Gone for an unsupported version before authentication is checked', async () => {
    // No refresh token is provided; an unsupported version must still short-circuit to 410
    // rather than falling through to a 401 from the auth middleware.
    const res = await SELF.fetch(`${BASE}/api/v1/user`);

    expect(res.status).toBe(410);
  });
});

describe('GET /dev/emails (API-066)', () => {
  it('lists every template and serves a selected body alone', async () => {
    const page = await SELF.fetch(`${BASE}/dev/emails`);
    expect(page.status).toBe(200);
    const pageHtml = await page.text();
    expect(pageHtml).toContain('Daily summary for');
    expect(pageHtml).toContain('Reset your');

    const html = await SELF.fetch(`${BASE}/dev/emails?t=password-reset&raw=html`);
    expect(html.status).toBe(200);
    expect(await html.text()).toContain('<h1');

    const text = await SELF.fetch(`${BASE}/dev/emails?t=password-reset&raw=text`);
    expect(await text.text()).toContain('A password reset was requested');
  });

  it('answers 404 unless email is only logged', async () => {
    const res = await worker.fetch(
      new Request(`${BASE}/dev/emails`),
      { ...env, EMAIL_DELIVERY_MODE: 'ses' },
      createExecutionContext(),
    );
    expect(res.status).toBe(404);
  });
});
