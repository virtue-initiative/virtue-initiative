export interface Device {
  id: string;
  name: string;
  platform: string;
  last_seen_at: string | null;
  last_upload_at: string | null;
  interval_seconds: number;
  status: 'online' | 'offline';
  enabled: boolean;
}

export interface Partner {
  id: string;
  partner_email: string;
  status: 'pending' | 'accepted';
  permissions: { view_images: boolean; view_logs: boolean };
  role: 'owner' | 'partner';
  created_at: string;
}

const BASE = (import.meta as any).env?.VITE_API_URL ?? 'http://localhost:8787';

async function req<T>(path: string, init: RequestInit = {}, token?: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    credentials: 'include',
    headers: {
      'Content-Type': 'application/json',
      ...(token && { Authorization: `Bearer ${token}` }),
      ...(init.headers as Record<string, string>),
    },
  });
  if (!res.ok) {
    const body = (await res.json().catch(() => ({}))) as { error?: unknown };
    const msg = typeof body.error === 'string' ? body.error : res.statusText;
    throw Object.assign(new Error(msg), { status: res.status });
  }
  if (res.status === 204) return undefined as T;
  return res.json();
}

export const api = {
  refreshToken: () => req<{ access_token: string }>('/token', { method: 'POST' }),

  login: (email: string, password: string) =>
    req<{ access_token: string }>('/login', {
      method: 'POST',
      body: JSON.stringify({ email, password }),
    }),

  signup: (email: string, password: string, name?: string) =>
    req<{ access_token: string; user: object }>('/signup', {
      method: 'POST',
      body: JSON.stringify({ email, password, ...(name ? { name } : {}) }),
    }),

  logout: (token: string) => req<void>('/logout', { method: 'POST' }, token),

  getDevices: (token: string) => req<Device[]>('/device', {}, token),

  getPartners: (token: string) => req<Partner[]>('/partner', {}, token),
};
