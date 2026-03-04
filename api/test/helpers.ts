import { env } from 'cloudflare:test';
import { SELF } from 'cloudflare:test';

export const BASE = 'http://localhost';

/** Sign up a user and return the access token + user id */
export async function signupAndGetToken(
  email: string,
  password = 'password123',
): Promise<{ token: string; userId: string }> {
  const res = await SELF.fetch(`${BASE}/signup`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, password }),
  });
  if (!res.ok) throw new Error(`signup failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as { access_token: string; user: { id: string } };
  return { token: body.access_token, userId: body.user.id };
}

/** Build Authorization + Content-Type headers */
export function authHeaders(token: string): Record<string, string> {
  return {
    'Content-Type': 'application/json',
    Authorization: `Bearer ${token}`,
  };
}

/** Delete all rows in FK-safe order (call in beforeEach) */
export async function clearDB(): Promise<void> {
  await env.DB.prepare('DELETE FROM logs').run();
  await env.DB.prepare('DELETE FROM images').run();
  await env.DB.prepare('DELETE FROM partners').run();
  await env.DB.prepare('DELETE FROM settings').run();
  await env.DB.prepare('DELETE FROM devices').run();
  await env.DB.prepare('DELETE FROM users').run();
}
