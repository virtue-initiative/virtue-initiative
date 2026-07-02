import { describe, it, expect } from 'vitest';
import { generateToken, verifyJWT } from '../src/lib/jwt';
import { generatePasswordSalt, hashPasswordAuth, verifyPasswordAuth } from '../src/lib/password';
import {
  TEST_JWT_PRIVATE_KEY,
  TEST_JWT_PUBLIC_KEY,
  TEST_OTHER_JWT_PUBLIC_KEY,
} from './jwt-test-keys';

describe('Password auth hashing', () => {
  it('hashes and verifies correct password auth material', async () => {
    const passwordAuth = new TextEncoder().encode('hunter2-auth');
    const hash = await hashPasswordAuth(passwordAuth);
    expect(hash).toMatch(/^[a-f0-9]{64}$/);
    expect(await verifyPasswordAuth(passwordAuth, hash)).toBe(true);
  });

  it('rejects different password auth material', async () => {
    const hash = await hashPasswordAuth(new TextEncoder().encode('hunter2-auth'));
    expect(await verifyPasswordAuth(new TextEncoder().encode('wrong-auth'), hash)).toBe(false);
  });

  it('produces a unique random password salt each time', () => {
    const first = generatePasswordSalt();
    const second = generatePasswordSalt();
    expect(first).toHaveLength(16);
    expect(second).toHaveLength(16);
    expect(Buffer.from(first).equals(Buffer.from(second))).toBe(false);
  });
});

describe('JWT tokens', () => {
  it('generates and verifies a server token', async () => {
    const token = await generateToken('server', 'device-123', TEST_JWT_PRIVATE_KEY, 60);
    const payload = await verifyJWT(token, TEST_JWT_PUBLIC_KEY);
    expect(payload.sub).toBe('device-123');
    expect(payload.type).toBe('server');
  });

  it('generates and verifies a hash-server token', async () => {
    const token = await generateToken('hash-server', 'device-123', TEST_JWT_PRIVATE_KEY, 3600);
    const payload = await verifyJWT(token, TEST_JWT_PUBLIC_KEY);
    expect(payload.sub).toBe('device-123');
    expect(payload.type).toBe('hash-server');
  });

  it('rejects a token signed with a different public key', async () => {
    const token = await generateToken('server', 'device-123', TEST_JWT_PRIVATE_KEY, 60);
    await expect(verifyJWT(token, TEST_OTHER_JWT_PUBLIC_KEY)).rejects.toThrow();
  });
});
