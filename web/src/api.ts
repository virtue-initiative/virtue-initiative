export interface User {
  id: string;
  email: string;
  email_verified: boolean;
  email_bounced_at: number | null;
  name?: string;
  e2ee_key?: string;
  pub_key?: string;
  priv_key?: string;
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
  risk?: number;
}

export interface DataPage {
  batches: Batch[];
  logs: DataLog[];
  next_cursor?: number;
}

export interface Partner {
  id: string;
  role: "owner" | "invitee";
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

export interface NotificationPreference {
  partnership_id: string;
  status: "pending" | "accepted";
  monitored_user: {
    id: string;
    email: string;
    name?: string;
  };
  digest_cadence: "daily" | "twice_weekly" | "weekly" | "none";
  immediate_tamper_severity: "warning" | "critical";
  send_digest: boolean;
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
  user_id: string;
  key_rotation_required: boolean;
  partner_access_targets: Array<{
    partnership_id: string;
    partner_email: string;
    partner_pub_key?: string;
  }>;
}

const BASE = (import.meta as any).env?.VITE_API_URL ?? "http://localhost:8787";

type ReauthHandler = () => Promise<string | null>;

interface RequestOptions {
  allowReauth?: boolean;
  retrying?: boolean;
}

let reauthHandler: ReauthHandler | null = null;
let reauthInFlight: Promise<string | null> | null = null;

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
    const message =
      typeof body.error === "string" ? body.error : res.statusText;
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

  login: (email: string, password: string) =>
    req<{ access_token: string }>("/login", {
      method: "POST",
      body: JSON.stringify({ email, password }),
    }),

  signup: (email: string, password: string, name?: string) =>
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
      body: JSON.stringify({ email, password, ...(name ? { name } : {}) }),
    }),

  logout: () => req<void>("/logout", { method: "POST" }),

  getUser: (token: string) => req<User>("/user", {}, token),

  updateUser: (
    token: string,
    fields: {
      email?: string;
      name?: string;
      e2ee_key?: string;
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
      "/verify-email/request",
      { method: "POST" },
      token,
    ),

  verifyEmail: (token: string) =>
    req<{ ok: boolean }>("/verify-email", {
      method: "POST",
      body: JSON.stringify({ token }),
    }),

  requestPasswordReset: (email: string) =>
    req<void>("/password-reset/request", {
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
    password: string,
    wrappedKeys?: {
      e2ee_key?: string;
      pub_key?: string;
      priv_key?: string;
      partner_access_keys?: Array<{ partnership_id: string; e2ee_key: string }>;
    },
  ) =>
    req<{ ok: boolean }>("/password-reset", {
      method: "POST",
      body: JSON.stringify({ token, password, ...(wrappedKeys ?? {}) }),
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

  getPartners: (token: string) => req<Partner[]>("/partner", {}, token),

  getPartnerPublicKey: async (email: string) => {
    const qs = new URLSearchParams({ email });
    const result = await req<{ pubkey: string }>(`/pubkey?${qs.toString()}`);
    return result.pubkey;
  },

  invitePartner: (
    token: string,
    email: string,
    permissions: { view_data: boolean },
    e2ee_key?: string,
  ) =>
    req<{ id: string; status: string }>(
      "/partner",
      {
        method: "POST",
        body: JSON.stringify({
          email,
          permissions,
          ...(e2ee_key ? { e2ee_key } : {}),
        }),
      },
      token,
    ),

  validatePartnerInvite: (inviteToken: string) =>
    req<PartnerInviteValidation>("/partner/invite/validate", {
      method: "POST",
      body: JSON.stringify({ token: inviteToken }),
    }),

  acceptPartnerInvite: (token: string, inviteToken: string) =>
    req<{ id: string }>(
      "/partner/invite/accept",
      {
        method: "POST",
        body: JSON.stringify({ token: inviteToken }),
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

  getNotificationPreferences: (token: string) =>
    req<NotificationPreference[]>("/notifications/preferences", {}, token),

  updateNotificationPreference: (
    token: string,
    id: string,
    patch: Partial<
      Pick<
        NotificationPreference,
        "digest_cadence" | "immediate_tamper_severity" | "send_digest"
      >
    >,
  ) =>
    req<{
      partnership_id: string;
      digest_cadence: "none" | "daily" | "twice_weekly" | "weekly";
      immediate_tamper_severity: "warning" | "critical";
      send_digest: boolean;
    }>(
      `/notifications/preferences/${id}`,
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
