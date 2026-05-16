import { ComponentChildren } from 'preact';
import { useState, useCallback, useRef } from 'preact/hooks';
import { ToastContext, ToastItem, ToastVariant } from './useToast';
import { Toast } from './Toast';
import './Toast.css';

export function ToastProvider({ children }: { children: ComponentChildren }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const timeoutsRef = useRef<number[]>([]);

  const dismiss = useCallback((id: string) => {
    setToasts((prev) => prev.map((t) => (t.id === id ? { ...t, closing: true } : t)));
    const t = window.setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, 220);
    timeoutsRef.current.push(t);
  }, []);

  const push = useCallback(
    (
      message: string,
      variant: ToastVariant,
      opts: { durationMs?: number | null; dismissible?: boolean } = {},
    ) => {
      const id = `${Date.now()}-${Math.random().toString(36).slice(2)}`;
      const durationMs = opts.durationMs === undefined ? 5000 : opts.durationMs;
      const item: ToastItem = {
        id,
        message,
        variant,
        durationMs,
        dismissible: opts.dismissible ?? true,
        closing: false,
      };
      setToasts((prev) => [...prev, item]);
      if (durationMs !== null) {
        const t = window.setTimeout(() => dismiss(id), durationMs);
        timeoutsRef.current.push(t);
      }
    },
    [dismiss],
  );

  return (
    <ToastContext.Provider value={{ toasts, push, dismiss }}>
      {children}
      <div class="vi-toast-stack" aria-live="polite" aria-atomic="false">
        {toasts.map((toast) => (
          <Toast key={toast.id} toast={toast} onDismiss={dismiss} />
        ))}
      </div>
    </ToastContext.Provider>
  );
}
