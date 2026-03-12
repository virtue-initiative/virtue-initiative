import { beforeEach, describe, expect, it } from 'vitest';
import { SELF, env } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  latestEmailToken,
  listEmailDeliveries,
  markUserEmailVerified,
  signupAndGetToken,
} from './helpers';
import { verifyPassword } from '../src/lib/password';
beforeEach(clearDB);

describe('Auth routes', () => {
  it('creates a user, returns session credentials, and sends a verification email on signup', async () => {
    const res = await SELF.fetch(`${BASE}/signup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email: 'alice@example.com',
        password: 'client-side-hash',
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

    const deliveries = await listEmailDeliveries();
    expect(deliveries).toHaveLength(1);
    expect(deliveries[0]).toMatchObject({
      kind: 'email_verification',
      recipient_email: 'alice@example.com',
      status: 'sent',
    });
    const signupMetadata = JSON.parse(deliveries[0]!.metadata) as { verifyUrl: string };
    expect(new URL(signupMetadata.verifyUrl).origin).toBe('http://localhost:5173');

    const token = await latestEmailToken('email_verification');
    expect(token?.email).toBe('alice@example.com');
  });

  it('refreshes an access token from the refresh cookie', async () => {
    const signupRes = await SELF.fetch(`${BASE}/signup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'bob@example.com', password: 'pw' }),
    });

    const cookie = signupRes.headers.get('set-cookie') ?? '';
    const res = await SELF.fetch(`${BASE}/token`, {
      method: 'POST',
      headers: { Cookie: cookie },
    });

    expect(res.status).toBe(201);
    const body = (await res.json()) as { access_token: string };
    expect(body.access_token).toBeTruthy();
  });

  it('returns the current user and allows updating profile fields', async () => {
    const { token } = await signupAndGetToken('carol@example.com', 'pw', 'Carol');

    const patchRes = await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(token),
      body: JSON.stringify({
        name: 'Updated Carol',
        e2ee_key: Buffer.from('secret').toString('base64'),
        pub_key: Buffer.from('public-key').toString('base64'),
        priv_key: Buffer.from('private-key').toString('base64'),
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
      e2ee_key: string;
      pub_key: string;
      priv_key: string;
    };
    expect(body.name).toBe('Updated Carol');
    expect(body.email_verified).toBe(false);
    expect(Buffer.from(body.e2ee_key, 'base64').toString()).toBe('secret');
    expect(Buffer.from(body.pub_key, 'base64').toString()).toBe('public-key');
    expect(Buffer.from(body.priv_key, 'base64').toString()).toBe('private-key');
  });

  it('verifies email tokens and marks the user as verified', async () => {
    const { token } = await signupAndGetToken('verifyme@example.com', 'pw', 'Verify Me');
    const deliveries = await listEmailDeliveries();
    const metadata = JSON.parse(deliveries[0]!.metadata) as { verifyUrl: string };
    const verifyToken = new URL(metadata.verifyUrl).searchParams.get('verify_email_token');

    const verifyRes = await SELF.fetch(`${BASE}/verify-email`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token: verifyToken }),
    });
    expect(verifyRes.status).toBe(200);

    const userRes = await SELF.fetch(`${BASE}/user`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    const user = (await userRes.json()) as { email_verified: boolean };
    expect(user.email_verified).toBe(true);
  });

  it('resends verification emails for authenticated unverified users', async () => {
    const { token } = await signupAndGetToken('resend@example.com', 'pw');

    const resendRes = await SELF.fetch(`${BASE}/verify-email/request`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}` },
    });

    expect(resendRes.status).toBe(200);
    const deliveries = await listEmailDeliveries();
    expect(deliveries.filter((delivery) => delivery.kind === 'email_verification')).toHaveLength(2);
  });

  it('requests and applies password resets', async () => {
    const { userId } = await signupAndGetToken('reset@example.com', 'old-password', 'Reset User');
    await markUserEmailVerified(userId);

    const requestRes = await SELF.fetch(`${BASE}/password-reset/request`, {
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
      user_id: userId,
      key_rotation_required: false,
      partner_access_targets: [],
    });

    const resetRes = await SELF.fetch(`${BASE}/password-reset`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token: resetToken, password: 'new-password-hash' }),
    });
    expect(resetRes.status).toBe(200);

    const storedUser = await env.DB.prepare('SELECT password_hash FROM users WHERE email = ?')
      .bind('reset@example.com')
      .first<{ password_hash: string }>();
    expect(storedUser).toBeTruthy();
    expect(await verifyPassword('new-password-hash', storedUser!.password_hash)).toBe(true);
  });

  it('rotates encrypted keys during password resets and refreshes shared partner access blobs', async () => {
    const { token, userId } = await signupAndGetToken('secure-reset@example.com', 'pw');
    const { token: ownerToken, userId: ownerUserId } = await signupAndGetToken(
      'shared-owner@example.com',
      'pw',
    );
    await markUserEmailVerified(userId);
    await markUserEmailVerified(ownerUserId);
    await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({
        pub_key: Buffer.from('shared-owner-public-key').toString('base64'),
      }),
    });

    await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(token),
      body: JSON.stringify({
        e2ee_key: Buffer.from('wrapped-e2ee').toString('base64'),
        pub_key: Buffer.from('wrapped-public').toString('base64'),
        priv_key: Buffer.from('wrapped-private').toString('base64'),
      }),
    });

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({
        email: 'shared-owner@example.com',
        permissions: { view_data: true },
      }),
    });
    const invite = (await inviteRes.json()) as { id: string };
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'shared-owner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/invite/accept`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    await env.DB.prepare('UPDATE partners SET e2ee_key = ? WHERE id = ?')
      .bind(Buffer.from('shared-access-key'), invite.id)
      .run();

    await SELF.fetch(`${BASE}/password-reset/request`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: 'secure-reset@example.com' }),
    });

    const deliveries = await listEmailDeliveries();
    const resetDelivery = deliveries.find((delivery) => delivery.kind === 'password_reset');
    const resetMetadata = JSON.parse(resetDelivery!.metadata) as { resetUrl: string };
    const resetToken = new URL(resetMetadata.resetUrl).searchParams.get('reset_password_token');

    const validateRes = await SELF.fetch(`${BASE}/password-reset/validate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token: resetToken }),
    });
    expect(await validateRes.json()).toEqual({
      ok: true,
      email: 'secure-reset@example.com',
      user_id: userId,
      key_rotation_required: true,
      partner_access_targets: [
        {
          partnership_id: invite.id,
          partner_email: 'shared-owner@example.com',
          partner_pub_key: Buffer.from('shared-owner-public-key').toString('base64'),
        },
      ],
    });

    const missingWrapRes = await SELF.fetch(`${BASE}/password-reset`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token: resetToken, password: 'new-password-hash' }),
    });
    expect(missingWrapRes.status).toBe(400);

    const resetRes = await SELF.fetch(`${BASE}/password-reset`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        token: resetToken,
        password: 'new-password-hash',
        e2ee_key: Buffer.from('rotated-e2ee').toString('base64'),
        pub_key: Buffer.from('rotated-public').toString('base64'),
        priv_key: Buffer.from('rotated-private').toString('base64'),
        partner_access_keys: [
          {
            partnership_id: invite.id,
            e2ee_key: Buffer.from('rotated-shared-access').toString('base64'),
          },
        ],
      }),
    });
    expect(resetRes.status).toBe(200);

    const storedUser = await env.DB.prepare(
      'SELECT password_hash, e2ee_key, pub_key, priv_key FROM users WHERE email = ?',
    )
      .bind('secure-reset@example.com')
      .first<{
        password_hash: string;
        e2ee_key: ArrayBuffer;
        pub_key: ArrayBuffer;
        priv_key: ArrayBuffer;
      }>();
    expect(storedUser).toBeTruthy();
    expect(await verifyPassword('new-password-hash', storedUser!.password_hash)).toBe(true);
    expect(Buffer.from(storedUser!.e2ee_key).toString()).toBe('rotated-e2ee');
    expect(Buffer.from(storedUser!.pub_key).toString()).toBe('rotated-public');
    expect(Buffer.from(storedUser!.priv_key).toString()).toBe('rotated-private');

    const sharedAccess = await env.DB.prepare('SELECT e2ee_key FROM partners WHERE id = ?')
      .bind(invite.id)
      .first<{ e2ee_key: ArrayBuffer | null }>();
    expect(Buffer.from(sharedAccess?.e2ee_key ?? new ArrayBuffer(0)).toString()).toBe(
      'rotated-shared-access',
    );
  });
});
