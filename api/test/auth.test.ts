import { describe, it, expect } from 'vitest';
import { hashPassword, verifyPassword } from '../src/lib/password';
import { generateAccessToken, verifyJWT } from '../src/lib/jwt';

// These tests run inside the Workers runtime context (via @cloudflare/vitest-pool-workers).
// They import crypto utilities directly — no HTTP layer, no D1.
// PBKDF2-SHA-256 with 100k iterations takes ~5–50ms per call in workerd.

describe('Password hashing (Argon2id)', () => {
  it('hashes and verifies a correct password', async () => {
    const hash = await hashPassword('hunter2');
    expect(hash).toContain(':'); // salt:hash format
    expect(await verifyPassword('hunter2', hash)).toBe(true);
  });

  it('rejects a wrong password', async () => {
    const hash = await hashPassword('hunter2');
    expect(await verifyPassword('wrong', hash)).toBe(false);
  });

  it('produces a unique salt each time', async () => {
    const h1 = await hashPassword('same');
    const h2 = await hashPassword('same');
    expect(h1).not.toBe(h2);
    expect(await verifyPassword('same', h1)).toBe(true);
    expect(await verifyPassword('same', h2)).toBe(true);
  });
});

describe('JWT tokens', () => {
  const secret = 'test-secret-key-12345';
  const userId = 'user-123';

  it('generates and verifies an access token', async () => {
    const token = await generateAccessToken(userId, secret, '15m');
    const payload = await verifyJWT(token, secret);
    expect(payload.sub).toBe(userId);
    expect(payload.type).toBe('access');
  });

  it('rejects a token signed with the wrong secret', async () => {
    const token = await generateAccessToken(userId, secret, '15m');
    await expect(verifyJWT(token, 'wrong-secret')).rejects.toThrow();
  });

  it('rejects an expired token', async () => {
    const token = await generateAccessToken(userId, secret, '1s');
    await new Promise((r) => setTimeout(r, 1500));
    await expect(verifyJWT(token, secret)).rejects.toThrow();
  });
});
