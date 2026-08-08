import { sendToast } from '../toast';
import '../cache/client';
import type {
  EmailFrequency,
  User,
  HashParams,
  LoginMaterial,
  Device,
  Batch,
  DataLog,
  BatchesPage,
  Updates,
  WatchingPartner,
  WatcherPartner,
  PartnerRelationships,
  PartnerInviteValidation,
  PasswordResetValidation,
  SignupPayload,
  SignupResponse,
  LoginResponse,
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
  BatchesPage,
  Updates,
  WatchingPartner,
  WatcherPartner,
  PartnerRelationships,
  PartnerInviteValidation,
  PasswordResetValidation,
  SignupPayload,
  SignupResponse,
  LoginResponse,
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

let unauthorizedHandler: (() => void) | null = null;

export function setUnauthorizedHandler(handler: (() => void) | null) {
  unauthorizedHandler = handler;
}

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

async function req<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);

  if (!headers.has('Content-Type') && !(init.body instanceof FormData)) {
    headers.set('Content-Type', 'application/json');
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
    if (res.status === 401) {
      unauthorizedHandler?.();
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
  getCurrentHashParams: () => req<HashParams>('/current-hash-params'),

  getLoginMaterial: (email: string) => {
    const qs = new URLSearchParams({ email });
    return req<LoginMaterial>(`/user/login-material?${qs.toString()}`);
  },

  login: (email: string, password_auth: string, timezone?: string) =>
    req<LoginResponse>('/login', {
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

  getUser: () => req<User>('/user'),

  getUpdates: () => req<Updates>('/updates'),

  updateUser: (fields: UpdateUserPayload) =>
    req<UpdateUserResponse>('/user', {
      method: 'PATCH',
      body: JSON.stringify(fields),
    }),

  deleteUser: (confirm_email: string) =>
    req<void>('/user', {
      method: 'DELETE',
      body: JSON.stringify({ confirm_email }),
    }),

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

  getDevices: () => req<Device[]>('/device'),

  patchDevice: (id: string, patch: { name?: string }) =>
    req<PatchDeviceResponse>(`/device/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }),

  deleteDevice: (id: string) => req<void>(`/device/${id}`, { method: 'DELETE' }),

  getPartners: () => req<PartnerRelationships>('/partner'),

  invitePartner: (email: string) =>
    req<CreatePartnerResponse>('/partner', {
      method: 'POST',
      body: JSON.stringify({ email }),
    }),

  validatePartnerInvite: (inviteToken: string) =>
    req<PartnerInviteValidation>('/partner/validate', {
      method: 'POST',
      body: JSON.stringify({ token: inviteToken }),
    }),

  acceptPartnerInvite: (inviteToken: string) =>
    req<{ id: string }>('/partner/accept', {
      method: 'POST',
      body: JSON.stringify({ token: inviteToken }),
    }),

  deleteWatcher: (id: string) => req<void>(`/partner/watcher/${id}`, { method: 'DELETE' }),

  deleteWatching: (id: string) => req<void>(`/partner/watching/${id}`, { method: 'DELETE' }),

  getBatches: (params?: { user?: string; since?: number }) => {
    const qs = new URLSearchParams();
    if (params?.user) qs.set('user', params.user);
    if (params?.since !== undefined) qs.set('since', String(params.since));
    const query = qs.toString();
    return req<BatchesPage>(`/batches${query ? `?${query}` : ''}`);
  },
};
