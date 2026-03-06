export interface User {
  id: string;
  email: string;
  name?: string;
  e2ee_key?: string;
}

export interface Device {
  id: string;
  owner: string;
  name: string;
  platform: string;
  last_upload_at: number | null;
  status: "online" | "offline";
  enabled: boolean;
}

export interface Batch {
  id: string;
  device_id: string;
  start: number;
  end: number;
  end_hash: string;
  url: string;
}

export interface DataLog {
  device_id: string;
  ts: number;
  type: string;
  data: Record<string, unknown>;
}

export interface DataPage {
  batches: Batch[];
  logs: DataLog[];
  next_cursor?: number;
}

export interface Partner {
  id: string;
  partner: {
    id?: string;
    email: string;
    name?: string;
  };
  status: "pending" | "accepted";
  permissions: { view_data: boolean };
  created_at: number;
  e2ee_key?: string;
}

const BASE = (import.meta as any).env?.VITE_API_URL ?? "http://localhost:8787";

async function req<T>(
  path: string,
  init: RequestInit = {},
  token?: string,
): Promise<T> {
  const headers = new Headers(init.headers);

  if (!headers.has("Content-Type") && !(init.body instanceof FormData)) {
    headers.set("Content-Type", "application/json");
  }

  if (token) {
    headers.set("Authorization", `Bearer ${token}`);
  }

  const res = await fetch(`${BASE}${path}`, {
    ...init,
    credentials: "include",
    headers,
  });

  if (!res.ok) {
    const body = (await res.json().catch(() => ({}))) as {
      error?: unknown;
      details?: unknown;
    };
    const message = typeof body.error === "string" ? body.error : res.statusText;
    throw Object.assign(new Error(message), { status: res.status, details: body.details });
  }

  if (res.status === 204) {
    return undefined as T;
  }

  return res.json();
}

export const api = {
  refreshToken: () => req<{ access_token: string }>("/token", { method: "POST" }),

  login: (email: string, password: string) =>
    req<{ access_token: string }>("/login", {
      method: "POST",
      body: JSON.stringify({ email, password }),
    }),

  signup: (email: string, password: string, name?: string) =>
    req<{ access_token: string; user: { id: string; email: string; name?: string } }>(
      "/signup",
      {
        method: "POST",
        body: JSON.stringify({ email, password, ...(name ? { name } : {}) }),
      },
    ),

  logout: () => req<void>("/logout", { method: "POST" }),

  getUser: (token: string) => req<User>("/user", {}, token),

  updateUser: (token: string, fields: { name?: string; e2ee_key?: string }) =>
    req<{ ok: boolean }>(
      "/user",
      {
        method: "PATCH",
        body: JSON.stringify(fields),
      },
      token,
    ),

  getDevices: (token: string) => req<Device[]>("/device", {}, token),

  patchDevice: (
    token: string,
    id: string,
    patch: { name?: string; enabled?: boolean },
  ) =>
    req<{ id: string; updated: boolean }>(
      `/device/${id}`,
      {
        method: "PATCH",
        body: JSON.stringify(patch),
      },
      token,
    ),

  getPartners: (token: string) => req<Partner[]>("/partner", {}, token),

  invitePartner: (
    token: string,
    email: string,
    permissions: { view_data: boolean },
  ) =>
    req<{ id: string; status: string }>(
      "/partner",
      {
        method: "POST",
        body: JSON.stringify({ email, permissions }),
      },
      token,
    ),

  acceptPartner: (token: string, id: string) =>
    req<{ id: string }>(
      "/partner/accept",
      {
        method: "POST",
        body: JSON.stringify({ id }),
      },
      token,
    ),

  updatePartner: (
    token: string,
    id: string,
    fields: { permissions?: { view_data?: boolean }; e2ee_key?: string },
  ) =>
    req<{ id: string; permissions: { view_data?: boolean } }>(
      `/partner/${id}`,
      {
        method: "PATCH",
        body: JSON.stringify(fields),
      },
      token,
    ),

  deletePartner: (token: string, id: string) =>
    req<void>(`/partner/${id}`, { method: "DELETE" }, token),

  getData: (
    token: string,
    params?: {
      user?: string;
      device_id?: string;
      cursor?: number;
      limit?: number;
    },
  ) => {
    const qs = new URLSearchParams();
    if (params?.user) qs.set("user", params.user);
    if (params?.device_id) qs.set("device_id", params.device_id);
    if (params?.cursor !== undefined) qs.set("cursor", String(params.cursor));
    if (params?.limit) qs.set("limit", String(params.limit));
    const query = qs.toString();
    return req<DataPage>(`/data${query ? `?${query}` : ""}`, {}, token);
  },
};
