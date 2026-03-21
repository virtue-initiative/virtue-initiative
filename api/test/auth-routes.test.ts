import { beforeEach, describe, expect, it } from 'vitest';
import { SELF, env } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  latestEmailToken,
  listEmailDeliveries,
  markUserEmailVerified,
  passwordAuthFor,
  passwordSaltFor,
  privateKeyFor,
  publicKeyFor,
  signupAndGetToken,
  uuidToBytes,
} from './helpers';
import { CURRENT_HASH_PARAMS, verifyPasswordAuth } from '../src/lib/password';

beforeEach(clearDB);

describe('Auth routes', () => {
  it('returns the current hash params and login material in an enumeration-safe shape', async () => {
    await signupAndGetToken('alice@example.com', 'correct horse');

    const paramsRes = await SELF.fetch(`${BASE}/current-hash-params`);
    expect(paramsRes.status).toBe(200);
    expect(await paramsRes.json()).toMatchObject({
      version: CURRENT_HASH_PARAMS.version,
      algorithm: CURRENT_HASH_PARAMS.algorithm,
      memory_cost_kib: CURRENT_HASH_PARAMS.memory_cost_kib,
      time_cost: CURRENT_HASH_PARAMS.time_cost,
      parallelism: CURRENT_HASH_PARAMS.parallelism,
      salt_length: CURRENT_HASH_PARAMS.salt_length,
      hkdf_hash: CURRENT_HASH_PARAMS.hkdf_hash,
    });

    const existingRes = await SELF.fetch(
      `${BASE}/user/login-material?email=${encodeURIComponent('alice@example.com')}`,
    );
    expect(existingRes.status).toBe(200);
    const existingBody = (await existingRes.json()) as {
      password_salt: string;
      params: { salt_length: number };
    };
    expect(Buffer.from(existingBody.password_salt, 'base64')).toHaveLength(
      CURRENT_HASH_PARAMS.salt_length,
    );

    const unknownRes = await SELF.fetch(
      `${BASE}/user/login-material?email=${encodeURIComponent('nobody@example.com')}`,
    );
    expect(unknownRes.status).toBe(200);
    const unknownBody = (await unknownRes.json()) as {
      password_salt: string;
      params: { salt_length: number };
    };
    expect(Buffer.from(unknownBody.password_salt, 'base64')).toHaveLength(
      CURRENT_HASH_PARAMS.salt_length,
    );
    expect(unknownBody).toHaveProperty('params');
  });

  it('creates a user, returns session credentials, and sends a verification email on signup', async () => {
    const password_auth = await passwordAuthFor('client-derived-auth');
    const password_salt = await passwordSaltFor('alice@example.com');
    const pub_key = await publicKeyFor('alice@example.com');
    const priv_key = privateKeyFor('alice@example.com');

    const res = await SELF.fetch(`${BASE}/signup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email: 'alice@example.com',
        password_auth,
        password_salt,
        pub_key,
        priv_key,
        name: 'Alice',
      }),
    });

    expect(res.status).toBe(201);
    expect(res.headers.get('set-cookie')).toContain('refresh_token=');

    const body = (await res.json()) as {
      access_token: string;
      user: { id: string; email: string; name: string; email_verified: boolean };
    };

    expect(body.access_token).toBeTruthy();
    expect(body.user.email).toBe('alice@example.com');
    expect(body.user.name).toBe('Alice');
    expect(body.user.email_verified).toBe(false);

    const storedUser = await env.DB.prepare(
      'SELECT id, password_hash, password_salt, pub_key, priv_key FROM users WHERE email = ?',
    )
      .bind('alice@example.com')
      .first<{
        id: ArrayBuffer;
        password_hash: string;
        password_salt: ArrayBuffer;
        pub_key: ArrayBuffer;
        priv_key: ArrayBuffer;
      }>();
    expect(storedUser).toBeTruthy();
    expect(new Uint8Array(storedUser!.id)).toHaveLength(16);
    expect(
      await verifyPasswordAuth(Buffer.from(password_auth, 'base64'), storedUser!.password_hash),
    ).toBe(true);
    expect(Buffer.from(storedUser!.password_salt).toString('base64')).toBe(password_salt);
    expect(Buffer.from(storedUser!.pub_key).toString('base64')).toBe(pub_key);
    expect(Buffer.from(storedUser!.priv_key).toString('base64')).toBe(priv_key);

    const session = await env.DB.prepare(
      `SELECT lower(hex(user_id)) as user_id_hex, expires_at
       FROM user_sessions
       WHERE user_id = ?
       ORDER BY created_at DESC
       LIMIT 1`,
    )
      .bind(uuidToBytes(body.user.id))
      .first<{
        user_id_hex: string | null;
        expires_at: number;
      }>();

    expect(session).toMatchObject({
      user_id_hex: body.user.id.replaceAll('-', ''),
    });
    expect(session?.expires_at).toBeGreaterThan(Date.now());

    const deliveries = await listEmailDeliveries();
    expect(deliveries).toHaveLength(1);
    expect(deliveries[0]).toMatchObject({
      kind: 'email_verification',
      recipient_email: 'alice@example.com',
      status: 'sent',
    });

    const token = await latestEmailToken('email_verification');
    expect(token?.email).toBe('alice@example.com');
  });

  it('logs in with password_auth and refreshes an access token from the refresh cookie', async () => {
    const signupRes = await SELF.fetch(`${BASE}/signup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email: 'bob@example.com',
        password_auth: await passwordAuthFor('pw'),
        password_salt: await passwordSaltFor('bob@example.com'),
        pub_key: await publicKeyFor('bob@example.com'),
        priv_key: privateKeyFor('bob@example.com'),
      }),
    });
    expect(signupRes.status).toBe(201);

    const loginRes = await SELF.fetch(`${BASE}/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email: 'bob@example.com',
        password_auth: await passwordAuthFor('pw'),
      }),
    });
    expect(loginRes.status).toBe(200);
    expect((await loginRes.json()) as { access_token: string }).toHaveProperty('access_token');

    const badLoginRes = await SELF.fetch(`${BASE}/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email: 'bob@example.com',
        password_auth: await passwordAuthFor('wrong'),
      }),
    });
    expect(badLoginRes.status).toBe(401);

    const cookie = signupRes.headers.get('set-cookie') ?? '';
    const refreshRes = await SELF.fetch(`${BASE}/token`, {
      method: 'POST',
      headers: { Cookie: cookie },
    });
    expect(refreshRes.status).toBe(201);
    expect((await refreshRes.json()) as { access_token: string }).toHaveProperty('access_token');
  });

  it('returns the current user and allows updating profile fields', async () => {
    const { token, userId } = await signupAndGetToken('carol@example.com', 'pw', 'Carol');

    const nextPubKey = await publicKeyFor('carol-updated');
    const nextPrivKey = privateKeyFor('carol-updated');

    const patchRes = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(token),
      body: JSON.stringify({
        name: 'Updated Carol',
        pub_key: nextPubKey,
        priv_key: nextPrivKey,
      }),
    });
    expect(patchRes.status).toBe(200);

    const getRes = await SELF.fetch(`${BASE}/user`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(getRes.status).toBe(200);

    const body = (await getRes.json()) as {
      name: string;
      email_verified: boolean;
      pub_key: string;
      priv_key: string;
    };
    expect(body.name).toBe('Updated Carol');
    expect(body.email_verified).toBe(false);
    expect(body.pub_key).toBe(nextPubKey);
    expect(body.priv_key).toBe(nextPrivKey);
    await markUserEmailVerified(userId);
    const updateEmailRes = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(token),
      body: JSON.stringify({ email: 'carol-new@example.com' }),
    });
    expect(updateEmailRes.status).toBe(200);

    const updatedUserRes = await SELF.fetch(`${BASE}/user`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    const updatedBody = (await updatedUserRes.json()) as {
      email: string;
      email_verified: boolean;
      email_bounced_at: number | null;
    };
    expect(updatedBody.email).toBe('carol-new@example.com');
    expect(updatedBody.email_verified).toBe(false);
    expect(updatedBody.email_bounced_at).toBeNull();
  });

  it('verifies email tokens and resends verification emails for authenticated users', async () => {
    const { token, userId } = await signupAndGetToken('verifyme@example.com', 'pw', 'Verify Me');
    const deliveries = await listEmailDeliveries();
    const metadata = JSON.parse(deliveries[0]!.metadata) as { verifyUrl: string };
    const verifyToken = new URL(metadata.verifyUrl).searchParams.get('verify_email_token');

    const verifyRes = await SELF.fetch(`${BASE}/email-verification/validate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token: verifyToken }),
    });
    expect(verifyRes.status).toBe(200);

    const userRes = await SELF.fetch(`${BASE}/user`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect((await userRes.json()) as { email_verified: boolean }).toMatchObject({
      email_verified: true,
    });

    await env.DB.prepare('UPDATE users SET email_verified = 0 WHERE id = ?')
      .bind(uuidToBytes(userId))
      .run();
    const resendRes = await SELF.fetch(`${BASE}/email-verification`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(resendRes.status).toBe(200);
  });

  it('blocks verification resend requests after a bounced delivery', async () => {
    const { token, userId } = await signupAndGetToken('bounced-resend@example.com', 'pw');
    await env.DB.prepare('UPDATE users SET email_bounced_at = ? WHERE id = ?')
      .bind(Date.now(), uuidToBytes(userId))
      .run();

    const resendRes = await SELF.fetch(`${BASE}/email-verification`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}` },
    });

    expect(resendRes.status).toBe(409);
  });

  it('requests and applies password resets with new auth material and keypair bytes', async () => {
    const { userId } = await signupAndGetToken('reset@example.com', 'old-password', 'Reset User');
    await markUserEmailVerified(userId);

    const requestRes = await SELF.fetch(`${BASE}/password-reset`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'reset@example.com' }),
    });
    expect(requestRes.status).toBe(204);

    const deliveries = await listEmailDeliveries();
    const resetDelivery = deliveries.find((delivery) => delivery.kind === 'password_reset');
    expect(resetDelivery?.recipient_email).toBe('reset@example.com');
    const resetMetadata = JSON.parse(resetDelivery!.metadata) as { resetUrl: string };
    const resetToken = new URL(resetMetadata.resetUrl).searchParams.get('reset_password_token');

    const validateRes = await SELF.fetch(`${BASE}/password-reset/validate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token: resetToken }),
    });
    expect(validateRes.status).toBe(200);
    expect(await validateRes.json()).toEqual({
      ok: true,
      email: 'reset@example.com',
    });

    const newPasswordAuth = await passwordAuthFor('new-password');
    const newPasswordSalt = await passwordSaltFor('reset@example.com:new');
    const newPubKey = await publicKeyFor('reset@example.com:new');
    const newPrivKey = privateKeyFor('reset@example.com:new');

    const resetRes = await SELF.fetch(`${BASE}/password-reset/finalize`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        token: resetToken,
        password_auth: newPasswordAuth,
        password_salt: newPasswordSalt,
        pub_key: newPubKey,
        priv_key: newPrivKey,
      }),
    });
    expect(resetRes.status).toBe(200);

    const storedUser = await env.DB.prepare(
      'SELECT password_hash, password_salt, pub_key, priv_key FROM users WHERE email = ?',
    )
      .bind('reset@example.com')
      .first<{
        password_hash: string;
        password_salt: ArrayBuffer;
        pub_key: ArrayBuffer;
        priv_key: ArrayBuffer;
      }>();
    expect(storedUser).toBeTruthy();
    expect(
      await verifyPasswordAuth(Buffer.from(newPasswordAuth, 'base64'), storedUser!.password_hash),
    ).toBe(true);
    expect(Buffer.from(storedUser!.password_salt).toString('base64')).toBe(newPasswordSalt);
    expect(Buffer.from(storedUser!.pub_key).toString('base64')).toBe(newPubKey);
    expect(Buffer.from(storedUser!.priv_key).toString('base64')).toBe(newPrivKey);
  });
});
