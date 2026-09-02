import { http, HttpResponse } from 'msw';
import { describe, expect, it } from 'vitest';
import { server } from '../mocks/server';
import { MIN_PASSWORD_LENGTH, checkPwnedPassword, passwordLengthError } from './password-policy';

async function sha1Hex(value: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-1', new TextEncoder().encode(value));
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')
    .toUpperCase();
}

/** Serves a range response built from real suffixes so the lookup finds them. */
function mockRange(body: (suffix: string) => string, password: string) {
  server.use(
    http.get('https://api.pwnedpasswords.com/range/:prefix', async ({ params }) => {
      const hash = await sha1Hex(password);
      expect(params.prefix).toBe(hash.slice(0, 5));
      return HttpResponse.text(body(hash.slice(5)));
    }),
  );
}

describe('passwordLengthError', () => {
  it('rejects a password one character below the minimum', () => {
    expect(passwordLengthError('a'.repeat(MIN_PASSWORD_LENGTH - 1))).toBe(
      'Use at least 12 characters.',
    );
  });

  it('accepts a password at the minimum', () => {
    expect(passwordLengthError('a'.repeat(MIN_PASSWORD_LENGTH))).toBeNull();
  });

  it('rejects an empty password', () => {
    expect(passwordLengthError('')).toBe('Use at least 12 characters.');
  });
});

describe('checkPwnedPassword', () => {
  it('returns the breach count when the suffix is present', async () => {
    mockRange(
      (suffix) => `AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:9\n${suffix}:4821`,
      'password123456',
    );
    expect(await checkPwnedPassword('password123456')).toBe(4821);
  });

  it('returns 0 when the suffix is absent', async () => {
    mockRange(() => 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:9', 'a-fine-password');
    expect(await checkPwnedPassword('a-fine-password')).toBe(0);
  });

  it('returns 0 for a padding row, which always has a count of zero', async () => {
    mockRange((suffix) => `${suffix}:0`, 'a-padded-password');
    expect(await checkPwnedPassword('a-padded-password')).toBe(0);
  });

  it('returns null when the request fails', async () => {
    server.use(
      http.get('https://api.pwnedpasswords.com/range/:prefix', () => HttpResponse.error()),
    );
    expect(await checkPwnedPassword('a-fine-password')).toBeNull();
  });

  it('returns null when the endpoint responds with an error status', async () => {
    server.use(
      http.get(
        'https://api.pwnedpasswords.com/range/:prefix',
        () => new HttpResponse(null, { status: 503 }),
      ),
    );
    expect(await checkPwnedPassword('a-fine-password')).toBeNull();
  });
});
