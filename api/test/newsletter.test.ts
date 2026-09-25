import { beforeAll, describe, expect, it, vi } from 'vitest';
import { env, fetchMock } from 'cloudflare:test';
import { subscribeToNewsletter } from '../src/lib/newsletter';
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

    await subscribeToNewsletter(envWithKey('bd-key'), 'alice@example.com');

    expect(captured?.headers.authorization).toBe('Token bd-key');
    // A collision header would let Buttondown resubscribe an address that opted out.
    expect(captured?.headers['x-buttondown-collision-behavior']).toBeUndefined();
    expect(captured?.body).toMatchObject({
      email_address: 'alice@example.com',
      type: 'regular',
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
