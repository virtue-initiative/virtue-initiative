import { createContext } from "preact";
import { useContext } from "preact/hooks";

export type ToastVariant = "error" | "success" | "info";

export type ToastItem = {
  id: string;
  message: string;
  variant: ToastVariant;
  durationMs?: number | null;
  dismissible?: boolean;
  closing?: boolean;
};

type ToastContextValue = {
  toasts: ToastItem[];
  push: (
    message: string,
    variant: ToastVariant,
    opts?: { durationMs?: number | null; dismissible?: boolean },
  ) => void;
  dismiss: (id: string) => void;
};

export const ToastContext = createContext<ToastContextValue | null>(null);

export function useToast() {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToast must be used within ToastProvider");
  return ctx;
}
