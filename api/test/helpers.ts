import { env, SELF } from 'cloudflare:test';
import { clearMockEmailDeliveries, listMockEmailDeliveries } from '../src/lib/email';
import { resetHashServerMock } from './hash-server-mock';

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

export async function signupAndGetCookie(
  email: string,
  password = 'password123',
  name?: string,
): Promise<{ cookie: string; userId: string }> {
  const password_auth = await passwordAuthFor(password);
  const password_salt = await passwordSaltFor(email);
  const pub_key = await publicKeyFor(email);
  const priv_key = privateKeyFor(email);

  const requestRes = await SELF.fetch(`${BASE}/signup-request`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email }),
  });

  if (!requestRes.ok) {
    throw new Error(`signup-request failed: ${requestRes.status} ${await requestRes.text()}`);
  }

  const deliveries = listMockEmailDeliveries();
  const signupDelivery = [...deliveries]
    .reverse()
    .find(
      (delivery) => delivery.kind === 'email_verification' && delivery.recipient_email === email,
    );

  if (!signupDelivery) {
    throw new Error(`signup verification delivery not found for ${email}`);
  }

  const verificationToken = extractTokenFromDelivery(signupDelivery, 'signup_token');
  if (!verificationToken) {
    throw new Error(`signup verification token not found for ${email}`);
  }

  const signupRes = await SELF.fetch(`${BASE}/signup`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      verification_token: verificationToken,
      password_auth,
      password_salt,
      pub_key,
      encrypted_priv_key: priv_key,
      ...(name ? { name } : {}),
    }),
  });

  if (!signupRes.ok) {
    throw new Error(`signup failed: ${signupRes.status} ${await signupRes.text()}`);
  }

  const signupBody = (await signupRes.json()) as { user: { id: string } };
  const setCookie = signupRes.headers.get('Set-Cookie') ?? '';
  const match = setCookie.match(/refresh_token=([^;]+)/);
  if (!match) {
    throw new Error('No refresh_token cookie in signup response');
  }
  return { cookie: match[1], userId: signupBody.user.id };
}

export function authHeaders(cookie: string): Record<string, string> {
  return {
    'Content-Type': 'application/json',
    Cookie: `refresh_token=${cookie}`,
  };
}

export async function createDeviceForUser(
  email: string,
  password = 'password123',
  name = 'Laptop',
  platform = 'linux',
) {
  const password_auth = await passwordAuthFor(password);
  const res = await SELF.fetch(`${BASE}/d/device`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, password_auth, name, platform }),
  });

  if (!res.ok) {
    throw new Error(`device creation failed: ${res.status} ${await res.text()}`);
  }

  const body = (await res.json()) as {
    token: string;
    settings: { id: string; hash_token: string };
  };

  return { id: body.settings.id, refresh_token: body.token, token: body.settings.hash_token };
}

export function batchMetadataForm(input: {
  start_time: number;
  end_time: number;
  access_keys: Record<string, string>;
  event_counts?: { total?: number; high?: number; medium?: number; screenshot?: number };
  notifications?: unknown[];
  file?: File;
}): FormData {
  const form = new FormData();
  form.set(
    'metadata',
    JSON.stringify({
      start_time: input.start_time,
      end_time: input.end_time,
      access_keys: input.access_keys,
      event_counts: {
        total: input.event_counts?.total ?? 0,
        high: input.event_counts?.high ?? 0,
        medium: input.event_counts?.medium ?? 0,
        screenshot: input.event_counts?.screenshot ?? 0,
      },
      ...(input.notifications ? { notifications: input.notifications } : {}),
    }),
  );
  form.set('file', input.file ?? new File([new Uint8Array([1, 2, 3])], 'batch.enc'));
  return form;
}

export async function listEmailDeliveries() {
  return listMockEmailDeliveries();
}

export async function latestEmailToken(
  purpose: 'email_verification' | 'email_change' | 'password_reset' | 'signup',
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
  resetHashServerMock();
  await env.DB.prepare('DELETE FROM device_auth_codes').run();
  await env.DB.prepare('DELETE FROM email_tokens').run();
  await env.DB.prepare('DELETE FROM user_sessions').run();
  await env.DB.prepare('DELETE FROM device_sessions').run();
  await env.DB.prepare('DELETE FROM batches').run();
  await env.DB.prepare('DELETE FROM partners').run();
  await env.DB.prepare('DELETE FROM devices').run();
  await env.DB.prepare('DELETE FROM users').run();
}
