import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { env, fetchMock } from 'cloudflare:test';
import {
  fillNewsletterRegion,
  recordNewsletterRegion,
  subscribeToNewsletter,
} from '../src/lib/newsletter';
import { Env } from '../src/types/bindings';

const BUTTONDOWN_ORIGIN = 'https://api.buttondown.com';

// The test worker has no BUTTONDOWN_API_KEY binding, so set one per call.
const envWithKey = (key: string | undefined) =>
  ({ ...env, BUTTONDOWN_API_KEY: key }) as unknown as Env;

beforeAll(() => {
  fetchMock.activate();
  fetchMock.disableNetConnect();
});

describe('subscribeToNewsletter', () => {
  it('creates an already-confirmed subscriber without overwriting an existing one', async () => {
    let captured: { headers: Record<string, string>; body: unknown } | undefined;
    fetchMock
      .get(BUTTONDOWN_ORIGIN)
      .intercept({ path: '/v1/subscribers', method: 'POST' })
      .reply((opts) => {
        captured = {
          headers: opts.headers as Record<string, string>,
          body: JSON.parse(String(opts.body)),
        };
        return { statusCode: 201, data: '{}' };
      });

    await subscribeToNewsletter(envWithKey('bd-key'), 'alice@example.com', '203.0.113.7');

    expect(captured?.headers.authorization).toBe('Token bd-key');
    // A collision header would let Buttondown resubscribe an address that opted out.
    expect(captured?.headers['x-buttondown-collision-behavior']).toBeUndefined();
    expect(captured?.body).toMatchObject({
      email_address: 'alice@example.com',
      type: 'regular',
      ip_address: '203.0.113.7',
    });
  });

  it('swallows Buttondown errors so they cannot fail a signup', async () => {
    fetchMock
      .get(BUTTONDOWN_ORIGIN)
      .intercept({ path: '/v1/subscribers', method: 'POST' })
      .reply(400, '{"code":"email_already_exists"}');
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});

    await expect(
      subscribeToNewsletter(envWithKey('bd-key'), 'alice@example.com'),
    ).resolves.toBeUndefined();
    expect(error).toHaveBeenCalledWith(expect.stringContaining('400'));
    error.mockRestore();
  });

  it('skips the request when no API key is configured', async () => {
    const log = vi.spyOn(console, 'log').mockImplementation(() => {});

    await subscribeToNewsletter(envWithKey(undefined), 'alice@example.com');

    expect(log).toHaveBeenCalledWith(expect.stringContaining('not set'));
    fetchMock.assertNoPendingInterceptors();
    log.mockRestore();
  });
});

describe('recordNewsletterRegion', () => {
  const cf = {
    country: 'US',
    regionCode: 'CA',
    region: 'California',
  } as Request['cf'];

  it('stores the signup region and keeps the first one seen', async () => {
    await env.DONATE_DB!.prepare('DELETE FROM newsletter_subscribers').run();

    await recordNewsletterRegion(env as unknown as Env, 'alice@example.com', cf);
    await recordNewsletterRegion(env as unknown as Env, 'alice@example.com', {
      country: 'US',
      regionCode: 'NY',
    } as Request['cf']);

    const rows = await env.DONATE_DB!.prepare('SELECT * FROM newsletter_subscribers').all();
    expect(rows.results).toMatchObject([
      {
        email: 'alice@example.com',
        country: 'US',
        region_code: 'CA',
        region: 'California',
        source: 'signup',
      },
    ]);
  });

  it('lets an account signup replace a row the landing form created', async () => {
    await env.DONATE_DB!.prepare('DELETE FROM newsletter_subscribers').run();
    await env
      .DONATE_DB!.prepare(
        "INSERT INTO newsletter_subscribers (email, country, region_code, source) VALUES ('alice@example.com', 'RU', '47', 'landing')",
      )
      .run();

    await recordNewsletterRegion(env as unknown as Env, 'alice@example.com', cf);

    const row = await env.DONATE_DB!.prepare('SELECT * FROM newsletter_subscribers').first();
    expect(row).toMatchObject({ country: 'US', region_code: 'CA', source: 'signup' });
  });

  it('stores nulls when Cloudflare gives no location', async () => {
    await env.DONATE_DB!.prepare('DELETE FROM newsletter_subscribers').run();
    await recordNewsletterRegion(env as unknown as Env, 'bob@example.com', undefined);
    const row = await env.DONATE_DB!.prepare('SELECT * FROM newsletter_subscribers').first();
    expect(row).toMatchObject({ email: 'bob@example.com', country: null, region_code: null });
  });
});

describe('fillNewsletterRegion', () => {
  const cf = { country: 'US', regionCode: 'TX', region: 'Texas' } as Request['cf'];
  const rows = () => env.DONATE_DB!.prepare('SELECT * FROM newsletter_subscribers').all();
  const insert = (
    email: string,
    source: string,
    country: string | null = null,
    code: string | null = null,
  ) =>
    env
      .DONATE_DB!.prepare(
        'INSERT INTO newsletter_subscribers (email, country, region_code, source) VALUES (?, ?, ?, ?)',
      )
      .bind(email, country, code, source)
      .run();

  beforeEach(async () => {
    await env.DONATE_DB!.prepare('DELETE FROM newsletter_subscribers').run();
  });

  it('fills the region of an account signup row that has none', async () => {
    await insert('a@example.com', 'signup');

    await fillNewsletterRegion(env as unknown as Env, 'a@example.com', cf);

    expect((await rows()).results).toMatchObject([
      { email: 'a@example.com', country: 'US', region_code: 'TX', region: 'Texas' },
    ]);
  });

  it('fills the region of a stored country only when the login is from that country', async () => {
    await insert('a@example.com', 'signup', 'US', null);
    await fillNewsletterRegion(env as unknown as Env, 'a@example.com', {
      country: 'CA',
      regionCode: 'ON',
      region: 'Ontario',
    } as Request['cf']);
    expect((await rows()).results).toMatchObject([
      { country: 'US', region_code: null, region: null },
    ]);

    await fillNewsletterRegion(env as unknown as Env, 'a@example.com', cf);
    expect((await rows()).results).toMatchObject([
      { country: 'US', region_code: 'TX', region: 'Texas' },
    ]);
  });

  it('never mixes a stored region code or name with another place', async () => {
    await insert('a@example.com', 'signup', 'US', 'WA');
    await insert('b@example.com', 'signup', 'US', null);
    await env
      .DONATE_DB!.prepare(
        "UPDATE newsletter_subscribers SET region = 'Washington' WHERE email = 'b@example.com'",
      )
      .run();

    await fillNewsletterRegion(env as unknown as Env, 'a@example.com', cf);
    await fillNewsletterRegion(env as unknown as Env, 'b@example.com', cf);

    expect((await rows()).results).toMatchObject([
      { email: 'a@example.com', country: 'US', region_code: 'WA', region: null },
      { email: 'b@example.com', country: 'US', region_code: null, region: 'Washington' },
    ]);
  });

  it('never overwrites a value that is set', async () => {
    await insert('a@example.com', 'signup', 'US', 'WA');
    await fillNewsletterRegion(env as unknown as Env, 'a@example.com', cf);
    expect((await rows()).results).toMatchObject([{ country: 'US', region_code: 'WA' }]);
  });

  it('does not write when nothing is missing', async () => {
    await insert('a@example.com', 'signup', 'US', 'WA');
    await env.DONATE_DB!.prepare("UPDATE newsletter_subscribers SET region = 'Washington'").run();
    const prepare = vi.spyOn(env.DONATE_DB!, 'prepare');
    await fillNewsletterRegion(env as unknown as Env, 'a@example.com', cf);
    expect(prepare.mock.calls.map(([sql]) => sql.trim().split(/\s/)[0])).toEqual(['SELECT']);
    prepare.mockRestore();
  });

  it('fills a country when Cloudflare gives no region code', async () => {
    await insert('a@example.com', 'signup');
    await fillNewsletterRegion(env as unknown as Env, 'a@example.com', {
      country: 'SG',
    } as Request['cf']);
    expect((await rows()).results).toMatchObject([{ country: 'SG', region_code: null }]);
  });

  it('never touches a row created by the landing form', async () => {
    await insert('a@example.com', 'landing');
    await fillNewsletterRegion(env as unknown as Env, 'a@example.com', cf);
    expect((await rows()).results).toMatchObject([{ country: null, region_code: null }]);
  });

  it('never creates a row for someone who did not subscribe', async () => {
    await fillNewsletterRegion(env as unknown as Env, 'nobody@example.com', cf);
    expect((await rows()).results).toEqual([]);
  });

  it('skips the update when Cloudflare gives no location', async () => {
    await insert('a@example.com', 'signup');
    await fillNewsletterRegion(env as unknown as Env, 'a@example.com', undefined);
    expect((await rows()).results).toMatchObject([{ region_code: null }]);
  });
});
