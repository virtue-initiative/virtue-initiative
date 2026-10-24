import { beforeEach, describe, expect, it, vi } from 'vitest';
import { env } from 'cloudflare:test';
import { fillNewsletterRegion, subscribeToNewsletter } from '../src/lib/newsletter';
import { Env } from '../src/types/bindings';

const testEnv = env as unknown as Env;
const reset = () => env.DONATE_DB!.prepare('DELETE FROM newsletter_subscribers').run();
const row = () => env.DONATE_DB!.prepare('SELECT * FROM newsletter_subscribers').first();

describe('subscribeToNewsletter', () => {
  const cf = {
    country: 'US',
    regionCode: 'CA',
    region: 'California',
  } as Request['cf'];

  beforeEach(reset);

  it('stores a confirmed subscriber with the signup region and keeps the first one seen', async () => {
    await subscribeToNewsletter(testEnv, 'alice@example.com', cf);
    await subscribeToNewsletter(testEnv, 'alice@example.com', {
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
        unsubscribed_at: null,
        suppressed_at: null,
      },
    ]);
    expect(rows.results[0]!.confirmed_at).toBeTypeOf('number');
  });

  it('lets an account signup replace a row the landing form created, and confirms it', async () => {
    await env
      .DONATE_DB!.prepare(
        "INSERT INTO newsletter_subscribers (email, country, region_code, source) VALUES ('alice@example.com', 'RU', '47', 'landing')",
      )
      .run();

    await subscribeToNewsletter(testEnv, 'alice@example.com', cf);

    const r = await row();
    expect(r).toMatchObject({ country: 'US', region_code: 'CA', source: 'signup' });
    expect(r!.confirmed_at).toBeTypeOf('number');
  });

  it('resubscribes an address that unsubscribed, because ticking the box is a new opt-in', async () => {
    await env
      .DONATE_DB!.prepare(
        "INSERT INTO newsletter_subscribers (email, source, confirmed_at, unsubscribed_at) VALUES ('a@example.com', 'signup', 100, 200)",
      )
      .run();

    await subscribeToNewsletter(testEnv, 'a@example.com', cf);

    const r = await row();
    expect(r).toMatchObject({ unsubscribed_at: null });
    expect(r!.confirmed_at).toBeGreaterThan(200);
  });

  it('keeps the original confirmation time of an address that is already subscribed', async () => {
    await env
      .DONATE_DB!.prepare(
        "INSERT INTO newsletter_subscribers (email, source, confirmed_at) VALUES ('a@example.com', 'signup', 100)",
      )
      .run();
    await subscribeToNewsletter(testEnv, 'a@example.com', cf);
    expect(await row()).toMatchObject({ confirmed_at: 100 });
  });

  it('never resubscribes an address that hard-bounced or complained', async () => {
    await env
      .DONATE_DB!.prepare(
        "INSERT INTO newsletter_subscribers (email, source, confirmed_at, unsubscribed_at, suppressed_at) VALUES ('a@example.com', 'landing', 100, 200, 300)",
      )
      .run();

    await subscribeToNewsletter(testEnv, 'a@example.com', cf);

    expect(await row()).toMatchObject({
      source: 'landing',
      confirmed_at: 100,
      unsubscribed_at: 200,
      suppressed_at: 300,
    });
  });

  it('stores nulls when Cloudflare gives no location', async () => {
    await subscribeToNewsletter(testEnv, 'bob@example.com', undefined);
    expect(await row()).toMatchObject({
      email: 'bob@example.com',
      country: null,
      region_code: null,
    });
  });

  it('skips quietly when the donations database is not bound', async () => {
    const log = vi.spyOn(console, 'log').mockImplementation(() => {});
    await subscribeToNewsletter({ ...testEnv, DONATE_DB: undefined }, 'a@example.com', cf);
    expect(log).toHaveBeenCalledWith(expect.stringContaining('not set'));
    log.mockRestore();
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
