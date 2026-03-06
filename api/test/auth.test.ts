import { describe, it, expect } from 'vitest';
import { generateAccessToken, verifyJWT } from '../src/lib/jwt';
import { hashPassword, verifyPassword } from '../src/lib/password';

describe('Password hashing', () => {
  it('hashes and verifies a correct password', async () => {
    const hash = await hashPassword('hunter2');
    expect(hash).toContain(':');
    expect(await verifyPassword('hunter2', hash)).toBe(true);
  });

  it('rejects a wrong password', async () => {
    const hash = await hashPassword('hunter2');
    expect(await verifyPassword('wrong', hash)).toBe(false);
  });

  it('produces a unique salt each time', async () => {
    const first = await hashPassword('same');
    const second = await hashPassword('same');
    expect(first).not.toBe(second);
  });
});

describe('JWT tokens', () => {
  const secret = 'test-secret-key-12345';

  it('generates and verifies an access token', async () => {
    const token = await generateAccessToken('user-123', secret, 900);
    const payload = await verifyJWT(token, secret);
    expect(payload.sub).toBe('user-123');
    expect(payload.type).toBe('access');
  });

  it('rejects a token signed with the wrong secret', async () => {
    const token = await generateAccessToken('user-123', secret, 900);
    await expect(verifyJWT(token, 'wrong-secret')).rejects.toThrow();
  });
});
