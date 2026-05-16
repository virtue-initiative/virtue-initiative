export interface ToastOptions {
  isError?: boolean;
  centered?: boolean;
  dismissible?: boolean;
  durationMs?: number | null;
}

type PushFn = (
  message: string,
  variant: 'error' | 'success' | 'info',
  opts?: { durationMs?: number | null; dismissible?: boolean },
) => void;

let _push: PushFn | null = null;

export function initToast(push: PushFn) {
  _push = push;
}

export function sendToast(message: string, options: ToastOptions = {}) {
  if (!_push || typeof window === 'undefined') return;
  _push(message, options.isError ? 'error' : 'success', {
    durationMs: options.durationMs === undefined ? 45_000 : options.durationMs,
    dismissible: options.dismissible ?? true,
  });
}
