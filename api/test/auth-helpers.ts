import { createExecutionContext, env, SELF, waitOnExecutionContext } from 'cloudflare:test';
import worker from '../src/index';
import type { Env } from '../src/types/bindings';
import {
  BASE,
  TEST_TURNSTILE_TOKEN,
  extractTokenFromDelivery,
  listEmailDeliveries,
  passwordAuthFor,
  passwordSaltFor,
  privateKeyFor,
  publicKeyFor,
  uuidToBytes,
} from './helpers';

// Helpers shared by auth-edge-cases.test.ts and device-auth.test.ts. They only use HTTP, plus
// D1 for setup that has no endpoint (expiring a token, recording a bounce).

export function postJson(path: string, body: unknown, headers: Record<string, string> = {}) {
  return SELF.fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...headers },
    body: JSON.stringify(body),
  });
}

export function cookieFrom(res: Response): string | undefined {
  return (res.headers.get('set-cookie') ?? '').match(/refresh_token=([^;]*)/)?.[1];
}

export function bearer(token: string): Record<string, string> {
  return { Authorization: `Bearer ${token}` };
}

export function cookieOnly(token: string): Record<string, string> {
  return { Cookie: `refresh_token=${token}` };
}

export async function login(email: string, password = 'password123', extra = {}) {
  return postJson('/login', { email, password_auth: await passwordAuthFor(password), ...extra });
}

export async function loginCookie(email: string, password = 'password123') {
  const res = await login(email, password);
  if (res.status !== 204) throw new Error(`login failed: ${res.status}`);
  return cookieFrom(res)!;
}

/** Sends POST /signup-request and returns the token from the signup link that was emailed. */
export async function requestSignupToken(email: string): Promise<string> {
  const before = (await listEmailDeliveries()).length;
  const res = await postJson('/signup-request', { email, turnstile_token: TEST_TURNSTILE_TOKEN });
  if (res.status !== 204) throw new Error(`signup-request failed: ${res.status}`);
  const delivery = (await listEmailDeliveries()).slice(before).at(-1);
  const token = delivery && extractTokenFromDelivery(delivery, 'signup_token');
  if (!token) throw new Error('no signup token delivered');
  return token;
}

export async function signupBody(email: string, token: string, extra = {}) {
  return {
    verification_token: token,
    password_auth: await passwordAuthFor('password123'),
    password_salt: await passwordSaltFor(email),
    pub_key: await publicKeyFor(email),
    encrypted_priv_key: privateKeyFor(email),
    ...extra,
  };
}

/**
 * Sends POST /password-reset straight to the worker and waits for the work it does after
 * responding (API-012), so the test can check what was sent. Each call gets its own IP, because
 * the rate limiter's window outlives a single test.
 */
export async function postPasswordReset(
  email: string,
  bindings: Partial<Env> = {},
  ip: string = crypto.randomUUID(),
) {
  const ctx = createExecutionContext();
  const res = await worker.fetch(
    new Request(`${BASE}/password-reset`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'CF-Connecting-IP': ip,
      },
      body: JSON.stringify({ email }),
    }),
    { ...env, ...bindings },
    ctx,
  );
  await waitOnExecutionContext(ctx);
  return res;
}

/** Sends POST /password-reset and returns the token from the reset link, if one was emailed. */
export async function requestPasswordResetToken(email: string): Promise<string> {
  const before = (await listEmailDeliveries()).length;
  const res = await postPasswordReset(email);
  if (res.status !== 204) throw new Error(`password-reset failed: ${res.status}`);
  const delivery = (await listEmailDeliveries())
    .slice(before)
    .find((d) => d.kind === 'password_reset');
  const token = delivery && extractTokenFromDelivery(delivery, 'token');
  if (!token) throw new Error('no reset token delivered');
  return token;
}

export async function newPasswordMaterial(email: string, password: string, tag = 'new') {
  return {
    password_auth: await passwordAuthFor(password),
    password_salt: await passwordSaltFor(`${email}:${tag}`),
    pub_key: await publicKeyFor(`${email}:${tag}`),
    encrypted_priv_key: privateKeyFor(`${email}:${tag}`),
  };
}

/** Makes every email token for `email` already expired. */
export async function expireEmailTokens(email: string) {
  await env.DB.prepare('UPDATE email_tokens SET expires_at = 1 WHERE email = ?').bind(email).run();
}

export async function expireWebSessions(userId: string) {
  await env.DB.prepare('UPDATE user_sessions SET expires_at = 1 WHERE user_id = ?')
    .bind(uuidToBytes(userId))
    .run();
}

export async function expireDeviceSessions(deviceId: string) {
  await env.DB.prepare('UPDATE device_sessions SET expires_at = 1 WHERE device_id = ?')
    .bind(uuidToBytes(deviceId))
    .run();
}

export async function recordBounce(email: string, reason: 'permanent' | 'temporary' = 'permanent') {
  await env.DB.prepare(
    'INSERT INTO email_bounces (id, email, bounced_at, reason) VALUES (?, ?, ?, ?)',
  )
    .bind(uuidToBytes(crypto.randomUUID()), email, Date.now(), reason)
    .run();
}

export function decodeJwt(token: string): Record<string, unknown> {
  const payload = token.split('.')[1] ?? '';
  const base64 = payload.replace(/-/g, '+').replace(/_/g, '/');
  return JSON.parse(atob(base64.padEnd(base64.length + ((4 - (base64.length % 4)) % 4), '=')));
}

/** The comparable parts of a response: status, body text and content type. */
export async function snapshot(res: Response) {
  return {
    status: res.status,
    contentType: res.headers.get('content-type'),
    body: await res.text(),
  };
}
