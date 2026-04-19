import { env, SELF } from 'cloudflare:test';
import { generateToken } from '../src/lib/jwt';
import { clearMockEmailDeliveries, listMockEmailDeliveries } from '../src/lib/email';

export const BASE = 'http://localhost';

async function sha256Bytes(input: string) {
  return new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(input)));
}

export async function passwordAuthFor(password: string) {
  return Buffer.from(await sha256Bytes(`auth:${password}`)).toString('base64');
}

export async function passwordSaltFor(seed: string) {
  return Buffer.from((await sha256Bytes(`salt:${seed}`)).slice(0, 16)).toString('base64');
}

export async function publicKeyFor(seed: string) {
  return Buffer.from(await sha256Bytes(`pub:${seed}`)).toString('base64');
}

export function privateKeyFor(seed: string) {
  return Buffer.from(`priv:${seed}`).toString('base64');
}

export function uuidToBytes(uuid: string): ArrayBuffer {
  const normalized = normalizeUuidString(uuid);
  const hex = normalized.replace(/-/g, '');

  if (!hex) {
    throw new Error(`Invalid UUID: ${uuid}`);
  }

  const bytes = new Uint8Array(16);

  for (let i = 0; i < bytes.length; i += 1) {
    bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }

  return bytes.buffer;
}

function normalizeUuidString(uuid: string): string {
  if (/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(uuid)) {
    return uuid.toLowerCase();
  }

  if (/^[0-9a-f]{32}$/i.test(uuid)) {
    const hex = uuid.toLowerCase();
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
  }

  throw new Error(`Invalid UUID: ${uuid}`);
}

function bytesToUuid(value: ArrayBuffer) {
  const bytes = new Uint8Array(value);

  if (bytes.byteLength !== 16) {
    return normalizeUuidString(new TextDecoder().decode(bytes));
  }

  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
  return normalizeUuidString(hex);
}

export async function signupAndGetToken(
  email: string,
  password = 'password123',
  name?: string,
): Promise<{ token: string; userId: string }> {
  const password_auth = await passwordAuthFor(password);
  const password_salt = await passwordSaltFor(email);
  const pub_key = await publicKeyFor(email);
  const priv_key = privateKeyFor(email);
  const res = await SELF.fetch(`${BASE}/signup`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      email,
      password_auth,
      password_salt,
      pub_key,
      priv_key,
      ...(name ? { name } : {}),
    }),
  });

  if (!res.ok) {
    throw new Error(`signup failed: ${res.status} ${await res.text()}`);
  }

  const deliveries = await listMockEmailDeliveries();
  const signupDelivery = [...deliveries]
    .reverse()
    .find(
      (delivery) => delivery.kind === 'email_verification' && delivery.recipient_email === email,
    );

  if (!signupDelivery) {
    throw new Error(`signup verification delivery not found for ${email}`);
  }

  const verificationToken = extractTokenFromDelivery(signupDelivery, 'token');
  if (!verificationToken) {
    throw new Error(`signup verification token not found for ${email}`);
  }

  const verifyRes = await SELF.fetch(`${BASE}/email-verification/validate`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ token: verificationToken }),
  });

  if (!verifyRes.ok) {
    throw new Error(`signup verification failed: ${verifyRes.status} ${await verifyRes.text()}`);
  }

  const loginRes = await SELF.fetch(`${BASE}/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      email,
      password_auth,
    }),
  });

  if (!loginRes.ok) {
    throw new Error(
      `login failed after signup verification: ${loginRes.status} ${await loginRes.text()}`,
    );
  }

  const loginBody = (await loginRes.json()) as { access_token: string };
  const userRes = await SELF.fetch(`${BASE}/user`, {
    headers: { Authorization: `Bearer ${loginBody.access_token}` },
  });

  if (!userRes.ok) {
    throw new Error(
      `failed to load user after signup verification: ${userRes.status} ${await userRes.text()}`,
    );
  }

  const user = (await userRes.json()) as { id: string };
  return { token: loginBody.access_token, userId: user.id };
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
  return generateToken('server', deviceId, env.JWT_PRIVATE_KEY, 60);
}

export async function listEmailDeliveries() {
  return listMockEmailDeliveries();
}

export async function latestEmailToken(
  purpose: 'email_verification' | 'email_change' | 'password_reset',
) {
  const token = await env.DB.prepare(
    `SELECT id, user_id, email, purpose, token_hash, expires_at, consumed_at, created_at
     FROM email_tokens
     WHERE purpose = ?
     ORDER BY created_at DESC
     LIMIT 1`,
  )
    .bind(purpose)
    .first<{
      id: ArrayBuffer;
      user_id: ArrayBuffer | null;
      email: string;
      purpose: string;
      token_hash: string;
      expires_at: number;
      consumed_at: number | null;
      created_at: number;
    }>();

  if (!token) {
    return token;
  }

  return {
    ...token,
    id: bytesToUuid(token.id),
    user_id: token.user_id ? bytesToUuid(token.user_id) : null,
  };
}

export async function markUserEmailVerified(userId: string) {
  await env.DB.prepare('UPDATE users SET email_verified = 1 WHERE id = ?')
    .bind(uuidToBytes(userId))
    .run();
}

export function extractTokenFromDelivery(
  delivery: { metadata: string; text: string },
  param: string,
) {
  const metadata = JSON.parse(delivery.metadata) as Record<string, string>;
  const url = Object.values(metadata).find((value) => {
    if (typeof value !== 'string' || !value.includes('://')) {
      return false;
    }

    try {
      return new URL(value).searchParams.has(param);
    } catch {
      return false;
    }
  });
  if (url) {
    return new URL(url).searchParams.get(param);
  }

  const match = delivery.text.match(new RegExp(`[?&]${param}=([^\\s&]+)`));
  return match ? decodeURIComponent(match[1] ?? '') : null;
}

export async function clearDB(): Promise<void> {
  clearMockEmailDeliveries();
  await env.DB.prepare('DELETE FROM email_tokens').run();
  await env.DB.prepare('DELETE FROM user_sessions').run();
  await env.DB.prepare('DELETE FROM device_sessions').run();
  await env.DB.prepare('DELETE FROM hash_states').run();
  await env.DB.prepare('DELETE FROM device_logs').run();
  await env.DB.prepare('DELETE FROM batches').run();
  await env.DB.prepare('DELETE FROM partners').run();
  await env.DB.prepare('DELETE FROM devices').run();
  await env.DB.prepare('DELETE FROM users').run();
}
