import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
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
  DataPage,
  WatchingPartner,
  WatcherPartner,
  PartnerRelationships,
  PartnerInviteValidation,
  PasswordResetValidation,
  SignupPayload,
  SignupResponse,
  SignupValidation,
  EmailVerifyResponse,
  UpdateUserPayload,
  UpdateUserResponse,
  CreatePartnerResponse,
  BugReportPayload,
  LockedPassword,
  CreateLockedPasswordPayload,
  CreateLockedPasswordResponse,
  RevealLockedPasswordResponse,
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
  SignupValidation,
  EmailVerifyResponse,
  UpdateUserPayload,
  UpdateUserResponse,
  CreatePartnerResponse,
  BugReportPayload,
  LockedPassword,
  CreateLockedPasswordPayload,
  CreateLockedPasswordResponse,
  RevealLockedPasswordResponse,
};

const BASE =
  ((import.meta as any).env?.VITE_API_URL ?? 'http://localhost:8787') + `/${CURRENT_API_VERSION}`;
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

export function describeError(err: unknown, fallback: string): string | null {
  if (err && typeof err === 'object' && 'toastHandled' in err) {
    return null;
  }
  if (err instanceof Error && err.message) {
    return err.message;
  }
  return fallback;
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
  getCurrentHashParams: () =>
    req<{ params: HashParams }>('/user/login-material').then((res) => res.params),

  getLoginMaterial: (email: string) => {
    const qs = new URLSearchParams({ email });
    return req<LoginMaterial>(`/user/login-material?${qs.toString()}`);
  },

  login: (email: string, password_auth: string, timezone?: string) =>
    req<void>('/login', {
      method: 'POST',
      body: JSON.stringify({ email, password_auth, ...(timezone ? { timezone } : {}) }),
    }),

  signupRequest: (email: string, to?: string) =>
    req<void>('/signup-request', {
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

  validateSignupToken: (token: string) =>
    req<SignupValidation>('/signup/validate', {
      method: 'POST',
      body: JSON.stringify({ token }),
    }),

  logout: () => req<void>('/logout', { method: 'POST' }),

  getUser: () => req<User>('/user'),

  updateUser: (fields: UpdateUserPayload) =>
    req<UpdateUserResponse>('/user', {
      method: 'PATCH',
      body: JSON.stringify(fields),
    }),

  deleteUser: (confirm_email: string) =>
    req<void>(`/user?confirm_email=${encodeURIComponent(confirm_email)}`, {
      method: 'DELETE',
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
      encrypted_priv_key?: string;
    },
  ) =>
    req<{ ok: boolean }>('/password-reset/finalize', {
      method: 'POST',
      body: JSON.stringify({ token, ...payload }),
    }),

  getDevices: () => req<Device[]>('/device'),

  patchDevice: (id: string, patch: { name?: string }) =>
    req<void>(`/device/${id}`, {
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

  // The API exposes a single DELETE /partner/:id that works from either side of the
  // partnership; deleteWatcher/deleteWatching stay as separate names here only because
  // the call sites (removing a watcher vs. leaving a partnership you're watching) read
  // more clearly that way.
  deleteWatcher: (id: string) => req<void>(`/partner/${id}`, { method: 'DELETE' }),

  deleteWatching: (id: string) => req<void>(`/partner/${id}`, { method: 'DELETE' }),

  reportBug: (payload: BugReportPayload) => {
    const form = new FormData();
    form.set('metadata', JSON.stringify(payload));
    return req<void>('/bug-report', { method: 'POST', body: form });
  },

  getLockedPasswords: () => req<LockedPassword[]>('/locked-password'),

  createLockedPassword: (payload: CreateLockedPasswordPayload) =>
    req<CreateLockedPasswordResponse>('/locked-password', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  revealLockedPassword: (id: string) =>
    req<RevealLockedPasswordResponse>(`/locked-password/${id}/reveal`, { method: 'POST' }),

  deleteLockedPassword: (id: string) => req<void>(`/locked-password/${id}`, { method: 'DELETE' }),

  restoreLockedPassword: (id: string) =>
    req<void>(`/locked-password/${id}/restore`, { method: 'POST' }),

  permanentlyDeleteLockedPassword: (id: string) =>
    req<void>(`/locked-password/${id}/permanent`, { method: 'DELETE' }),

  getData: (params?: { since?: number }) => {
    const qs = new URLSearchParams();
    if (params?.since !== undefined) qs.set('since', String(params.since));
    const query = qs.toString();
    return req<DataPage>(`/data${query ? `?${query}` : ''}`);
  },
};
