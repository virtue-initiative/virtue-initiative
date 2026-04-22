import { GLOBAL_ALERT_EVENT } from "../events";

export interface ToastOptions {
  isError?: boolean;
  centered?: boolean;
  dismissible?: boolean;
  durationMs?: number | null;
}

export function sendToast(message: string, options: ToastOptions = {}) {
  if (typeof window === "undefined") return;
  const event = new CustomEvent(GLOBAL_ALERT_EVENT, {
    detail: {
      message,
      isError: Boolean(options.isError),
      centered: Boolean(options.centered),
      dismissible: options.dismissible ?? true,
      durationMs:
        options.durationMs === undefined ? 45_000 : options.durationMs,
    },
  });
  window.dispatchEvent(event);
}
