import { env, SELF } from 'cloudflare:test';
import { generateToken } from '../src/lib/jwt';
import { clearMockEmailDeliveries, listMockEmailDeliveries } from '../src/lib/email';

export const BASE = 'http://localhost';

export async function signupAndGetToken(
  email: string,
  password = 'password123',
  name?: string,
): Promise<{ token: string; userId: string }> {
  const res = await SELF.fetch(`${BASE}/signup`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, password, ...(name ? { name } : {}) }),
  });

  if (!res.ok) {
    throw new Error(`signup failed: ${res.status} ${await res.text()}`);
  }

  const body = (await res.json()) as { access_token: string; user: { id: string } };
  return { token: body.access_token, userId: body.user.id };
}

export function authHeaders(token: string): Record<string, string> {
  return {
    'Content-Type': 'application/json',
    Authorization: `Bearer ${token}`,
  };
}

export async function createDeviceForUser(token: string, name = 'Laptop', platform = 'linux') {
  const res = await SELF.fetch(`${BASE}/d/device`, {
    method: 'POST',
    headers: authHeaders(token),
    body: JSON.stringify({ name, platform }),
  });

  if (!res.ok) {
    throw new Error(`device creation failed: ${res.status} ${await res.text()}`);
  }

  return (await res.json()) as {
    id: string;
    access_token: string;
    refresh_token: string;
  };
}

export async function createServerToken(deviceId: string) {
  return generateToken('server', deviceId, env.JWT_SECRET, 60);
}

export async function listEmailDeliveries() {
  return listMockEmailDeliveries();
}

export async function latestEmailToken(purpose: 'email_verification' | 'password_reset') {
  return env.DB.prepare(
    `SELECT id, user_id, email, purpose, token_hash, expires_at, consumed_at, created_at
     FROM email_tokens
     WHERE purpose = ?
     ORDER BY created_at DESC
     LIMIT 1`,
  )
    .bind(purpose)
    .first<{
      id: string;
      user_id: string;
      email: string;
      purpose: string;
      token_hash: string;
      expires_at: number;
      consumed_at: number | null;
      created_at: number;
    }>();
}

export async function markUserEmailVerified(userId: string) {
  await env.DB.prepare('UPDATE users SET email_verified = 1 WHERE id = ?').bind(userId).run();
}

export function extractTokenFromDelivery(
  delivery: { metadata: string; text: string },
  param: string,
) {
  const metadata = JSON.parse(delivery.metadata) as Record<string, string>;
  const url = Object.values(metadata).find((value) => value.includes?.(`?${param}=`));
  if (url) {
    return new URL(url).searchParams.get(param);
  }

  const match = delivery.text.match(new RegExp(`${param}=([^\\s]+)`));
  return match ? decodeURIComponent(match[1] ?? '') : null;
}

export async function clearDB(): Promise<void> {
  clearMockEmailDeliveries();
  await env.DB.prepare('DELETE FROM email_tokens').run();
  await env.DB.prepare('DELETE FROM partner_notification_preferences').run();
  await env.DB.prepare('DELETE FROM hash_states').run();
  await env.DB.prepare('DELETE FROM device_logs').run();
  await env.DB.prepare('DELETE FROM batches').run();
  await env.DB.prepare('DELETE FROM partners').run();
  await env.DB.prepare('DELETE FROM devices').run();
  await env.DB.prepare('DELETE FROM users').run();
}
