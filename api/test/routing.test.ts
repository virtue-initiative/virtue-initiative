import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import { BASE, clearDB } from './helpers';

beforeEach(clearDB);

describe('API base path routing', () => {
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
