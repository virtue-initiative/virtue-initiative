export interface Device {
  id: string;
  name: string;
  platform: string;
  last_seen_at: string | null;
  last_upload_at: string | null;
  status: 'online' | 'offline';
  enabled: boolean;
}

export interface Batch {
  id: string;
  device_id: string;
  r2_key: string;
  start_time: string;
  end_time: string;
  start_chain_hash: string;
  end_chain_hash: string;
  item_count: number;
  size_bytes: number;
  created_at: string;
}

export interface BatchPage {
  items: Batch[];
  next_cursor?: string;
}

export interface ChainHash {
  state_hex: string;
}

export interface BatchBlobItem {
  id: string;
  taken_at: number;
  kind: string;
  image?: Uint8Array;
  metadata: [string, string][];
}

export interface Partner {
  id: string;
  partner_user_id: string;
  partner_email: string;
  status: 'pending' | 'accepted';
  permissions: { view_data: boolean };
  role: 'owner' | 'partner';
  created_at: string;
  encryptedE2EEKey: string | null;
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

  getE2EEKey: (token: string) =>
    req<{ encryptedE2EEKey: string | null }>('/e2ee', {}, token),

  setE2EEKey: (token: string, encryptedKey: string) =>
    req<{ encryptedE2EEKey: string }>('/e2ee', {
      method: 'POST',
      body: JSON.stringify({ encryptedE2EEKey: encryptedKey }),
    }, token),

  getDevices: (token: string, params?: { user?: string }) => {
    const qs = params?.user ? `?user=${encodeURIComponent(params.user)}` : '';
    return req<Device[]>(`/device${qs}`, {}, token);
  },

  getPartners: (token: string) => req<Partner[]>('/partner', {}, token),

  invitePartner: (
    token: string,
    email: string,
    permissions: { view_data: boolean },
  ) =>
    req<{ id: string; status: string }>('/partner', {
      method: 'POST',
      body: JSON.stringify({ email, permissions }),
    }, token),

  acceptPartner: (token: string, id: string, encryptedKey?: string) =>
    req<{ id: string }>('/partner/accept', {
      method: 'POST',
      body: JSON.stringify({ id, ...(encryptedKey ? { encryptedE2EEKey: encryptedKey } : {}) }),
    }, token),

  deletePartner: (token: string, id: string) =>
    req<void>(`/partner/${id}`, { method: 'DELETE' }, token),

  patchDevice: (token: string, id: string, patch: { name?: string; enabled?: boolean }) =>
    req<{ id: string; updated: boolean }>(`/device/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }, token),

  getBatches: (token: string, params?: { user?: string; device_id?: string; cursor?: string; limit?: number }) => {
    const qs = new URLSearchParams();
    if (params?.user) qs.set('user', params.user);
    if (params?.device_id) qs.set('device_id', params.device_id);
    if (params?.cursor) qs.set('cursor', params.cursor);
    if (params?.limit) qs.set('limit', String(params.limit));
    const query = qs.toString();
    return req<BatchPage>(`/batch${query ? `?${query}` : ''}`, {}, token);
  },

  getDeviceState: (token: string, deviceId: string, user?: string) => {
    const qs = new URLSearchParams();
    qs.set('device_id', deviceId);
    if (user) qs.set('user', user);
    return req<{ state_hex: string }>(`/hash?${qs.toString()}`, {}, token);
  },
};
