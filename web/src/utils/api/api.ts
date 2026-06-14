import { sendToast } from '../toast';
import type {
  EmailFrequency,
  User,
  HashParams,
  LoginMaterial,
  Device,
  Batch,
  DataLog,
  DataPage,
  WatchingPartner,
  WatcherPartner,
  PartnerRelationships,
  PartnerInviteValidation,
  PasswordResetValidation,
  SignupPayload,
  SignupResponse,
  AccessTokenResponse,
  EmailVerifyResponse,
  UpdateUserPayload,
  UpdateUserResponse,
  CreatePartnerResponse,
  PatchDeviceResponse,
} from '@virtueinitiative/shared-web/types';
export type {
  EmailFrequency,
  User,
  HashParams,
  LoginMaterial,
  Device,
  Batch,
  DataLog,
  DataPage,
  WatchingPartner,
  WatcherPartner,
  PartnerRelationships,
  PartnerInviteValidation,
  PasswordResetValidation,
  SignupPayload,
  SignupResponse,
  AccessTokenResponse,
  EmailVerifyResponse,
  UpdateUserPayload,
  UpdateUserResponse,
  CreatePartnerResponse,
  PatchDeviceResponse,
};

const BASE = (import.meta as any).env?.VITE_API_URL ?? 'http://localhost:8787';
const NETWORK_ERROR_MESSAGE = "Error: Couldn't connect to the network. Try reloading.";
const NETWORK_TOAST_THROTTLE_MS = 3_000;
let lastNetworkToastAt = 0;

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
      if (typeof item === 'string' && item.trim()) {
        return item;
      }
      const nested = firstValidationMessage(item);
      if (nested) {
        return nested;
      }
    }
    return null;
  }

  if (typeof details === 'object') {
    const record = details as Record<string, unknown>;

    if (Array.isArray(record.errors)) {
      const topError = record.errors.find(
        (error): error is string => typeof error === 'string' && error.trim().length > 0,
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

  if (!headers.has('Content-Type') && !(init.body instanceof FormData)) {
    headers.set('Content-Type', 'application/json');
  }

  if (token) {
    headers.set('Authorization', `Bearer ${token}`);
  }

  let res: Response;
  try {
    res = await fetch(`${BASE}${path}`, {
      ...init,
      credentials: 'include',
      headers,
    });
  } catch (error) {
    const now = Date.now();
    if (now - lastNetworkToastAt > NETWORK_TOAST_THROTTLE_MS) {
      sendToast(NETWORK_ERROR_MESSAGE, {
        isError: true,
        centered: true,
        dismissible: true,
        durationMs: null,
      });
      lastNetworkToastAt = now;
    }
    throw Object.assign(new Error(''), {
      toastHandled: true,
      cause: error,
    });
  }

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
    let message = typeof body.error === 'string' ? body.error : res.statusText;

    if (message === 'Bad Request') {
      const validationMessage = firstValidationMessage(body.details);
      message = validationMessage
        ? `Invalid request: ${validationMessage}`
        : 'Invalid request data';
    } else if (message === 'Unauthorized') {
      message = 'Your session is invalid or expired. Please log in again.';
    } else if (message === 'Not found') {
      message = 'Requested resource was not found.';
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
    req<AccessTokenResponse>('/token', { method: 'POST' }, undefined, {
      allowReauth: false,
    }),

  getCurrentHashParams: () => req<HashParams>('/current-hash-params'),

  getLoginMaterial: (email: string) => {
    const qs = new URLSearchParams({ email });
    return req<LoginMaterial>(`/user/login-material?${qs.toString()}`);
  },

  login: (email: string, password_auth: string, timezone?: string) =>
    req<AccessTokenResponse>('/login', {
      method: 'POST',
      body: JSON.stringify({ email, password_auth, ...(timezone ? { timezone } : {}) }),
    }),

  signupRequest: (email: string, to?: string) =>
    req<{ ok: boolean }>('/signup-request', {
      method: 'POST',
      body: JSON.stringify({
        email,
        ...(to ? { to } : {}),
      }),
    }),

  signup: (payload: SignupPayload) =>
    req<SignupResponse>('/signup', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  logout: () => req<void>('/logout', { method: 'POST' }),

  getUser: (token: string) => req<User>('/user', {}, token),

  updateUser: (token: string, fields: UpdateUserPayload) =>
    req<UpdateUserResponse>(
      '/user',
      {
        method: 'PATCH',
        body: JSON.stringify(fields),
      },
      token,
    ),

  deleteUser: (token: string, confirm_email: string) =>
    req<void>(
      '/user',
      {
        method: 'DELETE',
        body: JSON.stringify({ confirm_email }),
      },
      token,
    ),

  verifyEmail: (token: string) =>
    req<EmailVerifyResponse>('/email-verification/validate', {
      method: 'POST',
      body: JSON.stringify({ token }),
    }),

  requestPasswordReset: (email: string) =>
    req<void>('/password-reset', {
      method: 'POST',
      body: JSON.stringify({ email }),
    }),

  validatePasswordResetToken: (token: string) =>
    req<PasswordResetValidation>('/password-reset/validate', {
      method: 'POST',
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
    req<{ ok: boolean }>('/password-reset/finalize', {
      method: 'POST',
      body: JSON.stringify({ token, ...payload }),
    }),

  getDevices: (token: string) => req<Device[]>('/device', {}, token),

  patchDevice: (token: string, id: string, patch: { name?: string }) =>
    req<PatchDeviceResponse>(
      `/device/${id}`,
      {
        method: 'PATCH',
        body: JSON.stringify(patch),
      },
      token,
    ),

  deleteDevice: (token: string, id: string) =>
    req<void>(`/device/${id}`, { method: 'DELETE' }, token),

  getPartners: (token: string) => req<PartnerRelationships>('/partner', {}, token),

  invitePartner: (token: string, email: string) =>
    req<CreatePartnerResponse>(
      '/partner',
      {
        method: 'POST',
        body: JSON.stringify({ email }),
      },
      token,
    ),

  validatePartnerInvite: (inviteToken: string) =>
    req<PartnerInviteValidation>('/partner/validate', {
      method: 'POST',
      body: JSON.stringify({ token: inviteToken }),
    }),

  acceptPartnerInvite: (token: string, inviteToken: string) =>
    req<{ id: string }>(
      '/partner/accept',
      {
        method: 'POST',
        body: JSON.stringify({ token: inviteToken }),
      },
      token,
    ),

  deleteWatcher: (token: string, id: string) =>
    req<void>(`/partner/watcher/${id}`, { method: 'DELETE' }, token),

  deleteWatching: (token: string, id: string) =>
    req<void>(`/partner/watching/${id}`, { method: 'DELETE' }, token),
  getData: (
    token: string,
    params?: {
      user?: string;
      since?: number;
    },
  ) => {
    const qs = new URLSearchParams();
    if (params?.user) qs.set('user', params.user);
    if (params?.since !== undefined) qs.set('since', String(params.since));
    const query = qs.toString();
    return req<DataPage>(`/data${query ? `?${query}` : ''}`, {}, token);
  },
};
