export interface User {
  id: string;
  email: string;
  email_verified: boolean;
  email_bounced_at: number | null;
  name?: string;
  pub_key?: string;
  priv_key?: string;
}

export interface HashParams {
  version: string;
  algorithm: string;
  memory_cost_kib: number;
  time_cost: number;
  parallelism: number;
  salt_length: number;
  hkdf_hash: string;
}

export interface LoginMaterial {
  password_salt: string;
  params: HashParams;
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
  start_time: number;
  end_time: number;
  end_hash: string;
  url: string;
  encrypted_key: string;
}

export interface DataLog {
  device_id: string;
  ts: number;
  type: string;
  data: Record<string, unknown>;
  risk?: number;
}

export interface DataPage {
  batches: Batch[];
  logs: DataLog[];
  next_cursor?: number;
}

export interface WatchingPartner {
  id: string;
  user: {
    id: string;
    email: string;
    name?: string;
  };
  status: "pending" | "accepted";
  digest_cadence: "none" | "alerts-only" | "daily" | "weekly";
  immediate_tamper_severity: "warning" | "critical";
  created_at?: number;
}

export interface WatcherPartner {
  id: string;
  user: {
    id?: string;
    email: string;
    name?: string;
  };
  status: "pending" | "accepted";
  created_at?: number;
}

export interface PartnerRelationships {
  watching: WatchingPartner[];
  watchers: WatcherPartner[];
}

export interface PartnerInviteValidation {
  ok: boolean;
  partnership_id: string;
  owner: {
    id: string;
    email: string;
    name?: string;
  };
}

export interface PasswordResetValidation {
  ok: boolean;
  email: string;
}

const BASE = (import.meta as any).env?.VITE_API_URL ?? "http://localhost:8787";

type ReauthHandler = () => Promise<string | null>;

interface RequestOptions {
  allowReauth?: boolean;
  retrying?: boolean;
}

let reauthHandler: ReauthHandler | null = null;
let reauthInFlight: Promise<string | null> | null = null;

function firstValidationMessage(details: unknown): string | null {
  if (!details) {
    return null;
  }

  if (Array.isArray(details)) {
    for (const item of details) {
      if (typeof item === "string" && item.trim()) {
        return item;
      }
      const nested = firstValidationMessage(item);
      if (nested) {
        return nested;
      }
    }
    return null;
  }

  if (typeof details === "object") {
    const record = details as Record<string, unknown>;

    if (Array.isArray(record.errors)) {
      const topError = record.errors.find(
        (error): error is string =>
          typeof error === "string" && error.trim().length > 0,
      );
      if (topError) {
        return topError;
      }
    }

    for (const value of Object.values(record)) {
      const nested = firstValidationMessage(value);
      if (nested) {
        return nested;
      }
    }
  }

  return null;
}

export function setReauthHandler(handler: ReauthHandler | null) {
  reauthHandler = handler;
}

async function tryReauth() {
  if (!reauthHandler) {
    return null;
  }

  if (!reauthInFlight) {
    reauthInFlight = reauthHandler().finally(() => {
      reauthInFlight = null;
    });
  }

  return reauthInFlight;
}

async function req<T>(
  path: string,
  init: RequestInit = {},
  token?: string,
  options: RequestOptions = {},
): Promise<T> {
  const { allowReauth = Boolean(token), retrying = false } = options;
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
    if (res.status === 401 && token && allowReauth && !retrying) {
      const refreshedToken = await tryReauth();

      if (refreshedToken) {
        return req<T>(path, init, refreshedToken, {
          allowReauth,
          retrying: true,
        });
      }
    }

    const body = (await res.json().catch(() => ({}))) as {
      error?: unknown;
      details?: unknown;
    };
    let message = typeof body.error === "string" ? body.error : res.statusText;

    if (message === "Bad Request") {
      const validationMessage = firstValidationMessage(body.details);
      message = validationMessage
        ? `Invalid request: ${validationMessage}`
        : "Invalid request data";
    } else if (message === "Unauthorized") {
      message = "Your session is invalid or expired. Please log in again.";
    } else if (message === "Not found") {
      message = "Requested resource was not found.";
    }

    throw Object.assign(new Error(message), {
      status: res.status,
      details: body.details,
    });
  }

  if (res.status === 204) {
    return undefined as T;
  }

  return res.json();
}

export const api = {
  refreshToken: () =>
    req<{ access_token: string }>("/token", { method: "POST" }, undefined, {
      allowReauth: false,
    }),

  getCurrentHashParams: () => req<HashParams>("/current-hash-params"),

  getLoginMaterial: (email: string) => {
    const qs = new URLSearchParams({ email });
    return req<LoginMaterial>(`/user/login-material?${qs.toString()}`);
  },

  login: (email: string, password_auth: string) =>
    req<{ access_token: string }>("/login", {
      method: "POST",
      body: JSON.stringify({ email, password_auth }),
    }),

  signup: (
    email: string,
    payload: {
      password_auth: string;
      password_salt: string;
      pub_key: string;
      priv_key: string;
      name?: string;
    },
  ) =>
    req<{
      access_token: string;
      user: {
        id: string;
        email: string;
        name?: string;
        email_verified: boolean;
      };
    }>("/signup", {
      method: "POST",
      body: JSON.stringify({ email, ...payload }),
    }),

  logout: () => req<void>("/logout", { method: "POST" }),

  getUser: (token: string) => req<User>("/user", {}, token),

  updateUser: (
    token: string,
    fields: {
      email?: string;
      name?: string;
      pub_key?: string;
      priv_key?: string;
    },
  ) =>
    req<{ ok: boolean }>(
      "/user",
      {
        method: "PATCH",
        body: JSON.stringify(fields),
      },
      token,
    ),

  requestVerificationEmail: (token: string) =>
    req<{ ok: boolean; already_verified?: boolean }>(
      "/email-verification",
      { method: "POST" },
      token,
    ),

  verifyEmail: (token: string) =>
    req<{ ok: boolean; email: string }>("/email-verification/validate", {
      method: "POST",
      body: JSON.stringify({ token }),
    }),

  requestPasswordReset: (email: string) =>
    req<void>("/password-reset", {
      method: "POST",
      body: JSON.stringify({ email }),
    }),

  validatePasswordResetToken: (token: string) =>
    req<PasswordResetValidation>("/password-reset/validate", {
      method: "POST",
      body: JSON.stringify({ token }),
    }),

  resetPassword: (
    token: string,
    payload: {
      password_auth: string;
      password_salt: string;
      pub_key?: string;
      priv_key?: string;
    },
  ) =>
    req<{ ok: boolean }>("/password-reset/finalize", {
      method: "POST",
      body: JSON.stringify({ token, ...payload }),
    }),

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

  deleteDevice: (token: string, id: string) =>
    req<void>(`/device/${id}`, { method: "DELETE" }, token),

  getPartners: (token: string) =>
    req<PartnerRelationships>("/partner", {}, token),

  invitePartner: (token: string, email: string) =>
    req<{ id: string; status: string }>(
      "/partner",
      {
        method: "POST",
        body: JSON.stringify({ email }),
      },
      token,
    ),

  validatePartnerInvite: (inviteToken: string) =>
    req<PartnerInviteValidation>("/partner/validate", {
      method: "POST",
      body: JSON.stringify({ token: inviteToken }),
    }),

  acceptPartnerInvite: (token: string, inviteToken: string) =>
    req<{ id: string }>(
      "/partner/accept",
      {
        method: "POST",
        body: JSON.stringify({ token: inviteToken }),
      },
      token,
    ),

  deleteWatcher: (token: string, id: string) =>
    req<void>(`/partner/watcher/${id}`, { method: "DELETE" }, token),

  deleteWatching: (token: string, id: string) =>
    req<void>(`/partner/watching/${id}`, { method: "DELETE" }, token),

  updateNotificationPreference: (
    token: string,
    id: string,
    patch: Partial<
      Pick<WatchingPartner, "digest_cadence" | "immediate_tamper_severity">
    >,
  ) =>
    req<void>(
      `/partner/watching/${id}`,
      {
        method: "PATCH",
        body: JSON.stringify(patch),
      },
      token,
    ),

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
