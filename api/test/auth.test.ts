import { describe, it, expect } from 'vitest';
import { generateToken, verifyJWT } from '../src/lib/jwt';
import { generatePasswordSalt, hashPasswordAuth, verifyPasswordAuth } from '../src/lib/password';
import { assertTokenPurpose, generateOpaqueToken } from '../src/lib/tokens';
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

describe('Opaque token purpose prefixes', () => {
  it('prefixes generated tokens by purpose', () => {
    expect(generateOpaqueToken('web_session')).toMatch(/^wst_/);
    expect(generateOpaqueToken('device_session')).toMatch(/^dst_/);
    expect(generateOpaqueToken('signup')).toMatch(/^sut_/);
    expect(generateOpaqueToken('email_change')).toMatch(/^ect_/);
    expect(generateOpaqueToken('password_reset')).toMatch(/^prt_/);
    expect(generateOpaqueToken('partner_invite')).toMatch(/^pit_/);
  });

  it('accepts a token whose prefix matches the asserted purpose', () => {
    const token = generateOpaqueToken('web_session');
    expect(() => assertTokenPurpose(token, 'web_session')).not.toThrow();
  });

  it('rejects a token whose prefix does not match the asserted purpose', () => {
    const token = generateOpaqueToken('device_session');
    expect(() => assertTokenPurpose(token, 'web_session')).toThrow();
  });

  it('rejects an unprefixed legacy-shaped token', () => {
    expect(() => assertTokenPurpose('just-some-opaque-string', 'web_session')).toThrow();
  });
});
