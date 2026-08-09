import { beforeEach, describe, expect, it } from 'vitest';
import { SELF, env } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  extractTokenFromDelivery,
  latestEmailToken,
  listEmailDeliveries,
  markUserEmailVerified,
  passwordAuthFor,
  passwordSaltFor,
  privateKeyFor,
  publicKeyFor,
  signupAndGetCookie,
  uuidToBytes,
} from './helpers';
import { CURRENT_HASH_PARAMS, verifyPasswordAuth } from '../src/lib/password';

beforeEach(clearDB);

describe('Auth routes', () => {
  it('returns the current hash params and login material in an enumeration-safe shape', async () => {
    await signupAndGetCookie('alice@example.com', 'correct horse');

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

  it('signup-request sends an email with a signup token, and /signup creates a verified user', async () => {
    const password_auth = await passwordAuthFor('client-derived-auth');
    const password_salt = await passwordSaltFor('alice@example.com');
    const pub_key = await publicKeyFor('alice@example.com');
    const priv_key = privateKeyFor('alice@example.com');

    const requestRes = await SELF.fetch(`${BASE}/signup-request`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'alice@example.com' }),
    });
    expect(requestRes.status).toBe(200);
    expect(await requestRes.json()).toEqual({ ok: true });

    const deliveries = await listEmailDeliveries();
    expect(deliveries).toHaveLength(1);
    expect(deliveries[0]).toMatchObject({
      kind: 'email_verification',
      recipient_email: 'alice@example.com',
      status: 'sent',
    });

    const signupToken = await latestEmailToken('signup');
    expect(signupToken?.email).toBe('alice@example.com');
    expect(signupToken?.user_id).toBeNull();

    const verificationToken = extractTokenFromDelivery(deliveries[0]!, 'signup_token');
    expect(verificationToken).toBeTruthy();

    const signupRes = await SELF.fetch(`${BASE}/signup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        verification_token: verificationToken,
        password_auth,
        password_salt,
        pub_key,
        priv_key,
        name: 'Alice',
      }),
    });
    expect(signupRes.status).toBe(201);
    expect(signupRes.headers.get('set-cookie')).toContain('refresh_token=');

    const signupBody = (await signupRes.json()) as {
      user: { id: string; email: string; email_verified: boolean; name?: string };
    };
    expect(signupBody.user).toMatchObject({
      email: 'alice@example.com',
      email_verified: true,
      name: 'Alice',
    });

    const storedUser = await env.DB.prepare(
      'SELECT id, email_verified, password_hash, password_salt, pub_key, priv_key FROM users WHERE email = ?',
    )
      .bind('alice@example.com')
      .first<{
        id: ArrayBuffer;
        email_verified: number;
        password_hash: string;
        password_salt: ArrayBuffer;
        pub_key: ArrayBuffer;
        priv_key: ArrayBuffer;
      }>();
    expect(storedUser).toBeTruthy();
    expect(storedUser?.email_verified).toBe(1);
    expect(new Uint8Array(storedUser!.id)).toHaveLength(16);
    expect(
      await verifyPasswordAuth(Buffer.from(password_auth, 'base64'), storedUser!.password_hash),
    ).toBe(true);
    expect(Buffer.from(storedUser!.password_salt).toString('base64')).toBe(password_salt);
    expect(Buffer.from(storedUser!.pub_key).toString('base64')).toBe(pub_key);
    expect(Buffer.from(storedUser!.priv_key).toString('base64')).toBe(priv_key);

    const consumedToken = await latestEmailToken('signup');
    expect(consumedToken?.consumed_at).not.toBeNull();
  });

  it('rejects signup with invalid or reused verification token', async () => {
    const badRes = await SELF.fetch(`${BASE}/signup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        verification_token: 'not-a-real-token',
        password_auth: await passwordAuthFor('pw'),
        password_salt: await passwordSaltFor('nope@example.com'),
        pub_key: await publicKeyFor('nope@example.com'),
        priv_key: privateKeyFor('nope@example.com'),
      }),
    });
    expect(badRes.status).toBe(400);
  });

  it('rejects signup-request for an existing account', async () => {
    await signupAndGetCookie('taken@example.com', 'pw');

    const res = await SELF.fetch(`${BASE}/signup-request`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'taken@example.com' }),
    });
    expect(res.status).toBe(409);
  });

  it('logs in with password_auth and sets a refresh cookie', async () => {
    await signupAndGetCookie('bob@example.com', 'pw');

    const loginRes = await SELF.fetch(`${BASE}/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email: 'bob@example.com',
        password_auth: await passwordAuthFor('pw'),
      }),
    });
    expect(loginRes.status).toBe(200);
    expect(loginRes.headers.get('set-cookie')).toContain('refresh_token=');
    const loginBody = (await loginRes.json()) as { ok: boolean; refresh_token: string };
    expect(loginBody).toMatchObject({ ok: true });
    expect(typeof loginBody.refresh_token).toBe('string');
    expect(loginBody.refresh_token.length).toBeGreaterThan(0);

    const badLoginRes = await SELF.fetch(`${BASE}/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email: 'bob@example.com',
        password_auth: await passwordAuthFor('wrong'),
      }),
    });
    expect(badLoginRes.status).toBe(401);
  });

  it('returns the current user and allows updating profile fields', async () => {
    const { cookie, userId } = await signupAndGetCookie('carol@example.com', 'pw', 'Carol');

    const nextPubKey = await publicKeyFor('carol-updated');
    const nextPrivKey = privateKeyFor('carol-updated');

    const patchRes = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(cookie),
      body: JSON.stringify({
        name: 'Updated Carol',
        pub_key: nextPubKey,
        priv_key: nextPrivKey,
      }),
    });
    expect(patchRes.status).toBe(200);

    const getRes = await SELF.fetch(`${BASE}/user`, {
      headers: authHeaders(cookie),
    });
    expect(getRes.status).toBe(200);

    const body = (await getRes.json()) as {
      name: string;
      email_verified: boolean;
      settings: { email_frequency: string; timezone: string };
      pub_key: string;
      priv_key: string;
    };
    expect(body.name).toBe('Updated Carol');
    expect(body.email_verified).toBe(true);
    expect(body.settings).toMatchObject({ email_frequency: 'daily', timezone: 'UTC' });
    expect(body.pub_key).toBe(nextPubKey);
    expect(body.priv_key).toBe(nextPrivKey);
    await markUserEmailVerified(userId);
    const updateEmailRes = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(cookie),
      body: JSON.stringify({ email: 'carol-new@example.com' }),
    });
    expect(updateEmailRes.status).toBe(200);
    expect(await updateEmailRes.json()).toEqual({
      ok: true,
      email_verification_required: true,
      pending_email: 'carol-new@example.com',
    });

    const updatedUserRes = await SELF.fetch(`${BASE}/user`, {
      headers: authHeaders(cookie),
    });
    const updatedBody = (await updatedUserRes.json()) as {
      email: string;
      email_verified: boolean;
      email_bounced_at: number | null;
    };
    expect(updatedBody.email).toBe('carol@example.com');
    expect(updatedBody.email_verified).toBe(true);
    expect(updatedBody.email_bounced_at).toBeNull();

    const updateSettingsRes = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(cookie),
      body: JSON.stringify({
        settings: { email_frequency: 'weekly' },
      }),
    });
    expect(updateSettingsRes.status).toBe(200);

    const settingsUserRes = await SELF.fetch(`${BASE}/user`, {
      headers: authHeaders(cookie),
    });
    expect(
      (await settingsUserRes.json()) as {
        settings: { email_frequency: string };
      },
    ).toMatchObject({
      settings: { email_frequency: 'weekly' },
    });

    const deliveries = await listEmailDeliveries();
    expect(deliveries.filter((delivery) => delivery.kind === 'email_verification')).toHaveLength(2);
    expect(deliveries[deliveries.length - 1]).toMatchObject({
      kind: 'email_verification',
      recipient_email: 'carol-new@example.com',
      status: 'sent',
    });
    const latestMetadata = JSON.parse(deliveries[deliveries.length - 1]!.metadata) as {
      verifyUrl: string;
    };
    expect(new URL(latestMetadata.verifyUrl).pathname).toBe('/verify-email');
    expect(new URL(latestMetadata.verifyUrl).searchParams.has('token')).toBe(true);

    const latestVerificationToken = await latestEmailToken('email_change');
    expect(latestVerificationToken?.email).toBe('carol-new@example.com');
  });

  it('verifies email-change tokens and sets a new session cookie', async () => {
    const { cookie, userId } = await signupAndGetCookie('verifyme@example.com', 'pw', 'Verify Me');

    const updateEmailRes = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(cookie),
      body: JSON.stringify({ email: 'verifyme-new@example.com' }),
    });
    expect(updateEmailRes.status).toBe(200);
    expect(await updateEmailRes.json()).toMatchObject({
      ok: true,
      email_verification_required: true,
      pending_email: 'verifyme-new@example.com',
    });

    const preChangeVerifyUserRes = await SELF.fetch(`${BASE}/user`, {
      headers: authHeaders(cookie),
    });
    expect(await preChangeVerifyUserRes.json()).toMatchObject({
      email: 'verifyme@example.com',
      email_verified: true,
    });

    const latestDelivery = (await listEmailDeliveries()).at(-1);
    const emailChangeToken = latestDelivery
      ? extractTokenFromDelivery(latestDelivery, 'token')
      : null;
    expect(emailChangeToken).toBeTruthy();

    const verifyChangeRes = await SELF.fetch(`${BASE}/email-verification/validate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token: emailChangeToken }),
    });
    expect(verifyChangeRes.status).toBe(200);
    expect(verifyChangeRes.headers.get('set-cookie')).toContain('refresh_token=');

    const verifyChangeBody = (await verifyChangeRes.json()) as {
      ok: boolean;
      email: string;
      purpose: string;
    };
    expect(verifyChangeBody).toMatchObject({
      ok: true,
      email: 'verifyme-new@example.com',
      purpose: 'email_change',
    });

    const newCookie = (verifyChangeRes.headers.get('set-cookie') ?? '').match(
      /refresh_token=([^;]+)/,
    )?.[1];
    expect(newCookie).toBeTruthy();

    const userRes = await SELF.fetch(`${BASE}/user`, {
      headers: authHeaders(newCookie!),
    });
    const userBody = (await userRes.json()) as {
      id: string;
      email: string;
      email_verified: boolean;
    };
    expect(userBody).toMatchObject({
      email: 'verifyme-new@example.com',
      email_verified: true,
    });
    void userId;
  });

  it('requires matching email confirmation and permanently deletes the account with cascaded data cleanup', async () => {
    const { cookie, userId } = await signupAndGetCookie('delete-me@example.com', 'pw', 'Delete Me');
    const device = await createDeviceForUser('delete-me@example.com', 'pw', 'Phone', 'ios');

    const hashUploadRes = await SELF.fetch(`${BASE}/hash`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.token}` },
      body: new Uint8Array(32).fill(9),
    });
    expect(hashUploadRes.status).toBe(200);

    const form = new FormData();
    form.set('start_time', '1710000000000');
    form.set('end_time', '1710003600000');
    form.set(
      'access_keys',
      JSON.stringify({
        keys: { [userId]: Buffer.from('owner-envelope').toString('base64') },
      }),
    );
    form.set('file', new File([new Uint8Array([4, 5, 6])], 'batch.enc'));
    const batchRes = await SELF.fetch(`${BASE}/d/batch`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.refresh_token}` },
      body: form,
    });
    expect(batchRes.status).toBe(201);
    const batch = (await batchRes.json()) as { id: string; url: string };

    expect(await env.BUCKET.head(batch.url.replace(`${env.R2_URL}/`, ''))).toBeTruthy();

    const badDeleteRes = await SELF.fetch(`${BASE}/user`, {
      method: 'DELETE',
      headers: authHeaders(cookie),
      body: JSON.stringify({ confirm_email: 'wrong@example.com' }),
    });
    expect(badDeleteRes.status).toBe(400);

    const deleteRes = await SELF.fetch(`${BASE}/user`, {
      method: 'DELETE',
      headers: authHeaders(cookie),
      body: JSON.stringify({ confirm_email: 'delete-me@example.com' }),
    });
    expect(deleteRes.status).toBe(204);
    expect(deleteRes.headers.get('set-cookie')).toContain('refresh_token=');

    expect(
      await env.DB.prepare('SELECT id FROM users WHERE id = ?').bind(uuidToBytes(userId)).first(),
    ).toBeNull();
    expect(
      await env.DB.prepare('SELECT id FROM devices WHERE id = ?')
        .bind(uuidToBytes(device.id))
        .first(),
    ).toBeNull();
    expect(
      await env.DB.prepare('SELECT id FROM batches WHERE id = ?')
        .bind(uuidToBytes(batch.id))
        .first(),
    ).toBeNull();

    expect(
      await env.DB.prepare('SELECT COUNT(*) AS count FROM user_sessions WHERE user_id = ?')
        .bind(uuidToBytes(userId))
        .first<{ count: number }>(),
    ).toMatchObject({ count: 0 });
    expect(
      await env.DB.prepare('SELECT COUNT(*) AS count FROM email_tokens WHERE user_id = ?')
        .bind(uuidToBytes(userId))
        .first<{ count: number }>(),
    ).toMatchObject({ count: 0 });
    expect(
      await env.DB.prepare('SELECT COUNT(*) AS count FROM device_sessions WHERE device_id = ?')
        .bind(uuidToBytes(device.id))
        .first<{ count: number }>(),
    ).toMatchObject({ count: 0 });
    expect(await env.BUCKET.head(batch.url.replace(`${env.R2_URL}/`, ''))).toBeNull();
  });

  it('requests and applies password resets with new auth material and keypair bytes', async () => {
    const { userId } = await signupAndGetCookie('reset@example.com', 'old-password', 'Reset User');
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
    const resetToken = new URL(resetMetadata.resetUrl).searchParams.get('token');

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
