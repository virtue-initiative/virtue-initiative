import { env, SELF } from 'cloudflare:test';
import { generateToken } from '../src/lib/jwt';

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

export async function clearDB(): Promise<void> {
  await env.DB.prepare('DELETE FROM hash_states').run();
  await env.DB.prepare('DELETE FROM device_logs').run();
  await env.DB.prepare('DELETE FROM batches').run();
  await env.DB.prepare('DELETE FROM partners').run();
  await env.DB.prepare('DELETE FROM devices').run();
  await env.DB.prepare('DELETE FROM users').run();
}
